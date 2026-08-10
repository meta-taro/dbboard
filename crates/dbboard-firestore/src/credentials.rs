//! Service-account credentials, and the signed assertion that trades them
//! for an access token.
//!
//! Firestore's REST API takes an `OAuth2` access token, and the only way to get
//! one without a browser is the JWT-bearer grant: sign a short-lived assertion
//! with the service account's private key and exchange it at the token
//! endpoint. The signing happens here; the exchange happens in [`crate::auth`].
//!
//! Signing uses `ring`, which is already in the dependency graph via
//! rustls-ring (ADR-0034 rules out `aws-lc-rs`). That keeps the production
//! crate count unchanged and keeps the private-key operation on a
//! constant-time implementation — the `rsa` crate carries RUSTSEC-2023-0071
//! (Marvin timing sidechannel) on exactly this operation, so it appears here
//! as a dev-dependency only, to generate throwaway test keys.

use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use base64::Engine as _;
use dbboard_core::{DbError, DbResult};
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use serde::Deserialize;

/// The `OAuth2` scope that grants Firestore document access. Firestore inherited
/// Datastore's scope name; there is no `.../auth/firestore`.
pub(crate) const DATASTORE_SCOPE: &str = "https://www.googleapis.com/auth/datastore";

/// Where the assertion is exchanged when the key file does not say.
pub(crate) const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// How long an assertion stays valid. One hour is Google's documented maximum;
/// anything longer is rejected at the endpoint.
pub(crate) const ASSERTION_LIFETIME_SECS: u64 = 3600;

/// A parsed service-account key file, with its private key already accepted by
/// `ring` — so a bad key fails when the connection is configured rather than on
/// the first query.
pub(crate) struct ServiceAccount {
    pub(crate) client_email: String,
    pub(crate) project_id: Option<String>,
    pub(crate) token_uri: String,
    key: RsaKeyPair,
}

// Hand-written so no future `derive` can start printing key material into a
// log line: the fields listed here are the only ones that exist.
impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("client_email", &self.client_email)
            .field("project_id", &self.project_id)
            .field("token_uri", &self.token_uri)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RawServiceAccount {
    client_email: Option<String>,
    project_id: Option<String>,
    token_uri: Option<String>,
    private_key: Option<String>,
}

impl ServiceAccount {
    /// Parse the JSON key file Google hands out for a service account.
    ///
    /// # Errors
    /// [`DbError::Connection`] if the file is not JSON, is missing the email or
    /// the key, or carries a key `ring` will not sign with. The error never
    /// quotes the file back — a credential file is exactly the thing that must
    /// not end up in a message.
    pub(crate) fn from_json(raw: &str) -> DbResult<Self> {
        let parsed: RawServiceAccount = serde_json::from_str(raw)
            .map_err(|_| DbError::Connection("the credentials are not valid JSON".into()))?;

        let client_email = parsed
            .client_email
            .ok_or_else(|| DbError::Connection("the credentials have no `client_email`".into()))?;
        let private_key = parsed
            .private_key
            .ok_or_else(|| DbError::Connection("the credentials have no `private_key`".into()))?;

        let der = pkcs8_der(&private_key)?;
        let key = RsaKeyPair::from_pkcs8(&der).map_err(|_| {
            DbError::Connection(
                "the `private_key` is not a PKCS#8 RSA key usable for RS256 signing".into(),
            )
        })?;

        Ok(Self {
            client_email,
            project_id: parsed.project_id,
            token_uri: parsed
                .token_uri
                .unwrap_or_else(|| DEFAULT_TOKEN_URI.to_string()),
            key,
        })
    }

    /// Build the signed JWT assertion to exchange for an access token.
    ///
    /// `now_unix` is passed in rather than read from the clock so the claim
    /// window is testable.
    ///
    /// # Errors
    /// [`DbError::Connection`] if signing fails, which in practice means the
    /// system RNG is unavailable.
    pub(crate) fn assertion(&self, now_unix: u64) -> DbResult<String> {
        let header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
        let claims = serde_json::json!({
            "iss": self.client_email,
            "scope": DATASTORE_SCOPE,
            "aud": self.token_uri,
            "iat": now_unix,
            "exp": now_unix + ASSERTION_LIFETIME_SECS,
        });

        let signing_input = format!("{}.{}", segment(&header), segment(&claims));
        let mut signature = vec![0u8; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signing_input.as_bytes(),
                &mut signature,
            )
            .map_err(|_| DbError::Connection("could not sign the token request".into()))?;

        Ok(format!("{signing_input}.{}", B64URL.encode(signature)))
    }
}

/// Encode one JWT segment. `serde_json::Value` always serialises, so there is
/// nothing here that can fail.
fn segment(value: &serde_json::Value) -> String {
    B64URL.encode(value.to_string())
}

/// Pull the DER body out of a PEM block.
///
/// Deliberately matches on the five-dash delimiter rather than on the header
/// text: writing the literal armor of a private key into a tracked file is a
/// *blocking* finding for `scripts/pii-scan.sh`, and matching it buys nothing —
/// every armor line is delimited the same way, and whatever is left is the
/// base64 body. A body that is not a PKCS#8 key is rejected by `ring` a moment
/// later anyway.
fn pkcs8_der(pem: &str) -> DbResult<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect();

    if body.is_empty() {
        return Err(DbError::Connection("the `private_key` is empty".into()));
    }

    B64.decode(body)
        .map_err(|_| DbError::Connection("the `private_key` is not valid base64".into()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// A throwaway 2048-bit key, generated once per test binary.
    ///
    /// Generated rather than committed: a PKCS#8 PEM block in a tracked file
    /// is exactly what `scripts/pii-scan.sh` blocks, and committing a private
    /// key to a public repository is wrong even when the key is worthless.
    /// 2048 is the floor `ring` accepts for signing, so a smaller/faster key
    /// would not exercise the real path.
    pub(crate) fn test_key_pem() -> &'static str {
        static PEM: OnceLock<String> = OnceLock::new();
        PEM.get_or_init(|| {
            use rsa::pkcs8::{EncodePrivateKey, LineEnding};
            let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048)
                .expect("test key generation");
            key.to_pkcs8_pem(LineEnding::LF)
                .expect("pkcs8 encoding")
                .to_string()
        })
    }

    fn credentials_json() -> String {
        serde_json::json!({
            "type": "service_account",
            "project_id": "demo-project",
            "client_email": "firestore@dbboard.example.com",
            "token_uri": "https://oauth2.googleapis.com/token",
            "private_key": test_key_pem(),
        })
        .to_string()
    }

    #[test]
    fn a_service_account_carries_its_email_project_and_token_endpoint() {
        let account = ServiceAccount::from_json(&credentials_json()).unwrap();
        assert_eq!(account.client_email, "firestore@dbboard.example.com");
        assert_eq!(account.project_id.as_deref(), Some("demo-project"));
        assert_eq!(account.token_uri, "https://oauth2.googleapis.com/token");
    }

    /// Older key files omit `token_uri`. Google's endpoint is the only
    /// possible value for a service-account grant, so defaulting is safer
    /// than refusing a file that works everywhere else.
    #[test]
    fn a_missing_token_uri_falls_back_to_googles_endpoint() {
        let raw = serde_json::json!({
            "client_email": "firestore@dbboard.example.com",
            "private_key": test_key_pem(),
        })
        .to_string();
        let account = ServiceAccount::from_json(&raw).unwrap();
        assert_eq!(account.token_uri, DEFAULT_TOKEN_URI);
    }

    #[test]
    fn a_file_that_is_not_json_is_refused() {
        let err = ServiceAccount::from_json("not json at all").unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
    }

    #[test]
    fn a_file_without_a_key_is_refused() {
        let raw = serde_json::json!({ "client_email": "a@dbboard.example.com" }).to_string();
        let err = ServiceAccount::from_json(&raw).unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
    }

    #[test]
    fn a_key_that_is_not_pkcs8_is_refused() {
        let raw = serde_json::json!({
            "client_email": "firestore@dbboard.example.com",
            "private_key": "-----BEGIN NONSENSE-----\nZm9v\n-----END NONSENSE-----\n",
        })
        .to_string();
        let err = ServiceAccount::from_json(&raw).unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
    }

    /// The whole point of the failure paths above: a malformed credential
    /// file must not echo its own contents into a message that reaches a log
    /// line or the UI.
    #[test]
    fn a_rejected_key_is_never_quoted_back_in_the_error() {
        let pem = test_key_pem();
        let raw = serde_json::json!({
            "client_email": "firestore@dbboard.example.com",
            // Valid PEM armor, truncated body — parses as base64, fails as a key.
            "private_key": format!("{}\n", pem.lines().take(3).collect::<Vec<_>>().join("\n")),
        })
        .to_string();
        let err = ServiceAccount::from_json(&raw).unwrap_err();
        let message = err.message();
        for line in pem.lines().filter(|l| l.len() > 20) {
            assert!(
                !message.contains(line),
                "the error quoted key material back: {message}"
            );
        }
    }

    #[test]
    fn the_assertion_is_three_dot_separated_segments() {
        let account = ServiceAccount::from_json(&credentials_json()).unwrap();
        let assertion = account.assertion(1_800_000_000).unwrap();
        assert_eq!(assertion.split('.').count(), 3);
    }

    fn decode_segment(segment: &str) -> serde_json::Value {
        use base64::Engine as _;
        let bytes = B64URL.decode(segment).expect("base64url segment");
        serde_json::from_slice(&bytes).expect("json segment")
    }

    #[test]
    fn the_assertion_header_names_rs256() {
        let account = ServiceAccount::from_json(&credentials_json()).unwrap();
        let assertion = account.assertion(1_800_000_000).unwrap();
        let header = decode_segment(assertion.split('.').next().unwrap());
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");
    }

    #[test]
    fn the_assertion_claims_the_datastore_scope_for_a_bounded_window() {
        let account = ServiceAccount::from_json(&credentials_json()).unwrap();
        let assertion = account.assertion(1_800_000_000).unwrap();
        let claims = decode_segment(assertion.split('.').nth(1).unwrap());
        assert_eq!(claims["iss"], "firestore@dbboard.example.com");
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["scope"], DATASTORE_SCOPE);
        assert_eq!(claims["iat"], 1_800_000_000_u64);
        assert_eq!(
            claims["exp"],
            1_800_000_000_u64 + ASSERTION_LIFETIME_SECS,
            "an assertion that never expires is a bearer credential"
        );
    }

    /// Google will reject an assertion signed wrong, but it would reject it
    /// with an opaque `invalid_grant` after a round-trip. Verifying locally
    /// means a signing regression fails here instead.
    #[test]
    fn the_signature_verifies_against_the_public_key() {
        use base64::Engine as _;
        use rsa::pkcs1v15::{Signature, VerifyingKey};
        use rsa::pkcs8::DecodePrivateKey;
        use rsa::signature::Verifier;
        use sha2::Sha256;

        let account = ServiceAccount::from_json(&credentials_json()).unwrap();
        let assertion = account.assertion(1_800_000_000).unwrap();
        let (signed, signature) = assertion.rsplit_once('.').unwrap();

        let private = rsa::RsaPrivateKey::from_pkcs8_pem(test_key_pem()).unwrap();
        let verifying = VerifyingKey::<Sha256>::new(private.to_public_key());
        let bytes = B64URL.decode(signature).unwrap();
        verifying
            .verify(
                signed.as_bytes(),
                &Signature::try_from(bytes.as_slice()).unwrap(),
            )
            .expect("the assertion did not verify under its own key");
    }

    /// A JWT segment carrying `+`, `/`, or `=` is not a JWT — the standard
    /// encoding is base64url without padding, and Google rejects the rest.
    #[test]
    fn every_segment_is_unpadded_base64url() {
        let account = ServiceAccount::from_json(&credentials_json()).unwrap();
        let assertion = account.assertion(1_800_000_000).unwrap();
        for segment in assertion.split('.') {
            assert!(
                !segment.contains(['+', '/', '=']),
                "segment is not base64url: {segment}"
            );
        }
    }
}
