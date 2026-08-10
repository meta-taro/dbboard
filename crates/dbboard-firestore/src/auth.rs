//! Turning credentials into the `Authorization` header every request carries.
//!
//! Two modes, because the emulator has no credentials: a real project signs a
//! JWT assertion and exchanges it for an access token (cached until shortly
//! before it expires), while the emulator accepts the literal token `owner`.

use std::time::{SystemTime, UNIX_EPOCH};

use dbboard_core::{DbError, DbResult};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::credentials::ServiceAccount;

/// RFC 7523's grant type — the "I signed something with a key you know about"
/// flow, which is how a service account authenticates without a browser.
const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// How early a token is treated as expired.
///
/// A token that is valid when checked but expires while the request is in
/// flight comes back as an intermittent 401 that looks like a permissions
/// problem. Retiring it a minute early costs one extra exchange per hour.
pub(crate) const EXPIRY_SKEW_SECS: u64 = 60;

/// Assumed lifetime when the token endpoint omits `expires_in`. The field is
/// documented but not guaranteed, and "no expiry stated" must not become
/// "cache it for the life of the process".
pub(crate) const DEFAULT_EXPIRY_SECS: u64 = 3600;

/// How a Firestore connection proves who it is.
pub(crate) enum Auth {
    ServiceAccount(Box<ServiceAccountAuth>),
    /// The Firestore emulator ignores credentials and expects this exact
    /// token. Nothing secret travels, because nothing secret exists.
    Emulator,
}

impl Auth {
    pub(crate) fn service_account(account: ServiceAccount, http: reqwest::Client) -> Self {
        Self::ServiceAccount(Box::new(ServiceAccountAuth {
            account,
            http,
            cached: Mutex::new(None),
        }))
    }

    /// The value for the `Authorization` header, exchanging or reusing a token
    /// as needed.
    ///
    /// # Errors
    /// [`DbError::Connection`] if the token endpoint is unreachable or refuses
    /// the credentials.
    pub(crate) async fn bearer(&self) -> DbResult<String> {
        self.bearer_at(now_unix()).await
    }

    /// [`Self::bearer`] with the clock passed in, so cache expiry is testable
    /// without waiting an hour.
    async fn bearer_at(&self, now: u64) -> DbResult<String> {
        match self {
            Self::Emulator => Ok("Bearer owner".to_string()),
            Self::ServiceAccount(auth) => Ok(format!("Bearer {}", auth.token_at(now).await?)),
        }
    }
}

pub(crate) struct ServiceAccountAuth {
    account: ServiceAccount,
    http: reqwest::Client,
    cached: Mutex<Option<AccessToken>>,
}

struct AccessToken {
    value: String,
    expires_at_unix: u64,
}

impl AccessToken {
    fn is_fresh(&self, now: u64) -> bool {
        now + EXPIRY_SKEW_SECS < self.expires_at_unix
    }
}

impl ServiceAccountAuth {
    async fn token_at(&self, now: u64) -> DbResult<String> {
        // The lock is held across the exchange on purpose: a burst of queries
        // on a cold cache should produce one token request, not one per query.
        let mut cached = self.cached.lock().await;
        if let Some(token) = cached.as_ref() {
            if token.is_fresh(now) {
                return Ok(token.value.clone());
            }
        }

        let fetched = self.exchange(now).await?;
        let value = fetched.value.clone();
        *cached = Some(fetched);
        Ok(value)
    }

    async fn exchange(&self, now: u64) -> DbResult<AccessToken> {
        let assertion = self.account.assertion(now)?;
        let response = self
            .http
            .post(&self.account.token_uri)
            .form(&[
                ("grant_type", JWT_BEARER_GRANT),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|e| {
                DbError::Connection(format!(
                    "could not reach the token endpoint: {}",
                    e.without_url()
                ))
            })?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(DbError::Connection(format!(
                "the token endpoint refused the credentials (HTTP {status}){}",
                oauth_detail(&body)
            )));
        }

        let parsed: TokenResponse = serde_json::from_str(&body).map_err(|_| {
            DbError::Connection("the token endpoint returned a response that is not JSON".into())
        })?;
        let value = parsed.access_token.ok_or_else(|| {
            DbError::Connection("the token endpoint returned no access token".into())
        })?;

        Ok(AccessToken {
            expires_at_unix: now + parsed.expires_in.unwrap_or(DEFAULT_EXPIRY_SECS),
            value,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
}

/// The reportable part of a failed token exchange.
///
/// Only the two documented OAuth error fields are quoted back, and only when
/// the body is JSON. A token endpoint's response is the one place an access
/// token is guaranteed to appear, and an error path is exactly where a body
/// tends to get logged — so everything else is dropped rather than trusted.
fn oauth_detail(body: &str) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return String::new();
    };
    let code = json.get("error").and_then(serde_json::Value::as_str);
    let detail = json
        .get("error_description")
        .and_then(serde_json::Value::as_str);
    match (code, detail) {
        (Some(code), Some(detail)) => format!(": {code} — {detail}"),
        (Some(only), None) | (None, Some(only)) => format!(": {only}"),
        (None, None) => String::new(),
    }
}

/// Seconds since the Unix epoch.
///
/// A clock set before 1970 yields 0, which produces an assertion Google
/// rejects outright — the right outcome, since signing a token window from a
/// nonsense clock would fail later and less clearly.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const NOW: u64 = 1_800_000_000;

    fn key_pem() -> &'static str {
        crate::credentials::tests::test_key_pem()
    }

    fn account_for(token_uri: &str) -> ServiceAccount {
        let raw = serde_json::json!({
            "client_email": "firestore@dbboard.example.com",
            "project_id": "demo-project",
            "token_uri": token_uri,
            "private_key": key_pem(),
        })
        .to_string();
        ServiceAccount::from_json(&raw).unwrap()
    }

    fn token_response(value: &str, expires_in: u64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": value,
            "token_type": "Bearer",
            "expires_in": expires_in,
        }))
    }

    fn auth_against(server: &MockServer) -> Auth {
        let token_uri = format!("{}/token", server.uri());
        Auth::service_account(account_for(&token_uri), reqwest::Client::new())
    }

    /// The emulator has no credentials at all; it wants the literal string
    /// `owner`, which grants full access to a throwaway local database.
    #[tokio::test]
    async fn the_emulator_authenticates_as_owner() {
        let header = Auth::Emulator.bearer_at(NOW).await.unwrap();
        assert_eq!(header, "Bearer owner");
    }

    #[tokio::test]
    async fn a_service_account_trades_its_assertion_for_a_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer",
            ))
            .and(body_string_contains("assertion="))
            .respond_with(token_response("ya29.test", 3599))
            .expect(1)
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        assert_eq!(auth.bearer_at(NOW).await.unwrap(), "Bearer ya29.test");
    }

    /// One token covers an hour of queries. Re-exchanging on every request
    /// would add a round-trip to Google in front of every round-trip to
    /// Firestore.
    #[tokio::test]
    async fn a_token_that_is_still_good_is_reused() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_response("ya29.first", 3599))
            .expect(1)
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        assert_eq!(auth.bearer_at(NOW).await.unwrap(), "Bearer ya29.first");
        assert_eq!(
            auth.bearer_at(NOW + 60).await.unwrap(),
            "Bearer ya29.first",
            "the cached token was still valid"
        );
    }

    #[tokio::test]
    async fn an_expired_token_is_exchanged_again() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_response("ya29.rolling", 3599))
            .expect(2)
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        auth.bearer_at(NOW).await.unwrap();
        auth.bearer_at(NOW + 3600).await.unwrap();
    }

    /// A token that expires between the check and the request arriving at
    /// Google fails the request, not the check. Retiring it early is cheaper
    /// than explaining an intermittent 401.
    #[tokio::test]
    async fn a_token_is_retired_before_it_actually_expires() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(token_response("ya29.skew", 3599))
            .expect(2)
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        auth.bearer_at(NOW).await.unwrap();
        auth.bearer_at(NOW + 3599 - EXPIRY_SKEW_SECS).await.unwrap();
    }

    #[tokio::test]
    async fn a_rejected_grant_is_reported_as_a_connection_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Invalid JWT Signature.",
            })))
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        let err = auth.bearer_at(NOW).await.unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
        assert!(
            err.message().contains("400"),
            "the status is the one actionable part: {}",
            err.message()
        );
    }

    /// A 200 with no token is not a success — treating it as one would defer
    /// the failure to the first query, where it reads as a Firestore problem.
    #[tokio::test]
    async fn a_response_without_a_token_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token_type": "Bearer",
            })))
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        assert!(matches!(
            auth.bearer_at(NOW).await.unwrap_err(),
            DbError::Connection(_)
        ));
    }

    /// `expires_in` is documented but not guaranteed. Assuming "forever" on a
    /// missing field would cache a dead token for the life of the process.
    #[tokio::test]
    async fn a_response_without_an_expiry_is_not_cached_forever() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.no-expiry",
            })))
            .expect(2)
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        auth.bearer_at(NOW).await.unwrap();
        auth.bearer_at(NOW + DEFAULT_EXPIRY_SECS).await.unwrap();
    }

    /// A token is a bearer credential: whoever holds it holds the database.
    /// It has no business in a message that may be logged or shown.
    #[tokio::test]
    async fn a_failure_never_carries_a_token_in_its_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(401).set_body_string("ya29.leaked-secret"))
            .mount(&server)
            .await;

        let auth = auth_against(&server);
        let err = auth.bearer_at(NOW).await.unwrap_err();
        let message = err.message();
        assert!(
            !message.contains("ya29."),
            "the error echoed the response body: {message}"
        );
    }
}
