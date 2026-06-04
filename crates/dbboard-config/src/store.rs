//! On-disk shape of the connection store.
//!
//! [`ConnectionFile::parse`] is the schema-layer validator;
//! [`default_path`], [`load_or_empty`], and [`save_atomic`] are the
//! filesystem layer on top of it.
//!
//! Secrets are *referenced* here (`keyring_*_ref`) but never *stored*
//! here; the actual token / URL is round-tripped through an OS keychain.
//! The TOML file is therefore safe to back up, sync between machines, or
//! paste into a bug report.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::ConfigError;

/// The single TOML schema version this build understands.
///
/// We refuse to guess at unknown versions: future schema evolutions
/// will bump this constant and add an explicit in-place migration.
pub const CONFIG_VERSION: u32 = 1;

/// Top-level shape of `connections.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionFile {
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<ConnectionEntry>,
}

/// A single `[[connections]]` entry. `id` is the stable primary key
/// referenced by `DBBOARD_CONNECTION` and the future connection picker;
/// `name` is the human label shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionEntry {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub kind: ConnectionKind,
}

/// Adapter-specific fields. `serde(tag = "kind")` puts the discriminator
/// inline with the entry so the TOML stays flat:
///
/// ```toml
/// [[connections]]
/// id   = "local-turso"
/// name = "Local libSQL"
/// kind = "turso"
/// path = ":memory:"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionKind {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
        keyring_token_ref: String,
    },
    Postgres {
        keyring_url_ref: String,
    },
    /// A Neon connection (ADR-0018). Shape is byte-identical to
    /// [`ConnectionKind::Postgres`]; the discriminator is the only
    /// distinction so the connection picker and capability output can
    /// label the connection as Neon rather than generic Postgres.
    Neon {
        keyring_url_ref: String,
    },
}

impl ConnectionFile {
    /// Parse and validate a `connections.toml` payload.
    ///
    /// Validates the schema version and that ids are unique. Unknown
    /// `kind` values, unknown versions, and duplicate ids are surfaced
    /// as hard errors — silent drops would hide real drift between the
    /// app and a hand-edited file.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Parse`] if the TOML is malformed or contains an
    ///   unknown `kind`.
    /// - [`ConfigError::UnsupportedVersion`] if `version` is not
    ///   [`CONFIG_VERSION`].
    /// - [`ConfigError::DuplicateId`] if two entries share the same `id`.
    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        let file: ConnectionFile = toml::from_str(input)?;
        if file.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(file.version));
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(file.connections.len());
        for entry in &file.connections {
            if !seen.insert(entry.id.as_str()) {
                return Err(ConfigError::DuplicateId(entry.id.clone()));
            }
        }
        Ok(file)
    }

    /// Convenience constructor for an empty store at the current
    /// schema version. Used by [`load_or_empty`] and by tests.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            version: CONFIG_VERSION,
            connections: Vec::new(),
        }
    }
}

/// The default per-user path for `connections.toml`, resolved via the
/// `directories` crate so it matches each platform's convention:
///
/// - Windows: `%APPDATA%\dbboard\dbboard\config\connections.toml`
/// - macOS:   `~/Library/Application Support/dev.dbboard.dbboard/connections.toml`
/// - Linux:   `$XDG_CONFIG_HOME/dbboard/connections.toml`
///   (default `~/.config/dbboard/connections.toml`)
///
/// # Errors
///
/// Returns [`ConfigError::NoConfigDir`] when the OS reports no usable
/// per-user config directory (no `$HOME`, no `%APPDATA%`).
pub fn default_path() -> Result<PathBuf, ConfigError> {
    let dirs = ProjectDirs::from("dev", "dbboard", "dbboard").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().join("connections.toml"))
}

/// The default per-user path for `history.jsonl` (ADR-0017), resolved
/// via the same `directories` lookup as [`default_path`] so the two
/// live side by side under one config dir:
///
/// - Windows: `%APPDATA%\dbboard\dbboard\config\history.jsonl`
/// - macOS:   `~/Library/Application Support/dev.dbboard.dbboard/history.jsonl`
/// - Linux:   `$XDG_CONFIG_HOME/dbboard/history.jsonl`
///   (default `~/.config/dbboard/history.jsonl`)
///
/// # Errors
///
/// Returns [`ConfigError::NoConfigDir`] when the OS reports no usable
/// per-user config directory (no `$HOME`, no `%APPDATA%`).
pub fn default_history_path() -> Result<PathBuf, ConfigError> {
    let dirs = ProjectDirs::from("dev", "dbboard", "dbboard").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().join("history.jsonl"))
}

/// Read and parse `connections.toml` at `path`.
///
/// A missing file is **not** an error: it yields an empty store at the
/// current schema version. The file is created lazily by
/// [`save_atomic`] when the user adds the first entry. Any other I/O
/// error is propagated.
///
/// # Errors
///
/// - [`ConfigError::Io`] for non-`NotFound` I/O failures.
/// - [`ConfigError::Parse`], [`ConfigError::UnsupportedVersion`], or
///   [`ConfigError::DuplicateId`] from the underlying
///   [`ConnectionFile::parse`].
pub fn load_or_empty(path: &Path) -> Result<ConnectionFile, ConfigError> {
    match fs::read_to_string(path) {
        Ok(contents) => ConnectionFile::parse(&contents),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(ConnectionFile::empty()),
        Err(err) => Err(ConfigError::Io(err)),
    }
}

/// Write `file` to `path` atomically: serialize to a sibling `*.tmp`
/// file (created with mode `0o600` on Unix) and then `rename` it into
/// place. Parent directories are created if necessary.
///
/// On Windows `fs::rename` maps to `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`,
/// which is the closest practical equivalent — atomic with respect to
/// concurrent readers on the same volume.
///
/// # Errors
///
/// - [`ConfigError::Serialize`] if re-serializing the in-memory store
///   to TOML fails.
/// - [`ConfigError::Io`] for any filesystem failure (creating parent
///   dirs, opening the temp file, writing, syncing, renaming).
pub fn save_atomic(path: &Path, file: &ConnectionFile) -> Result<(), ConfigError> {
    let serialized = toml::to_string(file)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path_for(path);
    write_new_file(&tmp, serialized.as_bytes())?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::Io(err));
    }
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".connections.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

#[cfg(unix)]
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    // `create_new(true)` rejects a stale temp left behind by an
    // interrupted save — better to fail loudly than to clobber.
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

#[cfg(not(unix))]
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_constructor_uses_the_current_schema_version() {
        let file = ConnectionFile::empty();
        assert_eq!(file.version, CONFIG_VERSION);
        assert!(file.connections.is_empty());
    }

    #[test]
    fn version_only_file_parses_with_no_connections() {
        let toml_src = "version = 1\n";
        let file = ConnectionFile::parse(toml_src).expect("version-only file parses");
        assert_eq!(file.version, 1);
        assert!(file.connections.is_empty());
    }

    #[test]
    fn parses_a_minimal_turso_entry() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "local-turso"
name = "Local libSQL"
kind = "turso"
path = ":memory:"
"#;
        let file = ConnectionFile::parse(toml_src).expect("turso entry parses");
        assert_eq!(file.connections.len(), 1);
        let entry = &file.connections[0];
        assert_eq!(entry.id, "local-turso");
        assert_eq!(entry.name, "Local libSQL");
        assert_eq!(
            entry.kind,
            ConnectionKind::Turso {
                path: ":memory:".to_string()
            }
        );
    }

    #[test]
    fn parses_a_d1_entry_with_optional_base_url_present() {
        let toml_src = r#"
version = 1

[[connections]]
id                = "prod-d1"
name              = "Prod D1"
kind              = "d1"
account_id        = "acct-123"
database_id       = "db-456"
base_url          = "https://api.cloudflare.com/client/v4"
keyring_token_ref = "dbboard.prod-d1.token"
"#;
        let file = ConnectionFile::parse(toml_src).expect("d1 entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::D1 {
                account_id: "acct-123".to_string(),
                database_id: "db-456".to_string(),
                base_url: Some("https://api.cloudflare.com/client/v4".to_string()),
                keyring_token_ref: "dbboard.prod-d1.token".to_string(),
            }
        );
    }

    #[test]
    fn parses_a_d1_entry_with_optional_base_url_absent() {
        let toml_src = r#"
version = 1

[[connections]]
id                = "prod-d1"
name              = "Prod D1"
kind              = "d1"
account_id        = "acct-123"
database_id       = "db-456"
keyring_token_ref = "dbboard.prod-d1.token"
"#;
        let file = ConnectionFile::parse(toml_src).expect("d1 without base_url parses");
        match &file.connections[0].kind {
            ConnectionKind::D1 { base_url, .. } => assert!(base_url.is_none()),
            other => panic!("expected D1, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_neon_entry() {
        let toml_src = r#"
version = 1

[[connections]]
id              = "neon-prod"
name            = "Neon (prod)"
kind            = "neon"
keyring_url_ref = "dbboard.neon-prod.url"
"#;
        let file = ConnectionFile::parse(toml_src).expect("neon entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::Neon {
                keyring_url_ref: "dbboard.neon-prod.url".to_string(),
            }
        );
    }

    #[test]
    fn parses_a_postgres_entry() {
        let toml_src = r#"
version = 1

[[connections]]
id              = "neon-staging"
name            = "Neon Staging"
kind            = "postgres"
keyring_url_ref = "dbboard.neon-staging.url"
"#;
        let file = ConnectionFile::parse(toml_src).expect("postgres entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::Postgres {
                keyring_url_ref: "dbboard.neon-staging.url".to_string(),
            }
        );
    }

    #[test]
    fn unknown_kind_is_a_parse_error() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "bogus"
name = "Bogus"
kind = "mysql"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("unknown kind must fail");
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn duplicate_id_is_rejected_loudly() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "dup"
name = "First"
kind = "turso"
path = ":memory:"

[[connections]]
id   = "dup"
name = "Second"
kind = "turso"
path = "/tmp/x.db"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("duplicate id must fail");
        match err {
            ConfigError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let toml_src = r#"
version = 2

[[connections]]
id   = "x"
name = "X"
kind = "turso"
path = ":memory:"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("v2 must be rejected");
        match err {
            ConfigError::UnsupportedVersion(v) => assert_eq!(v, 2),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn missing_version_field_is_a_parse_error() {
        let toml_src = r#"
[[connections]]
id   = "x"
name = "X"
kind = "turso"
path = ":memory:"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("missing version must fail");
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn serialize_then_parse_is_identity_for_every_kind() {
        let original = ConnectionFile {
            version: CONFIG_VERSION,
            connections: vec![
                ConnectionEntry {
                    id: "local-turso".to_string(),
                    name: "Local libSQL".to_string(),
                    kind: ConnectionKind::Turso {
                        path: ":memory:".to_string(),
                    },
                },
                ConnectionEntry {
                    id: "prod-d1".to_string(),
                    name: "Prod D1".to_string(),
                    kind: ConnectionKind::D1 {
                        account_id: "acct".to_string(),
                        database_id: "db".to_string(),
                        base_url: Some("https://example.test".to_string()),
                        keyring_token_ref: "dbboard.prod-d1.token".to_string(),
                    },
                },
                ConnectionEntry {
                    id: "neon".to_string(),
                    name: "Neon".to_string(),
                    kind: ConnectionKind::Postgres {
                        keyring_url_ref: "dbboard.neon.url".to_string(),
                    },
                },
                ConnectionEntry {
                    id: "neon-managed".to_string(),
                    name: "Neon (managed)".to_string(),
                    kind: ConnectionKind::Neon {
                        keyring_url_ref: "dbboard.neon-managed.url".to_string(),
                    },
                },
            ],
        };
        let serialized = toml::to_string(&original).expect("serialize");
        let reparsed = ConnectionFile::parse(&serialized).expect("re-parse");
        assert_eq!(original, reparsed);
    }

    /// A grep-level guard: even when the caller injects values that
    /// *look* like secrets into the non-secret fields, the schema never
    /// surfaces them under a key named `token`, `password`, or `secret`
    /// in the serialized TOML. The only secret-adjacent keys are
    /// `keyring_token_ref` / `keyring_url_ref`, which by design carry
    /// keychain *references*, not material.
    #[test]
    fn serialized_toml_has_no_secret_value_keys() {
        let file = ConnectionFile {
            version: CONFIG_VERSION,
            connections: vec![ConnectionEntry {
                id: "prod-d1".to_string(),
                name: "Prod D1".to_string(),
                kind: ConnectionKind::D1 {
                    account_id: "acct".to_string(),
                    database_id: "db".to_string(),
                    base_url: None,
                    keyring_token_ref: "dbboard.prod-d1.token".to_string(),
                },
            }],
        };
        let serialized = toml::to_string(&file).expect("serialize");
        for forbidden_key in ["token =", "password =", "secret ="] {
            assert!(
                !serialized.contains(forbidden_key),
                "serialized TOML must not expose a `{forbidden_key}` field: {serialized}"
            );
        }
        // `keyring_token_ref =` is fine (and required), so the assertion
        // above must use the exact-key form ("token =" not "token").
        assert!(serialized.contains("keyring_token_ref ="));
    }

    #[test]
    fn omitted_base_url_is_not_emitted_during_serialization() {
        let file = ConnectionFile {
            version: CONFIG_VERSION,
            connections: vec![ConnectionEntry {
                id: "d1".to_string(),
                name: "D1".to_string(),
                kind: ConnectionKind::D1 {
                    account_id: "a".to_string(),
                    database_id: "b".to_string(),
                    base_url: None,
                    keyring_token_ref: "dbboard.d1.token".to_string(),
                },
            }],
        };
        let serialized = toml::to_string(&file).expect("serialize");
        assert!(
            !serialized.contains("base_url"),
            "absent base_url must not be emitted: {serialized}"
        );
    }
}
