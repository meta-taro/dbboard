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

use crate::secure_fs;
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
    /// Whether `dbboard-mcp` may run write statements against this connection
    /// (ADR-0087). Off unless the operator says otherwise: the same file backs
    /// the desktop app and can name a production database, so an agent must
    /// not inherit DDL rights merely by being pointed at a config.
    ///
    /// It gates the write tools only. The read tools ignore it, and connection
    /// CRUD is closed regardless — see baseline §15.
    ///
    /// Skipped when false so enabling this on one connection does not rewrite
    /// every other entry in an existing file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mcp_write: bool,
    /// The name `dbboard-mcp` shows an AI agent instead of this entry's `id`
    /// **and** `name` (ADR-0088).
    ///
    /// Ids are typed by hand, and the obvious thing to type is what the
    /// connect dialog already shows — `app@db.internal`. That id is on the
    /// first tool result an agent produces and travels wherever its transcript
    /// does. The display name is no better: a store's real name identifies a
    /// business as precisely as its hostname identifies a server.
    ///
    /// `None` means the id and name are used as they are, so existing configs
    /// behave exactly as before — ids are referenced by every other tool call
    /// and by `annotations.toml`, so this is opt-in rather than a rename.
    ///
    /// Must be unique across every entry's alias *and* id
    /// ([`ConfigError::DuplicateAlias`](crate::ConfigError::DuplicateAlias)): an
    /// agent hands it back as a handle, and a handle that matches two
    /// connections would route a query to the wrong database.
    ///
    /// Same TOML ordering constraint as `mcp_write` — a scalar, so it must be
    /// emitted before the `ssh` table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_alias: Option<String>,
    /// Optional SSH local-forward tunnel (ADR-0069). Cross-cutting: it applies
    /// uniformly to the URL-bearing TCP engines and to none of the others, so
    /// it lives here on the entry rather than being copied onto each
    /// [`ConnectionKind`] variant. Serialized as a trailing `[connections.ssh]`
    /// sub-table — it MUST stay the last field so the flattened `kind` scalars
    /// are emitted before this table (TOML requires values before tables).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnelToml>,
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
    /// A `MySQL` connection (ADR-0068). Like [`ConnectionKind::Postgres`] it
    /// stores only a keychain reference to a `mysql://…` URL; the difference is
    /// a genuinely different SQL dialect (back-tick quoting, backslash-escaped
    /// literals) behind the `dbboard-mysql` adapter. The variant is spelled
    /// `MySql` in Rust, but `rename_all = "snake_case"` would emit `my_sql`, so
    /// the TOML discriminator is pinned to `kind = "mysql"` to match
    /// `dbboard_mysql::FLAVOR_MYSQL`.
    #[serde(rename = "mysql")]
    MySql {
        keyring_url_ref: String,
    },
    /// A Neon connection (ADR-0018). Shape is byte-identical to
    /// [`ConnectionKind::Postgres`]; the discriminator is the only
    /// distinction so the connection picker and capability output can
    /// label the connection as Neon rather than generic Postgres.
    Neon {
        keyring_url_ref: String,
    },
    /// A Supabase connection (ADR-0019). Shape is byte-identical to
    /// [`ConnectionKind::Postgres`]; the discriminator is the only
    /// distinction so the connection picker and capability output can
    /// label the connection as Supabase rather than generic Postgres.
    /// REST surfaces (auth / storage / realtime / functions) are not
    /// part of this kind; ADR-0019 §Decision defers them to a future
    /// ADR with the matching capability flag extension.
    Supabase {
        keyring_url_ref: String,
    },
    /// An AWS Aurora DSQL connection (ADR-0021). Shape is byte-identical
    /// to [`ConnectionKind::Postgres`]; the discriminator is the only
    /// distinction so the connection picker, capability output, and
    /// `id()` surface label the connection as Aurora DSQL rather than
    /// generic Postgres. The URL stored under `keyring_url_ref` is
    /// expected to embed a short-lived IAM authentication token (~15 min
    /// TTL) in its password field; automatic refresh via the AWS SDK is
    /// out of scope for v=1 and will land via a future ADR. The TOML
    /// discriminator is `kind = "aurora-dsql"` (kebab-case), matching
    /// `dbboard_postgres::FLAVOR_AURORA_DSQL`.
    #[serde(rename = "aurora-dsql")]
    AuroraDsql {
        keyring_url_ref: String,
    },
    /// An AWS Aurora DSQL connection that mints its own IAM auth token at
    /// connect time (ADR-0036). Unlike [`ConnectionKind::AuroraDsql`],
    /// which stores a pre-generated (and quickly-expiring) token URL under
    /// `keyring_url_ref`, this kind stores only long-lived AWS credentials
    /// and derives a fresh `SigV4` token on every connect — the path the
    /// 24/7 team rollout needs. `endpoint`, `region`, `database`,
    /// `username`, and `access_key_id` are non-secret and live inline;
    /// only the AWS secret access key is a secret, referenced through
    /// `keyring_secret_key_ref`. The TOML discriminator is
    /// `kind = "aurora-dsql-iam"` (kebab-case). Automatic in-pool token
    /// refresh (段階B) is a follow-up ADR; v1 mints at connect time only.
    #[serde(rename = "aurora-dsql-iam")]
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        keyring_secret_key_ref: String,
    },
    /// A Cloud Firestore connection (ADR-0091, ADR-0093). `project_id` is
    /// mandatory; `database_id` defaults to `(default)` and `base_url` to the
    /// production REST host when absent.
    ///
    /// `keyring_service_account_ref` is the only optional keychain reference in
    /// this enum, and deliberately so: a connection pointed at the local
    /// Firestore emulator has **no credential at all** — the emulator accepts a
    /// fixed `Bearer owner` — so a mandatory reference would make an emulator
    /// connection unrepresentable, or force an empty-string secret that reads
    /// as a real one. `None` therefore means "emulator", not "not filled in
    /// yet".
    Firestore {
        project_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        database_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keyring_service_account_ref: Option<String>,
    },
    /// A `MongoDB` connection (ADR-0096). Like [`ConnectionKind::Postgres`] it
    /// stores only a keychain reference — here to a `mongodb://…` or
    /// `mongodb+srv://…` URI, which carries the password in its authority.
    ///
    /// `database` is optional because the URI may name it in the path
    /// (`mongodb://host/orders`). When neither names one the adapter refuses
    /// rather than guessing; that check belongs to the adapter, not the store,
    /// so a file written by an older build still parses.
    ///
    /// The variant is spelled `MongoDb` in Rust, but `rename_all = "snake_case"`
    /// would emit `mongo_db`, so the TOML discriminator is pinned to
    /// `kind = "mongodb"` — the same reason [`ConnectionKind::MySql`] pins
    /// `mysql`.
    #[serde(rename = "mongodb")]
    MongoDb {
        keyring_url_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        database: Option<String>,
    },
}

impl ConnectionKind {
    /// Short human label for the adapter behind this kind — the brand name
    /// the connection picker and the header pill show (e.g. `Turso`,
    /// `Neon`, `Aurora DSQL`). These are proper nouns, identical across
    /// locales, so they are not routed through the i18n catalogue. Lives on
    /// the enum (not the UI) so every surface that labels a connection reads
    /// one definition.
    #[must_use]
    pub fn adapter_label(&self) -> &'static str {
        match self {
            ConnectionKind::Turso { .. } => "Turso",
            ConnectionKind::D1 { .. } => "Cloudflare D1",
            ConnectionKind::Postgres { .. } => "Postgres",
            ConnectionKind::MySql { .. } => "MySQL",
            ConnectionKind::Neon { .. } => "Neon",
            ConnectionKind::Supabase { .. } => "Supabase",
            ConnectionKind::AuroraDsql { .. } => "Aurora DSQL",
            ConnectionKind::AuroraDsqlIam { .. } => "Aurora DSQL (IAM)",
            ConnectionKind::Firestore { .. } => "Firestore",
            ConnectionKind::MongoDb { .. } => "MongoDB",
        }
    }

    /// Whether an SSH tunnel (ADR-0069) can front this kind. Tunnels apply
    /// only to the URL-bearing TCP engines — the Postgres-wire family and
    /// `MySQL` — whose connection is a plain `host:port` we can redirect to a
    /// loopback forward. Turso is a local file, D1 and Firestore are HTTPS
    /// APIs, and the Aurora DSQL IAM kind mints its own endpoint at connect; none
    /// of them route through a forwarded TCP port, so a tunnel on them is a
    /// configuration error rather than silently ignored.
    ///
    /// `MongoDB` is excluded for a different reason: it *is* TCP, but a
    /// `mongodb://` URI may list several hosts and `mongodb+srv://` discovers a
    /// whole replica set out of DNS. Rewriting one host to a loopback forward
    /// would leave the driver failing over to the untunnelled members, so it
    /// would appear to work and then silently stop. Refusing is the honest
    /// answer until a tunnel can front every member.
    #[must_use]
    pub fn supports_ssh_tunnel(&self) -> bool {
        matches!(
            self,
            ConnectionKind::Postgres { .. }
                | ConnectionKind::MySql { .. }
                | ConnectionKind::Neon { .. }
                | ConnectionKind::Supabase { .. }
                | ConnectionKind::AuroraDsql { .. }
        )
    }
}

/// SSH local-forward tunnel settings for a connection (ADR-0069).
///
/// The far-side database address (host/port the bastion connects to on our
/// behalf) is **not** stored here — it is taken from the connection's own URL
/// at connect time, exactly like a GUI client forwards to the DB host from the
/// main session settings. This block carries only the bastion coordinates,
/// how to authenticate to it, and how to verify its host key.
///
/// Secrets stay out of the file: the key **passphrase** and the SSH
/// **password** are keychain references (`keyring_*_ref`), never inline. The
/// key *path*, host/port/user, and the host-key *fingerprint* are non-secret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshTunnelToml {
    /// Bastion hostname or IP.
    pub host: String,
    /// Bastion SSH port. Defaults to 22.
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// SSH username on the bastion.
    pub user: String,

    /// Path to the private key for public-key auth. Mutually exclusive with
    /// [`keyring_password_ref`](Self::keyring_password_ref).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    /// Keychain reference to the passphrase decrypting `key_path`, if the key
    /// is encrypted. Only meaningful alongside `key_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring_passphrase_ref: Option<String>,
    /// Keychain reference to the SSH password for password auth. Mutually
    /// exclusive with `key_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring_password_ref: Option<String>,

    /// Pinned server host-key fingerprint (`SHA256:...`, prefix optional).
    /// Mutually exclusive with [`known_hosts`](Self::known_hosts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// Path to an OpenSSH `known_hosts` file to verify the server key against.
    /// Mutually exclusive with `fingerprint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_hosts: Option<String>,
}

/// `skip_serializing_if` predicate for boolean fields whose `false` is the
/// default and should stay off disk.
#[allow(clippy::trivially_copy_pass_by_ref)] // serde requires the reference
fn is_false(value: &bool) -> bool {
    !*value
}

fn default_ssh_port() -> u16 {
    22
}

impl SshTunnelToml {
    /// Validate that the block names exactly one authentication method and
    /// exactly one host-key policy. Returns a human-readable reason on
    /// failure, which [`ConnectionFile::parse`] wraps in
    /// [`ConfigError::SshInvalid`].
    ///
    /// # Errors
    /// Returns `Err(reason)` when auth or host-key policy is ambiguous or
    /// missing.
    pub fn validate(&self) -> Result<(), String> {
        match (self.key_path.is_some(), self.keyring_password_ref.is_some()) {
            (true, true) => {
                return Err("specify either key_path or keyring_password_ref, not both".into())
            }
            (false, false) => {
                return Err("needs an auth method: key_path or keyring_password_ref".into())
            }
            _ => {}
        }
        if self.keyring_passphrase_ref.is_some() && self.key_path.is_none() {
            return Err("keyring_passphrase_ref is only valid together with key_path".into());
        }
        match (self.fingerprint.is_some(), self.known_hosts.is_some()) {
            (true, true) => Err("specify either fingerprint or known_hosts, not both".into()),
            (false, false) => Err("needs a host-key policy: fingerprint or known_hosts".into()),
            _ => Ok(()),
        }
    }

    /// The keychain references this block owns, for the admin layer to write
    /// on add and purge on delete. Empty when the tunnel uses key-file auth
    /// with an unencrypted key.
    #[must_use]
    pub fn keyring_refs(&self) -> Vec<&str> {
        [
            self.keyring_passphrase_ref.as_deref(),
            self.keyring_password_ref.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
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
            if let Some(ssh) = &entry.ssh {
                if !entry.kind.supports_ssh_tunnel() {
                    return Err(ConfigError::SshUnsupportedKind {
                        id: entry.id.clone(),
                        kind: entry.kind.adapter_label(),
                    });
                }
                ssh.validate().map_err(|reason| ConfigError::SshInvalid {
                    id: entry.id.clone(),
                    reason,
                })?;
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

// `create_new_user_only` rejects a stale temp left behind by an
// interrupted save — better to fail loudly than to clobber. On Unix
// the file lands as `0o600`; on Windows it inherits the user-only DACL
// of `%APPDATA%\Roaming\<user>\` (ADR-0024).
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = secure_fs::create_new_user_only(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_label_names_each_kind() {
        assert_eq!(
            ConnectionKind::Turso {
                path: ":memory:".into()
            }
            .adapter_label(),
            "Turso"
        );
        assert_eq!(
            ConnectionKind::Neon {
                keyring_url_ref: "r".into()
            }
            .adapter_label(),
            "Neon"
        );
        // Postgres and Neon share a shape but not a label — the discriminator
        // is the whole point of the separate kind.
        assert_ne!(
            ConnectionKind::Postgres {
                keyring_url_ref: "r".into()
            }
            .adapter_label(),
            ConnectionKind::Neon {
                keyring_url_ref: "r".into()
            }
            .adapter_label()
        );
    }

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
    fn parses_a_supabase_entry() {
        let toml_src = r#"
version = 1

[[connections]]
id              = "supabase-prod"
name            = "Supabase (prod)"
kind            = "supabase"
keyring_url_ref = "dbboard.supabase-prod.url"
"#;
        let file = ConnectionFile::parse(toml_src).expect("supabase entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::Supabase {
                keyring_url_ref: "dbboard.supabase-prod.url".to_string(),
            }
        );
    }

    #[test]
    fn parses_an_aurora_dsql_entry() {
        // The TOML discriminator is the kebab-case literal "aurora-dsql"
        // (matches `dbboard_postgres::FLAVOR_AURORA_DSQL`); a snake_case
        // "aurora_dsql" would *not* parse.
        let toml_src = r#"
version = 1

[[connections]]
id              = "dsql-prod"
name            = "Aurora DSQL (prod)"
kind            = "aurora-dsql"
keyring_url_ref = "dbboard.dsql-prod.url"
"#;
        let file = ConnectionFile::parse(toml_src).expect("aurora-dsql entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::AuroraDsql {
                keyring_url_ref: "dbboard.dsql-prod.url".to_string(),
            }
        );
    }

    #[test]
    fn parses_an_aurora_dsql_iam_entry() {
        // The discriminator is the kebab-case literal "aurora-dsql-iam".
        // All fields except the secret-key reference live inline; the
        // AWS secret access key itself is never in the file.
        let toml_src = r#"
version = 1

[[connections]]
id                     = "store-b"
name                   = "store-b"
kind                   = "aurora-dsql-iam"
endpoint               = "abc123.dsql.ap-northeast-1.on.aws"
region                 = "ap-northeast-1"
database               = "postgres"
username               = "admin"
access_key_id          = "AKIAEXAMPLE"
keyring_secret_key_ref = "dbboard.store-b.secret_key"
"#;
        let file = ConnectionFile::parse(toml_src).expect("aurora-dsql-iam entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::AuroraDsqlIam {
                endpoint: "abc123.dsql.ap-northeast-1.on.aws".to_string(),
                region: "ap-northeast-1".to_string(),
                database: "postgres".to_string(),
                username: "admin".to_string(),
                access_key_id: "AKIAEXAMPLE".to_string(),
                keyring_secret_key_ref: "dbboard.store-b.secret_key".to_string(),
            }
        );
    }

    #[test]
    fn parses_a_firestore_entry_with_a_service_account() {
        let toml_src = r#"
version = 1

[[connections]]
id                          = "fs-prod"
name                        = "Firestore (prod)"
kind                        = "firestore"
project_id                  = "example-project"
keyring_service_account_ref = "dbboard.fs-prod.service_account"
"#;
        let file = ConnectionFile::parse(toml_src).expect("firestore entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::Firestore {
                project_id: "example-project".to_string(),
                database_id: None,
                base_url: None,
                keyring_service_account_ref: Some("dbboard.fs-prod.service_account".to_string()),
            }
        );
    }

    /// The emulator has no credential at all — not an empty one. Every other
    /// secret-bearing kind makes its keychain reference mandatory; Firestore
    /// cannot, or a local emulator connection would be unrepresentable.
    #[test]
    fn parses_a_firestore_emulator_entry_with_no_credential_reference() {
        let toml_src = r#"
version = 1

[[connections]]
id          = "fs-local"
name        = "Firestore (emulator)"
kind        = "firestore"
project_id  = "demo-project"
database_id = "(default)"
base_url    = "http://127.0.0.1:8080"
"#;
        let file = ConnectionFile::parse(toml_src).expect("firestore emulator entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::Firestore {
                project_id: "demo-project".to_string(),
                database_id: Some("(default)".to_string()),
                base_url: Some("http://127.0.0.1:8080".to_string()),
                keyring_service_account_ref: None,
            }
        );
    }

    /// Firestore is an HTTPS REST API, so there is no `host:port` to forward.
    /// Like D1, a tunnel on it is a configuration error rather than a
    /// silently-ignored field.
    #[test]
    fn firestore_refuses_an_ssh_tunnel() {
        assert!(!ConnectionKind::Firestore {
            project_id: "p".into(),
            database_id: None,
            base_url: None,
            keyring_service_account_ref: None,
        }
        .supports_ssh_tunnel());
        assert_eq!(
            ConnectionKind::Firestore {
                project_id: "p".into(),
                database_id: None,
                base_url: None,
                keyring_service_account_ref: None,
            }
            .adapter_label(),
            "Firestore"
        );
    }

    #[test]
    fn parses_a_mongodb_entry() {
        let toml_src = r#"
version = 1

[[connections]]
id              = "mongo-local"
name            = "Mongo (local)"
kind            = "mongodb"
keyring_url_ref = "dbboard.mongo-local.url"
database        = "dbboard_test"
"#;
        let file = ConnectionFile::parse(toml_src).expect("mongodb entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::MongoDb {
                keyring_url_ref: "dbboard.mongo-local.url".to_string(),
                database: Some("dbboard_test".to_string()),
            }
        );
    }

    /// The URI can name the database in its path (`mongodb://host/dbname`), so
    /// the explicit field is optional — but only one of the two may be absent.
    /// The adapter refuses a connection that names neither rather than picking
    /// one, and that refusal belongs there, not here.
    #[test]
    fn parses_a_mongodb_entry_that_leaves_the_database_to_the_uri() {
        let toml_src = r#"
version = 1

[[connections]]
id              = "mongo-atlas"
name            = "Mongo (atlas)"
kind            = "mongodb"
keyring_url_ref = "dbboard.mongo-atlas.url"
"#;
        let file = ConnectionFile::parse(toml_src).expect("mongodb entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::MongoDb {
                keyring_url_ref: "dbboard.mongo-atlas.url".to_string(),
                database: None,
            }
        );
    }

    /// `MongoDB` *is* a TCP protocol on `host:port`, so unlike D1 and Firestore
    /// the refusal is not "there is nothing to forward". It is that a
    /// `mongodb://` URI may name several hosts and `mongodb+srv://` resolves a
    /// whole replica set out of DNS; rewriting one host to a loopback forward
    /// would leave the driver talking to a different node than the one the
    /// tunnel fronts, and failing over to the untunnelled ones. Better refused
    /// than quietly wrong (ADR-0069).
    #[test]
    fn mongodb_refuses_an_ssh_tunnel() {
        let kind = ConnectionKind::MongoDb {
            keyring_url_ref: "r".into(),
            database: None,
        };
        assert!(!kind.supports_ssh_tunnel());
        assert_eq!(kind.adapter_label(), "MongoDB");
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
    fn parses_a_mysql_entry() {
        let toml_src = r#"
version = 1

[[connections]]
id              = "shop-mysql"
name            = "Shop MySQL"
kind            = "mysql"
keyring_url_ref = "dbboard.shop-mysql.url"
"#;
        let file = ConnectionFile::parse(toml_src).expect("mysql entry parses");
        assert_eq!(
            file.connections[0].kind,
            ConnectionKind::MySql {
                keyring_url_ref: "dbboard.shop-mysql.url".to_string(),
            }
        );
    }

    #[test]
    fn unknown_kind_is_a_parse_error() {
        // `oracle` is a genuinely unsupported engine — using a real-but-absent
        // adapter name keeps this a true "unknown kind" test even as new kinds
        // (mysql, …) graduate into the enum.
        let toml_src = r#"
version = 1

[[connections]]
id   = "bogus"
name = "Bogus"
kind = "oracle"
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
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "local-turso".to_string(),
                    name: "Local libSQL".to_string(),
                    kind: ConnectionKind::Turso {
                        path: ":memory:".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
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
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "neon".to_string(),
                    name: "Neon".to_string(),
                    kind: ConnectionKind::Postgres {
                        keyring_url_ref: "dbboard.neon.url".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "shop-mysql".to_string(),
                    name: "Shop MySQL".to_string(),
                    kind: ConnectionKind::MySql {
                        keyring_url_ref: "dbboard.shop-mysql.url".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "neon-managed".to_string(),
                    name: "Neon (managed)".to_string(),
                    kind: ConnectionKind::Neon {
                        keyring_url_ref: "dbboard.neon-managed.url".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "supabase-prod".to_string(),
                    name: "Supabase (prod)".to_string(),
                    kind: ConnectionKind::Supabase {
                        keyring_url_ref: "dbboard.supabase-prod.url".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "dsql-prod".to_string(),
                    name: "Aurora DSQL (prod)".to_string(),
                    kind: ConnectionKind::AuroraDsql {
                        keyring_url_ref: "dbboard.dsql-prod.url".to_string(),
                    },
                },
                ConnectionEntry {
                    mcp_alias: None,
                    mcp_write: false,
                    ssh: None,
                    id: "dsql-iam".to_string(),
                    name: "Aurora DSQL (IAM)".to_string(),
                    kind: ConnectionKind::AuroraDsqlIam {
                        endpoint: "abc123.dsql.ap-northeast-1.on.aws".to_string(),
                        region: "ap-northeast-1".to_string(),
                        database: "postgres".to_string(),
                        username: "admin".to_string(),
                        access_key_id: "AKIAEXAMPLE".to_string(),
                        keyring_secret_key_ref: "dbboard.dsql-iam.secret_key".to_string(),
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
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
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
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
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

    fn pg_ssh_toml() -> &'static str {
        r#"
version = 1

[[connections]]
id   = "work-mysql"
name = "Work MySQL"
kind = "mysql"
keyring_url_ref = "dbboard.work-mysql.url"

[connections.ssh]
host = "bastion.example"
port = 2222
user = "deploy"
key_path = "/home/user/.ssh/id_ed25519"
keyring_passphrase_ref = "dbboard.work-mysql.ssh_passphrase"
fingerprint = "SHA256:abc123def456"
"#
    }

    #[test]
    fn parses_a_connection_with_an_ssh_tunnel_block() {
        let file = ConnectionFile::parse(pg_ssh_toml()).expect("ssh entry parses");
        let ssh = file.connections[0].ssh.as_ref().expect("ssh block present");
        assert_eq!(ssh.host, "bastion.example");
        assert_eq!(ssh.port, 2222);
        assert_eq!(ssh.user, "deploy");
        assert_eq!(ssh.key_path.as_deref(), Some("/home/user/.ssh/id_ed25519"));
        assert_eq!(
            ssh.keyring_passphrase_ref.as_deref(),
            Some("dbboard.work-mysql.ssh_passphrase")
        );
        assert_eq!(ssh.fingerprint.as_deref(), Some("SHA256:abc123def456"));
        assert!(ssh.keyring_password_ref.is_none());
        assert!(ssh.known_hosts.is_none());
    }

    #[test]
    fn ssh_port_defaults_to_22_when_omitted() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "pg"
name = "PG"
kind = "postgres"
keyring_url_ref = "dbboard.pg.url"

[connections.ssh]
host = "bastion.example"
user = "deploy"
keyring_password_ref = "dbboard.pg.ssh_password"
known_hosts = "/home/user/.ssh/known_hosts"
"#;
        let file = ConnectionFile::parse(toml_src).expect("parses");
        assert_eq!(file.connections[0].ssh.as_ref().unwrap().port, 22);
    }

    #[test]
    fn ssh_block_round_trips_through_serialization() {
        let original = ConnectionFile::parse(pg_ssh_toml()).expect("parse");
        let serialized = toml::to_string(&original).expect("serialize");
        let reparsed = ConnectionFile::parse(&serialized).expect("re-parse");
        assert_eq!(original, reparsed);
    }

    #[test]
    fn ssh_tunnel_on_a_non_tunnelable_kind_is_rejected() {
        // Turso is a local file — an ssh tunnel makes no sense and must be a
        // hard error, not silently ignored.
        let toml_src = r#"
version = 1

[[connections]]
id   = "local"
name = "Local"
kind = "turso"
path = ":memory:"

[connections.ssh]
host = "bastion.example"
user = "deploy"
key_path = "/k"
fingerprint = "SHA256:abc"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("turso + ssh must fail");
        assert!(matches!(err, ConfigError::SshUnsupportedKind { .. }));
    }

    #[test]
    fn ssh_with_two_auth_methods_is_rejected() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "pg"
name = "PG"
kind = "postgres"
keyring_url_ref = "dbboard.pg.url"

[connections.ssh]
host = "bastion.example"
user = "deploy"
key_path = "/k"
keyring_password_ref = "dbboard.pg.ssh_password"
fingerprint = "SHA256:abc"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("two auth methods must fail");
        assert!(matches!(err, ConfigError::SshInvalid { .. }));
    }

    #[test]
    fn ssh_without_a_host_key_policy_is_rejected() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "pg"
name = "PG"
kind = "postgres"
keyring_url_ref = "dbboard.pg.url"

[connections.ssh]
host = "bastion.example"
user = "deploy"
key_path = "/k"
"#;
        let err = ConnectionFile::parse(toml_src).expect_err("no host-key policy must fail");
        assert!(matches!(err, ConfigError::SshInvalid { .. }));
    }

    #[test]
    fn serialized_ssh_block_has_no_secret_value_keys() {
        // The passphrase/password live behind keyring refs; even the ref key
        // names must not trip the `password =` / `secret =` grep guard.
        let file = ConnectionFile::parse(pg_ssh_toml()).expect("parse");
        let serialized = toml::to_string(&file).expect("serialize");
        for forbidden_key in ["password =", "passphrase =", "secret =", "token ="] {
            assert!(
                !serialized.contains(forbidden_key),
                "serialized ssh block must not expose `{forbidden_key}`: {serialized}"
            );
        }
        assert!(serialized.contains("keyring_passphrase_ref ="));
    }

    #[test]
    fn ssh_keyring_refs_lists_only_the_secret_references() {
        let ssh = SshTunnelToml {
            host: "h".into(),
            port: 22,
            user: "u".into(),
            key_path: Some("/k".into()),
            keyring_passphrase_ref: Some("dbboard.x.ssh_passphrase".into()),
            keyring_password_ref: None,
            fingerprint: Some("SHA256:abc".into()),
            known_hosts: None,
        };
        assert_eq!(ssh.keyring_refs(), vec!["dbboard.x.ssh_passphrase"]);

        let key_only = SshTunnelToml {
            keyring_passphrase_ref: None,
            ..ssh
        };
        assert!(key_only.keyring_refs().is_empty());
    }

    // `mcp_write` (ADR-0087) gates the dbboard-mcp write tools per connection.
    // Every existing `connections.toml` predates the key, so absence has to
    // mean "read-only" rather than a parse error.

    #[test]
    fn mcp_write_defaults_to_false_when_absent() {
        let file = ConnectionFile::parse(pg_ssh_toml()).expect("parses");
        assert!(
            !file.connections[0].mcp_write,
            "a connection that never opted in must not be writable"
        );
    }

    #[test]
    fn mcp_write_opt_in_parses() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "pg"
name = "PG"
kind = "postgres"
keyring_url_ref = "dbboard.pg.url"
mcp_write = true
"#;
        let file = ConnectionFile::parse(toml_src).expect("parses");
        assert!(file.connections[0].mcp_write);
    }

    #[test]
    fn mcp_write_round_trips_and_stays_before_the_ssh_table() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "work-mysql"
name = "Work MySQL"
kind = "mysql"
keyring_url_ref = "dbboard.work-mysql.url"
mcp_write = true

[connections.ssh]
host = "bastion.example"
user = "deploy"
keyring_password_ref = "dbboard.work-mysql.ssh_password"
known_hosts = "/home/user/.ssh/known_hosts"
"#;
        let original = ConnectionFile::parse(toml_src).expect("parse");
        let serialized = toml::to_string(&original).expect("serialize");
        // TOML requires scalars before tables: emitting `mcp_write` after
        // `[connections.ssh]` would put it inside the tunnel table instead.
        let write_at = serialized.find("mcp_write").expect("key is emitted");
        let ssh_at = serialized
            .find("[connections.ssh]")
            .expect("ssh table is emitted");
        assert!(
            write_at < ssh_at,
            "mcp_write must precede the ssh table: {serialized}"
        );
        assert_eq!(
            original,
            ConnectionFile::parse(&serialized).expect("re-parse")
        );
    }

    #[test]
    fn read_only_connections_do_not_gain_an_mcp_write_key() {
        // Serialization rewrites the whole file. Emitting `mcp_write = false`
        // for every entry would churn every existing config on first save.
        let file = ConnectionFile::parse(pg_ssh_toml()).expect("parse");
        let serialized = toml::to_string(&file).expect("serialize");
        assert!(
            !serialized.contains("mcp_write"),
            "the default must stay absent from disk: {serialized}"
        );
    }

    // `mcp_alias` (ADR-0088) is the name an AI agent sees instead of the id.
    // Ids routinely carry a host and an account (`app@db.internal`), and the
    // id is on the very first tool result an agent produces.

    #[test]
    fn mcp_alias_is_absent_by_default() {
        let file = ConnectionFile::parse(pg_ssh_toml()).expect("parses");
        assert_eq!(
            file.connections[0].mcp_alias, None,
            "no alias means the id is used, which is today's behaviour"
        );
    }

    #[test]
    fn mcp_alias_round_trips_and_stays_before_the_ssh_table() {
        let toml_src = r#"
version = 1

[[connections]]
id   = "work-mysql"
name = "Work MySQL"
kind = "mysql"
keyring_url_ref = "dbboard.work-mysql.url"
mcp_alias = "shop-db"

[connections.ssh]
host = "bastion.example"
user = "deploy"
keyring_password_ref = "dbboard.work-mysql.ssh_password"
known_hosts = "/home/user/.ssh/known_hosts"
"#;
        let original = ConnectionFile::parse(toml_src).expect("parse");
        assert_eq!(
            original.connections[0].mcp_alias.as_deref(),
            Some("shop-db")
        );

        let serialized = toml::to_string(&original).expect("serialize");
        // Same TOML constraint as `mcp_write`: a scalar emitted after
        // `[connections.ssh]` would land inside the tunnel table.
        let alias_at = serialized.find("mcp_alias").expect("key is emitted");
        let ssh_at = serialized
            .find("[connections.ssh]")
            .expect("ssh table is emitted");
        assert!(
            alias_at < ssh_at,
            "mcp_alias must precede the ssh table: {serialized}"
        );
        assert_eq!(
            original,
            ConnectionFile::parse(&serialized).expect("re-parse")
        );
    }

    #[test]
    fn connections_without_an_alias_do_not_gain_the_key() {
        let file = ConnectionFile::parse(pg_ssh_toml()).expect("parse");
        let serialized = toml::to_string(&file).expect("serialize");
        assert!(
            !serialized.contains("mcp_alias"),
            "the default must stay absent from disk: {serialized}"
        );
    }
}
