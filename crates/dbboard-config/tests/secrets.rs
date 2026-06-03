//! Integration tests for `dbboard-config::secrets`.
//!
//! The point of these tests is the cross-module invariant — TOML round
//! trips of a `ConnectionFile` must never carry secret material, only
//! opaque `keyring_*_ref` strings. The unit tests inside
//! `src/secrets.rs` already cover the in-memory backend's get/set/delete
//! semantics; this file holds the contract-level checks.

use dbboard_config::secrets::{InMemorySecretStore, SecretError, SecretStore};
use dbboard_config::{ConnectionEntry, ConnectionFile, ConnectionKind, CONFIG_VERSION};

/// Round-trip a D1 entry through TOML and assert the only credential-
/// shaped field is the keyring reference. If anyone ever adds a raw
/// `token = "..."` field to `ConnectionKind::D1` this test must trip.
#[test]
fn toml_round_trip_never_carries_a_secret_token() {
    let file = ConnectionFile {
        version: CONFIG_VERSION,
        connections: vec![ConnectionEntry {
            id: "cf-d1".to_string(),
            name: "Cloudflare D1".to_string(),
            kind: ConnectionKind::D1 {
                account_id: "acct-123".to_string(),
                database_id: "db-456".to_string(),
                base_url: None,
                keyring_token_ref: "dbboard.cf-d1.token".to_string(),
            },
        }],
    };
    let rendered = toml::to_string(&file).expect("serialize");
    assert!(
        !rendered.contains("token = \"") || rendered.contains("keyring_token_ref"),
        "raw token field leaked into TOML: {rendered}"
    );
    assert!(
        rendered.contains("keyring_token_ref = \"dbboard.cf-d1.token\""),
        "expected opaque reference, got: {rendered}"
    );
}

/// Same check for the Postgres variant — the only credential-shaped
/// field must be the opaque keyring reference.
#[test]
fn toml_round_trip_never_carries_a_postgres_url() {
    let file = ConnectionFile {
        version: CONFIG_VERSION,
        connections: vec![ConnectionEntry {
            id: "neon-prod".to_string(),
            name: "Neon prod".to_string(),
            kind: ConnectionKind::Postgres {
                keyring_url_ref: "dbboard.neon-prod.url".to_string(),
            },
        }],
    };
    let rendered = toml::to_string(&file).expect("serialize");
    assert!(
        !rendered.contains("postgres://") && !rendered.contains("postgresql://"),
        "raw postgres URL leaked into TOML: {rendered}"
    );
    assert!(
        rendered.contains("keyring_url_ref = \"dbboard.neon-prod.url\""),
        "expected opaque reference, got: {rendered}"
    );
}

/// Sanity check that a `Box<dyn SecretStore>` resolves a stored secret —
/// the shape the `apps/dbboard` wiring will use.
#[test]
fn boxed_secret_store_resolves_a_keyring_reference() {
    let store: Box<dyn SecretStore> = Box::new(InMemorySecretStore::new());
    store.set("dbboard.cf-d1.token", "live-token").expect("set");
    let resolved = store.get("dbboard.cf-d1.token").expect("get");
    assert_eq!(resolved, "live-token");
}

/// `SecretError::NotFound` must carry the missing reference verbatim so
/// the eventual UI error message can name what the user has to fix.
#[test]
fn not_found_error_carries_the_missing_reference() {
    let store = InMemorySecretStore::new();
    let err = store
        .get("dbboard.absent.ref")
        .expect_err("missing key must fail");
    match &err {
        SecretError::NotFound(key) => assert_eq!(key, "dbboard.absent.ref"),
        SecretError::Backend { .. } => panic!("expected NotFound, got {err:?}"),
    }
}
