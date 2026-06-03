//! Secret storage abstraction (ADR-0013).
//!
//! The TOML connection store holds only opaque keyring references
//! (`keyring_token_ref`, `keyring_url_ref`); the actual secret material
//! lives in a [`SecretStore`]. Two implementations ship with the crate:
//!
//! - [`KeyringStore`] wraps the `keyring` crate, mapping uniformly to
//!   Windows Credential Manager, macOS Keychain, and Linux Secret
//!   Service.
//! - [`InMemorySecretStore`] is a `HashMap` for tests, CI, and as a
//!   fallback on hosts without a usable secret service.
//!
//! All secrets stored through this crate use the constant service name
//! `"dbboard"` so a user can wipe everything dbboard owns through the
//! OS UI by that single string.

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;

/// Service string used for every keyring entry written by dbboard.
///
/// Kept short and constant so the OS UI groups dbboard's credentials
/// under a single recognisable name. The per-entry differentiation
/// lives in the account string (the `keyring_*_ref` value from the
/// TOML).
pub const KEYRING_SERVICE: &str = "dbboard";

/// Failure modes for a [`SecretStore`] operation.
///
/// Kept narrow on purpose: callers should rarely need to branch on the
/// concrete keyring backend's failure modes.
#[derive(Debug, Error)]
pub enum SecretError {
    /// No secret is stored for the supplied reference. For
    /// [`KeyringStore`] this means the OS keychain has no entry under
    /// `(KEYRING_SERVICE, key_ref)`.
    #[error("no secret stored for reference: {0}")]
    NotFound(String),

    /// The OS reported a usable secret service but the request itself
    /// failed (e.g. the user denied access from a prompt, or the
    /// platform API returned an unexpected error).
    #[error("secret backend error for {key_ref}: {source}")]
    Backend {
        key_ref: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Read / write / delete secrets keyed by an opaque reference string.
///
/// Implementations must be `Send + Sync` so they can be shared as
/// `Arc<dyn SecretStore>` across the `apps/dbboard` startup wiring.
pub trait SecretStore: Send + Sync {
    /// Fetch the secret stored under `key_ref`.
    ///
    /// # Errors
    ///
    /// - [`SecretError::NotFound`] if no entry exists.
    /// - [`SecretError::Backend`] for any other failure.
    fn get(&self, key_ref: &str) -> Result<String, SecretError>;

    /// Store `value` under `key_ref`, replacing any previous secret
    /// with the same reference.
    ///
    /// # Errors
    ///
    /// [`SecretError::Backend`] for any backend failure.
    fn set(&self, key_ref: &str, value: &str) -> Result<(), SecretError>;

    /// Remove the secret stored under `key_ref`.
    ///
    /// # Errors
    ///
    /// - [`SecretError::NotFound`] if no entry exists.
    /// - [`SecretError::Backend`] for any other failure.
    fn delete(&self, key_ref: &str) -> Result<(), SecretError>;
}

/// In-memory store, intended for tests, CI, and as a fallback on hosts
/// without a usable OS keychain. Not persistent across process restarts.
#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    inner: Mutex<HashMap<String, String>>,
}

impl InMemorySecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for InMemorySecretStore {
    fn get(&self, key_ref: &str) -> Result<String, SecretError> {
        let guard = self
            .inner
            .lock()
            .expect("InMemorySecretStore mutex poisoned");
        guard
            .get(key_ref)
            .cloned()
            .ok_or_else(|| SecretError::NotFound(key_ref.to_string()))
    }

    fn set(&self, key_ref: &str, value: &str) -> Result<(), SecretError> {
        let mut guard = self
            .inner
            .lock()
            .expect("InMemorySecretStore mutex poisoned");
        guard.insert(key_ref.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        let mut guard = self
            .inner
            .lock()
            .expect("InMemorySecretStore mutex poisoned");
        if guard.remove(key_ref).is_none() {
            return Err(SecretError::NotFound(key_ref.to_string()));
        }
        Ok(())
    }
}

/// OS keychain backed store. Backed by the `keyring` crate so the same
/// type works on Windows (Credential Manager), macOS (Keychain), and
/// Linux (Secret Service).
///
/// This struct is a zero-sized handle; the keyring backend is resolved
/// per-call by `keyring::Entry::new`.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

impl KeyringStore {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn entry(key_ref: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(KEYRING_SERVICE, key_ref).map_err(|err| SecretError::Backend {
            key_ref: key_ref.to_string(),
            source: Box::new(err),
        })
    }

    fn map_err(key_ref: &str, err: keyring::Error) -> SecretError {
        if matches!(err, keyring::Error::NoEntry) {
            SecretError::NotFound(key_ref.to_string())
        } else {
            SecretError::Backend {
                key_ref: key_ref.to_string(),
                source: Box::new(err),
            }
        }
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key_ref: &str) -> Result<String, SecretError> {
        Self::entry(key_ref)?
            .get_password()
            .map_err(|err| Self::map_err(key_ref, err))
    }

    fn set(&self, key_ref: &str, value: &str) -> Result<(), SecretError> {
        Self::entry(key_ref)?
            .set_password(value)
            .map_err(|err| Self::map_err(key_ref, err))
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        Self::entry(key_ref)?
            .delete_credential()
            .map_err(|err| Self::map_err(key_ref, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_service_constant_is_short_and_stable() {
        assert_eq!(KEYRING_SERVICE, "dbboard");
    }

    #[test]
    fn in_memory_get_unknown_key_is_not_found() {
        let store = InMemorySecretStore::new();
        let err = store.get("missing").expect_err("unknown key must fail");
        match &err {
            SecretError::NotFound(key) => assert_eq!(key, "missing"),
            SecretError::Backend { .. } => panic!("expected NotFound, got {err:?}"),
        }
    }

    #[test]
    fn in_memory_set_then_get_returns_the_stored_value() {
        let store = InMemorySecretStore::new();
        store.set("dbboard.x.token", "s3cret").expect("set");
        assert_eq!(store.get("dbboard.x.token").expect("get"), "s3cret");
    }

    #[test]
    fn in_memory_set_overwrites_existing_value() {
        let store = InMemorySecretStore::new();
        store.set("k", "first").expect("first set");
        store.set("k", "second").expect("second set");
        assert_eq!(store.get("k").expect("get"), "second");
    }

    #[test]
    fn in_memory_delete_removes_the_value() {
        let store = InMemorySecretStore::new();
        store.set("k", "v").expect("set");
        store.delete("k").expect("delete");
        let err = store.get("k").expect_err("get after delete must fail");
        assert!(matches!(err, SecretError::NotFound(_)));
    }

    #[test]
    fn in_memory_delete_unknown_key_is_not_found() {
        let store = InMemorySecretStore::new();
        let err = store
            .delete("never-existed")
            .expect_err("delete of unknown key must fail");
        match &err {
            SecretError::NotFound(key) => assert_eq!(key, "never-existed"),
            SecretError::Backend { .. } => panic!("expected NotFound, got {err:?}"),
        }
    }

    #[test]
    fn in_memory_store_is_shareable_as_a_trait_object() {
        let store: std::sync::Arc<dyn SecretStore> =
            std::sync::Arc::new(InMemorySecretStore::new());
        store.set("k", "v").expect("set via trait object");
        assert_eq!(store.get("k").expect("get via trait object"), "v");
    }

    /// Live keyring round-trip — marked `#[ignore]` so default
    /// `cargo test --all-features` runs (including the pre-commit
    /// hook) stay green on hosts without a Secret Service. Opt in
    /// with `cargo test -p dbboard-config -- --ignored`. The test key
    /// is unique per run so a stale entry left by a crashed previous
    /// run cannot pass the assertion.
    #[test]
    #[ignore = "touches the live OS keychain; run with --ignored when wanted"]
    fn keyring_store_round_trips_through_the_os_keychain() {
        let store = KeyringStore::new();
        let key_ref = format!(
            "dbboard.test.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        store.set(&key_ref, "live-secret").expect("live set");
        let read = store.get(&key_ref).expect("live get");
        assert_eq!(read, "live-secret");
        store.delete(&key_ref).expect("live delete");
        let err = store.get(&key_ref).expect_err("get after delete");
        assert!(matches!(err, SecretError::NotFound(_)));
    }
}
