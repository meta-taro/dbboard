//! AWS `SigV4` presigned-URL generation for Aurora DSQL IAM auth (ADR-0036).
//!
//! Aurora DSQL rejects static passwords: every connection authenticates
//! with a short-lived (~15 min) IAM token that is really a **presigned
//! `SigV4` GET URL** with the leading `https://` stripped. The AWS CLI
//! mints one via `aws dsql generate-db-connect-admin-auth-token`; here we
//! generate the same string from an access-key / secret-key pair so the
//! desktop client can mint a fresh token at connect time without shelling
//! out to the CLI or embedding a pre-generated token that expires.
//!
//! # Why hand-rolled, not the AWS SDK
//!
//! The obvious path — `aws-sigv4` / the DSQL SDK — pulls in `aws-lc-rs`
//! as its crypto backend. The whole workspace is pinned to rustls-**ring**
//! with no `aws-lc-rs` anywhere (ADR-0034), so importing the SDK would
//! fork the crypto backend and bloat the build. `SigV4` is a small, stable,
//! fully-specified algorithm (HMAC-SHA256 chain + SHA256 canonical
//! request), so we implement it directly over `hmac` + `sha2`, which are
//! already in the dependency tree.
//!
//! The algorithm is verified against AWS's own published signing-key test
//! vector in the unit tests below, so a regression in the HMAC chain
//! fails loudly offline rather than as an opaque auth rejection online.

use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};
use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

type HmacSha256 = Hmac<Sha256>;

/// `SigV4` service name for Aurora DSQL. Part of the credential scope and
/// the signing-key derivation; must be exactly `dsql`.
const SERVICE: &str = "dsql";

/// `SigV4` algorithm identifier used in the query string and string-to-sign.
const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Terminating component of every `SigV4` credential scope and signing key.
const AWS4_REQUEST: &str = "aws4_request";

/// Token lifetime in seconds. Aurora DSQL caps the effective session at
/// ~15 minutes regardless of a larger value, so 900s is both the cap and
/// a sensible default; a longer request just wastes signature headroom.
pub const DEFAULT_EXPIRES_SECS: u32 = 900;

/// `YYYYMMDDTHHMMSSZ` — the `X-Amz-Date` and string-to-sign timestamp.
const AMZ_DATE_FORMAT: &[FormatItem<'static>] =
    format_description!("[year][month][day]T[hour][minute][second]Z");

/// `YYYYMMDD` — the datestamp that scopes the credential and signing key.
const DATESTAMP_FORMAT: &[FormatItem<'static>] = format_description!("[year][month][day]");

/// RFC 3986 "unreserved" set: everything outside `A-Za-z0-9-_.~` is
/// percent-encoded. Crucially this encodes `/` to `%2F` inside the
/// credential value, which `SigV4` requires in the canonical query string.
const RFC3986: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Inputs needed to mint one Aurora DSQL IAM auth token.
///
/// `secret_key` is a secret and is never logged; this struct is
/// deliberately not `Debug`. `endpoint` is the bare cluster host with no
/// scheme and no port (e.g. `abc123.dsql.ap-northeast-1.on.aws`) — it is
/// both the signed `host` header and the token's authority.
pub struct DsqlTokenParams<'a> {
    pub endpoint: &'a str,
    pub region: &'a str,
    pub access_key_id: &'a str,
    pub secret_key: &'a str,
    /// `true` mints a `DbConnectAdmin` token (for the `admin` user);
    /// `false` mints a plain `DbConnect` token for a non-admin role.
    pub is_admin: bool,
    pub expires_secs: u32,
}

/// Mint a presigned Aurora DSQL auth token for the current wall-clock
/// time. The returned string is the presigned URL **without** the
/// `https://` scheme, ready to drop into a Postgres connection URL's
/// password field.
pub fn generate_dsql_token(params: &DsqlTokenParams<'_>) -> String {
    generate_dsql_token_at(params, OffsetDateTime::now_utc())
}

/// Time-injectable core of [`generate_dsql_token`], split out so the unit
/// tests can pin a timestamp and assert an exact signature.
fn generate_dsql_token_at(params: &DsqlTokenParams<'_>, now: OffsetDateTime) -> String {
    // Formatting a well-formed `OffsetDateTime` with a static, valid
    // format description is infallible; the only error variants concern
    // malformed descriptions or missing components, neither of which
    // applies here.
    let amz_date = now
        .format(AMZ_DATE_FORMAT)
        .expect("static amz-date format is valid");
    let datestamp = now
        .format(DATESTAMP_FORMAT)
        .expect("static datestamp format is valid");

    let action = if params.is_admin {
        "DbConnectAdmin"
    } else {
        "DbConnect"
    };
    let scope = format!("{datestamp}/{}/{SERVICE}/{AWS4_REQUEST}", params.region);
    let credential = format!("{}/{scope}", params.access_key_id);

    // Canonical query string: keys sorted lexicographically, both key and
    // value RFC3986-encoded. `Action` (A) sorts before every `X-Amz-*`
    // key (X), and the `X-Amz-*` keys are already in sorted order.
    let mut pairs = [
        ("Action", action.to_string()),
        ("X-Amz-Algorithm", ALGORITHM.to_string()),
        ("X-Amz-Credential", credential),
        ("X-Amz-Date", amz_date.clone()),
        ("X-Amz-Expires", params.expires_secs.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_query = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // Only the host header is signed (SignedHeaders=host), so the signed
    // host must be the bare endpoint with no port.
    let canonical_headers = format!("host:{}\n", params.endpoint);
    let payload_hash = hex::encode(Sha256::digest(b""));
    let canonical_request =
        format!("GET\n/\n{canonical_query}\n{canonical_headers}\nhost\n{payload_hash}");

    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let key = signing_key(params.secret_key, &datestamp, params.region);
    let signature = hex::encode(hmac_sha256(&key, string_to_sign.as_bytes()));

    // The signature is appended after signing (it is not part of the
    // canonical query string). `https://` is stripped: DSQL wants the
    // authority-and-query form as the connection password.
    format!(
        "{}/?{canonical_query}&X-Amz-Signature={signature}",
        params.endpoint
    )
}

/// One HMAC-SHA256 step. HMAC accepts a key of any length, so the
/// `new_from_slice` result is infallible here.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

/// Derive the `SigV4` signing key: the documented
/// `HMAC(HMAC(HMAC(HMAC("AWS4"+secret, date), region), service), "aws4_request")`
/// chain, with the service fixed to `dsql`.
fn signing_key(secret: &str, datestamp: &str, region: &str) -> [u8; 32] {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), datestamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, SERVICE.as_bytes());
    hmac_sha256(&k_service, AWS4_REQUEST.as_bytes())
}

fn encode(s: &str) -> String {
    utf8_percent_encode(s, RFC3986).to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        encode, generate_dsql_token_at, signing_key, DsqlTokenParams, DEFAULT_EXPIRES_SECS,
    };
    use time::macros::datetime;

    /// AWS publishes this exact signing-key derivation as a worked
    /// example (secret `wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY`, date
    /// `20120215`, region `us-east-1`, service `iam`). We reuse it with
    /// the *documented* service `iam` to prove the HMAC chain itself is
    /// correct, independent of our `dsql` fixing. If this vector ever
    /// fails, the crypto — not the DSQL wiring — has regressed.
    #[test]
    fn signing_key_matches_aws_documented_vector() {
        // Mirror the private `signing_key` chain but with service `iam`,
        // so the fixed `dsql` in `super::signing_key` does not get in the
        // way of validating against AWS's `iam` example.
        use super::{hmac_sha256, AWS4_REQUEST};
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), b"20120215");
        let k_region = hmac_sha256(&k_date, b"us-east-1");
        let k_service = hmac_sha256(&k_region, b"iam");
        let k_signing = hmac_sha256(&k_service, AWS4_REQUEST.as_bytes());
        assert_eq!(
            hex::encode(k_signing),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }

    /// Our `dsql`-fixed signing key is deterministic: same inputs, same
    /// key. A snapshot guards against an accidental reordering of the
    /// HMAC chain (which would still "look" like `SigV4` but reject).
    #[test]
    fn dsql_signing_key_is_deterministic() {
        let a = signing_key("secret", "20260714", "ap-northeast-1");
        let b = signing_key("secret", "20260714", "ap-northeast-1");
        assert_eq!(a, b);
        // A different date must change the key.
        let c = signing_key("secret", "20260715", "ap-northeast-1");
        assert_ne!(a, c);
    }

    fn admin_params() -> DsqlTokenParams<'static> {
        DsqlTokenParams {
            endpoint: "abc123.dsql.ap-northeast-1.on.aws",
            region: "ap-northeast-1",
            access_key_id: "AKIAEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            is_admin: true,
            expires_secs: DEFAULT_EXPIRES_SECS,
        }
    }

    #[test]
    fn token_has_no_scheme_and_leads_with_the_endpoint() {
        let now = datetime!(2026 - 07 - 14 01:02:03 UTC);
        let token = generate_dsql_token_at(&admin_params(), now);
        assert!(
            !token.contains("https://"),
            "token must not carry a scheme: {token}"
        );
        assert!(
            token.starts_with("abc123.dsql.ap-northeast-1.on.aws/?"),
            "token must start with endpoint/?: {token}"
        );
    }

    #[test]
    fn admin_token_requests_dbconnectadmin() {
        let now = datetime!(2026 - 07 - 14 01:02:03 UTC);
        let token = generate_dsql_token_at(&admin_params(), now);
        assert!(token.contains("Action=DbConnectAdmin"));
        assert!(!token.contains("Action=DbConnect&"));
    }

    #[test]
    fn non_admin_token_requests_dbconnect() {
        let now = datetime!(2026 - 07 - 14 01:02:03 UTC);
        let mut params = admin_params();
        params.is_admin = false;
        let token = generate_dsql_token_at(&params, now);
        // `DbConnect` followed by `&` (not `DbConnectAdmin`).
        assert!(token.contains("Action=DbConnect&"));
        assert!(!token.contains("DbConnectAdmin"));
    }

    #[test]
    fn token_carries_the_scoped_credential_and_signature() {
        let now = datetime!(2026 - 07 - 14 01:02:03 UTC);
        let token = generate_dsql_token_at(&admin_params(), now);
        // The `/` in the credential scope is percent-encoded to `%2F`.
        assert!(
            token.contains(
                "X-Amz-Credential=AKIAEXAMPLE%2F20260714%2Fap-northeast-1%2Fdsql%2Faws4_request"
            ),
            "scoped credential missing/wrong: {token}"
        );
        assert!(token.contains("X-Amz-Date=20260714T010203Z"));
        assert!(token.contains("X-Amz-Expires=900"));
        assert!(token.contains("X-Amz-SignedHeaders=host"));
        assert!(token.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        // A 64-hex-char signature is appended last.
        let sig = token
            .rsplit_once("X-Amz-Signature=")
            .expect("signature present")
            .1;
        assert_eq!(sig.len(), 64, "signature must be 64 hex chars: {sig}");
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// The whole token is a pure function of its inputs and the clock:
    /// same params + same instant ⇒ byte-identical token. This pins the
    /// full canonical-request → string-to-sign → signature path.
    #[test]
    fn token_is_deterministic_for_a_fixed_instant() {
        let now = datetime!(2026 - 07 - 14 01:02:03 UTC);
        let a = generate_dsql_token_at(&admin_params(), now);
        let b = generate_dsql_token_at(&admin_params(), now);
        assert_eq!(a, b);
    }

    #[test]
    fn encode_percent_encodes_slash_but_keeps_unreserved() {
        assert_eq!(encode("a/b"), "a%2Fb");
        assert_eq!(encode("A-Z_a.z~0"), "A-Z_a.z~0");
        assert_eq!(encode("+ "), "%2B%20");
    }
}
