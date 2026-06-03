//! Local user configuration for dbboard (ADR-0013).
//!
//! Owns two halves that must never blur into one another:
//!
//! - [`store`] — the on-disk shape of `connections.toml`. This module is
//!   serde-only at the schema layer; filesystem reads/writes land in a
//!   later commit.
//! - [`error`] — crate-local error type. Config errors happen at process
//!   startup, before the loopback server binds, so they never reach the
//!   HTTP envelope.
//!
//! The crate exists because `dbboard-core` is "no I/O" (ADR-0002 /
//! ADR-0009) and `apps/dbboard` is wiring only — neither is the right
//! home for filesystem + OS-keychain persistence.

pub mod error;
pub mod store;

pub use error::ConfigError;
pub use store::{ConnectionEntry, ConnectionFile, ConnectionKind, CONFIG_VERSION};
