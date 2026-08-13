//! Crate-local error type.
//!
//! Covers schema parsing, schema-version mismatch, duplicate ids,
//! filesystem I/O around `connections.toml`, and secret-store failures
//! surfaced from [`crate::secrets`]. Drift we surface loudly rather
//! than dropping silently.

use thiserror::Error;

use crate::bundle::BundleError;
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

    /// An `mcp_alias` (ADR-0088) collides with another entry's alias or id.
    /// The alias is the handle an agent hands back, and the resolver tries
    /// aliases before ids, so a collision would route a query to whichever
    /// entry the resolver happened to see first.
    #[error("connection alias already in use: {0}")]
    DuplicateAlias(String),

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

    /// `ConnectionAdmin::{update, delete}` was called with an id that
    /// no entry in the store matches. Surfaced loudly because the
    /// caller is almost certainly using a stale view of the entries
    /// vector (ADR-0016).
    #[error("no connection entry with id: {0}")]
    NotFound(String),

    /// `ConnectionAdmin::update` was called with a draft whose
    /// `ConnectionKind` variant differs from the existing entry's. Kind
    /// changes are intentionally not supported on edit (ADR-0016): they
    /// would require migrating keyring references mid-flight. Callers
    /// must delete + re-add to switch adapter kind.
    #[error("connection {id} kind cannot change on update")]
    KindMismatch { id: String },

    /// A `[connections.ssh]` block was attached to a connection kind that
    /// cannot be tunneled — a local file (Turso), an HTTPS API (D1), or the
    /// self-signing Aurora DSQL IAM kind. SSH tunnels apply only to the
    /// URL-bearing TCP engines (ADR-0069).
    #[error("connection {id}: an ssh tunnel is not supported for a {kind} connection")]
    SshUnsupportedKind {
        /// The offending connection id.
        id: String,
        /// The adapter label of the kind that cannot be tunneled.
        kind: &'static str,
    },

    /// A `[connections.ssh]` block was malformed: it must name exactly one
    /// authentication method (`key_path` or `keyring_password_ref`) and
    /// exactly one host-key policy (`fingerprint` or `known_hosts`) (ADR-0069).
    #[error("connection {id}: invalid ssh tunnel: {reason}")]
    SshInvalid {
        /// The offending connection id.
        id: String,
        /// Human-readable reason the ssh block is invalid.
        reason: String,
    },

    /// The DSN stored in the keychain for this connection could not be
    /// parsed, so the password the user asked to keep cannot be recovered
    /// (ADR-0080). Raised only on the save path: saving the edit anyway would
    /// drop the credential from a working connection and break it silently.
    #[error("connection {id}: the stored connection url could not be parsed")]
    DsnUnparseable {
        /// The offending connection id.
        id: String,
    },

    /// Encrypting or decrypting a connection bundle failed (ADR-0038).
    /// Wraps the crypto-layer [`BundleError`] so the connection-admin
    /// export/import methods surface a single error type. Distinct from
    /// [`ConfigError::Secret`], which is a keychain fault while resolving
    /// the plaintext the bundle carries.
    #[error("config bundle failed: {0}")]
    Bundle(#[from] BundleError),

    /// A selective export named no connections (ADR-0105). Refused rather
    /// than honoured: an empty bundle encrypts and decrypts perfectly well
    /// and then imports nothing, which the user reads as a wrong
    /// passphrase. Failing at export time says what actually happened.
    #[error("select at least one connection to export")]
    EmptySelection,
}
