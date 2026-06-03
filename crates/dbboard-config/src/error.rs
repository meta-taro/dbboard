//! Crate-local error type.
//!
//! Covers schema parsing, schema-version mismatch, duplicate ids,
//! filesystem I/O around `connections.toml`, and secret-store failures
//! surfaced from [`crate::secrets`]. Drift we surface loudly rather
//! than dropping silently.

use thiserror::Error;

use crate::secrets::SecretError;

/// Errors that can occur while loading or validating a connection store.
///
/// Config errors live below the HTTP surface: they are raised during
/// process startup, before the loopback server binds, so they never
/// reach the `{category, message}` envelope defined in
/// `docs/api-contract.md`.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The TOML payload could not be parsed at all.
    #[error("config parse failed: {0}")]
    Parse(#[from] toml::de::Error),

    /// `version` does not equal the single supported value
    /// ([`crate::CONFIG_VERSION`]). We refuse to guess at a forward- or
    /// backward-incompatible shape.
    #[error("unsupported config version: {0} (only version {expected} is supported)", expected = crate::CONFIG_VERSION)]
    UnsupportedVersion(u32),

    /// Two `[[connections]]` entries share the same `id`. Ids are the
    /// primary key used by `DBBOARD_CONNECTION` and by the future
    /// connection picker, so collisions are a hard error.
    #[error("duplicate connection id: {0}")]
    DuplicateId(String),

    /// Filesystem read or write failed. The path is *not* embedded so
    /// the message can be surfaced in logs without leaking a home
    /// directory; callers attach the path when they have it.
    #[error("config io failed: {0}")]
    Io(#[from] std::io::Error),

    /// Re-serializing the in-memory store back to TOML failed. With our
    /// schema this should only happen if a future variant carries data
    /// that the `toml` crate cannot represent.
    #[error("config serialize failed: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// The OS reported no usable per-user config directory. This is
    /// extremely rare on a real desktop (no `$HOME`, no
    /// `%APPDATA%`); we surface it rather than silently choosing the
    /// process working directory.
    #[error("could not resolve a per-user config directory")]
    NoConfigDir,

    /// The keyring / in-memory secret store reported a failure while
    /// resolving a `keyring_*_ref` referenced from the TOML.
    #[error("config secret failed: {0}")]
    Secret(#[from] SecretError),
}
