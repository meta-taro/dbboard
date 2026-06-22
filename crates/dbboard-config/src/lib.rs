//! Local user configuration for dbboard (ADR-0013).
//!
//! Owns three halves that must never blur into one another:
//!
//! - [`store`] — the on-disk shape of `connections.toml`, plus the
//!   filesystem layer (`load_or_empty`, `save_atomic`) and the
//!   `default_history_path()` helper that `dbboard-ui` uses to find
//!   `history.jsonl` (ADR-0017) under the same config dir.
//! - [`secrets`] — `SecretStore` trait with an OS-keychain backend
//!   ([`secrets::KeyringStore`]) and an in-memory fallback
//!   ([`secrets::InMemorySecretStore`]) for tests and CI.
//! - [`error`] — crate-local error type. Config errors happen at process
//!   startup, before the loopback server binds, so they never reach the
//!   HTTP envelope.
//!
//! The crate exists because `dbboard-core` is "no I/O" (ADR-0002 /
//! ADR-0009) and `apps/dbboard` is wiring only — neither is the right
//! home for filesystem + OS-keychain persistence.

pub mod admin;
pub mod error;
pub mod secrets;
pub mod secure_fs;
pub mod store;

pub use admin::{
    ConnectionAdmin, ConnectionDraft, ConnectionEditDraft, ConnectionKindDraft,
    ConnectionKindEditDraft, SecretField,
};
pub use error::ConfigError;
pub use secrets::{InMemorySecretStore, KeyringStore, SecretError, SecretStore, KEYRING_SERVICE};
pub use store::{
    default_history_path, default_path, ConnectionEntry, ConnectionFile, ConnectionKind,
    CONFIG_VERSION,
};
