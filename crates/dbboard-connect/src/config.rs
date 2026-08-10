//! Which database the server connects to, and how that choice is
//! resolved from the environment and the local connection store.
//!
//! This logic moved here from the desktop binary in Phase 1.5 (ADR-0009):
//! the binary no longer reads database environment variables — the
//! server owns backend selection so the desktop and (future) headless
//! deployments share one source of truth. Phase 2 / ADR-0013 widens the
//! resolver to consult `connections.toml` after the environment has had
//! its say.

use std::fmt;

use dbboard_config::{ConfigError, ConnectionEntry, ConnectionFile, ConnectionKind, SecretStore};
use dbboard_d1::D1Config;
use dbboard_firestore::{FirestoreConfig, FirestoreCredentials};
use dbboard_mongodb::MongoConfig;

use crate::ssh::{resolved_ssh_from_env, ResolvedSsh, SshEnv};

const SSH_HOST_ENV: &str = "DBBOARD_SSH_HOST";
const SSH_PORT_ENV: &str = "DBBOARD_SSH_PORT";
const SSH_USER_ENV: &str = "DBBOARD_SSH_USER";
const SSH_KEY_PATH_ENV: &str = "DBBOARD_SSH_KEY_PATH";
const SSH_KEY_PASSPHRASE_ENV: &str = "DBBOARD_SSH_KEY_PASSPHRASE";
const SSH_PASSWORD_ENV: &str = "DBBOARD_SSH_PASSWORD";
const SSH_FINGERPRINT_ENV: &str = "DBBOARD_SSH_FINGERPRINT";
const SSH_KNOWN_HOSTS_ENV: &str = "DBBOARD_SSH_KNOWN_HOSTS";

const TURSO_PATH_ENV: &str = "DBBOARD_TURSO_PATH";
const DEFAULT_TURSO_PATH: &str = ":memory:";

const D1_ACCOUNT_ID_ENV: &str = "DBBOARD_D1_ACCOUNT_ID";
const D1_DATABASE_ID_ENV: &str = "DBBOARD_D1_DATABASE_ID";
const D1_TOKEN_ENV: &str = "DBBOARD_D1_TOKEN";
const D1_BASE_URL_ENV: &str = "DBBOARD_D1_BASE_URL";

const FIRESTORE_PROJECT_ID_ENV: &str = "DBBOARD_FIRESTORE_PROJECT_ID";
const FIRESTORE_SERVICE_ACCOUNT_ENV: &str = "DBBOARD_FIRESTORE_SERVICE_ACCOUNT";
const FIRESTORE_DATABASE_ID_ENV: &str = "DBBOARD_FIRESTORE_DATABASE_ID";
const FIRESTORE_BASE_URL_ENV: &str = "DBBOARD_FIRESTORE_BASE_URL";

const MONGODB_URI_ENV: &str = "DBBOARD_MONGODB_URI";
const MONGODB_DATABASE_ENV: &str = "DBBOARD_MONGODB_DATABASE";

const PG_URL_ENV: &str = "DBBOARD_PG_URL";
const MYSQL_URL_ENV: &str = "DBBOARD_MYSQL_URL";
const NEON_URL_ENV: &str = "DBBOARD_NEON_URL";
const SUPABASE_URL_ENV: &str = "DBBOARD_SUPABASE_URL";
const AURORA_DSQL_URL_ENV: &str = "DBBOARD_AURORA_DSQL_URL";

const CONNECTION_SELECTOR_ENV: &str = "DBBOARD_CONNECTION";

/// What to connect to. Resolved cheaply (no I/O); the actual connecting
/// is done by [`crate::connect_adapter`] inside a tokio runtime.
pub enum BackendConfig {
    Turso {
        path: String,
    },
    D1(D1Config),
    Postgres {
        url: String,
        /// Optional SSH tunnel to forward through (ADR-0069). When present,
        /// [`crate::connect_adapter`] opens it and rewrites `url` to the
        /// loopback forward before connecting.
        ssh: Option<ResolvedSsh>,
    },
    /// `MySQL` connection (ADR-0068). Unlike the Postgres-wire family this
    /// is a genuinely different SQL dialect served by the `dbboard-mysql`
    /// adapter (back-tick quoting, backslash-escaped literals). The URL
    /// (`mysql://…`) embeds the password, so it is redacted in `Debug`.
    MySql {
        url: String,
        /// Optional SSH tunnel to forward through (ADR-0069).
        ssh: Option<ResolvedSsh>,
    },
    /// Postgres-wire connection labelled as Neon (ADR-0018). Wire shape
    /// is identical to [`BackendConfig::Postgres`]; the distinction is
    /// the flavor the adapter exposes through `id()`, so the connection
    /// picker and history records can name "neon" instead of generic
    /// "postgres".
    Neon {
        url: String,
        /// Optional SSH tunnel to forward through (ADR-0069).
        ssh: Option<ResolvedSsh>,
    },
    /// Postgres-wire connection labelled as Supabase (ADR-0019). Wire
    /// shape is identical to [`BackendConfig::Postgres`]; the
    /// distinction is the flavor the adapter exposes through `id()`,
    /// so the connection picker and history records can name "supabase"
    /// instead of generic "postgres". REST surfaces (auth / storage /
    /// realtime / functions) are out of scope for this variant; a
    /// future ADR will introduce them with the matching capability
    /// flag extension.
    Supabase {
        url: String,
        /// Optional SSH tunnel to forward through (ADR-0069).
        ssh: Option<ResolvedSsh>,
    },
    /// Postgres-wire connection labelled as AWS Aurora DSQL (ADR-0021).
    /// Wire shape is identical to [`BackendConfig::Postgres`]; the
    /// distinction is the flavor the adapter exposes through `id()`. The
    /// URL is expected to embed a short-lived IAM authentication token
    /// (~15 min TTL) in its password field; automatic refresh via the
    /// AWS SDK is out of scope for v=1 and will land via a future ADR.
    AuroraDsql {
        url: String,
        /// Optional SSH tunnel to forward through (ADR-0069).
        ssh: Option<ResolvedSsh>,
    },
    /// AWS Aurora DSQL with agent-minted IAM auth (ADR-0036). Unlike
    /// [`BackendConfig::AuroraDsql`], the caller does not supply a
    /// pre-signed URL; instead the adapter mints a fresh `SigV4` token at
    /// build time from the AWS credentials carried here. `secret_key` is
    /// the resolved AWS secret access key (from the OS keychain, never a
    /// tracked file) and is redacted in `Debug`. v1 mints once at build;
    /// 24/7 auto-refresh is deferred to a follow-up ADR.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_key: String,
    },
    /// Cloud Firestore over REST (ADR-0091, ADR-0093). The first non-SQL
    /// backend, and the only one whose credential is genuinely optional:
    /// [`FirestoreCredentials::Emulator`] carries none, because the local
    /// emulator accepts a fixed `Bearer owner`.
    Firestore(FirestoreConfig),
    /// `MongoDB` over the wire protocol (ADR-0096). The URI embeds the
    /// password in its authority, so the whole struct is redacted in `Debug`
    /// exactly like the Postgres-wire URLs.
    MongoDb(MongoConfig),
}

impl BackendConfig {
    /// A local Turso/libSQL backend at `path`. Use `":memory:"` for an
    /// ephemeral database (the default, and what tests use).
    #[must_use]
    pub fn turso(path: impl Into<String>) -> Self {
        Self::Turso { path: path.into() }
    }
}

// Hand-written so the Postgres URL (embeds the password) and the D1 API
// token never reach a log line or panic message. Only the non-secret
// Turso path is shown in full.
impl fmt::Debug for BackendConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Turso { path } => f.debug_struct("Turso").field("path", path).finish(),
            Self::D1(_) => f.write_str("D1(<redacted>)"),
            Self::Postgres { .. } => f.write_str("Postgres(<redacted>)"),
            Self::MySql { .. } => f.write_str("MySql(<redacted>)"),
            Self::Neon { .. } => f.write_str("Neon(<redacted>)"),
            Self::Supabase { .. } => f.write_str("Supabase(<redacted>)"),
            Self::AuroraDsql { .. } => f.write_str("AuroraDsql(<redacted>)"),
            // endpoint/region/username/access_key_id are non-secret, but
            // secret_key must never surface — redact the whole struct to
            // keep the Debug impl trivially safe against field reordering.
            Self::AuroraDsqlIam { .. } => f.write_str("AuroraDsqlIam(<redacted>)"),
            // The service-account JSON embeds an RSA private key.
            Self::Firestore(_) => f.write_str("Firestore(<redacted>)"),
            // The URI's authority carries the password.
            Self::MongoDb(_) => f.write_str("MongoDb(<redacted>)"),
        }
    }
}

/// Resolve the backend from the environment, in priority order:
///
/// 1. `DBBOARD_AURORA_DSQL_URL` — an AWS Aurora DSQL Postgres-wire
///    database (ADR-0021). Ranks first by alphabetical tiebreaker
///    between the three specific pg-wire labels (aurora-dsql < neon <
///    supabase).
/// 2. `DBBOARD_NEON_URL` — a Neon Postgres-wire database (ADR-0018).
///    Ranks above Supabase and the generic `DBBOARD_PG_URL`.
/// 3. `DBBOARD_SUPABASE_URL` — a Supabase Postgres-wire database
///    (ADR-0019). Ranks below Neon and above generic `DBBOARD_PG_URL`.
/// 4. `DBBOARD_PG_URL` — a PostgreSQL-wire database (`CockroachDB`,
///    self-hosted Postgres).
/// 5. `DBBOARD_MYSQL_URL` — a `MySQL` / `MariaDB` database (ADR-0068). A
///    distinct dialect, not a pg-wire flavor; ranks just below the
///    Postgres-wire family.
/// 6. The `DBBOARD_D1_*` trio — Cloudflare D1 over REST.
/// 7. `DBBOARD_FIRESTORE_PROJECT_ID` — Cloud Firestore over REST
///    (ADR-0093). The project id alone is enough: without
///    `DBBOARD_FIRESTORE_SERVICE_ACCOUNT` this targets the local emulator.
/// 8. `DBBOARD_MONGODB_URI` — a `MongoDB` deployment (ADR-0096). Ranked
///    below Firestore so an environment configured before `MongoDB` existed
///    keeps resolving to what it did. `DBBOARD_MONGODB_DATABASE` is optional
///    because the URI may name the database in its path.
/// 9. Otherwise local Turso/libSQL at `DBBOARD_TURSO_PATH` (default
///    `":memory:"`), so a fresh checkout runs without configuration.
///
/// This entry point does not consult `connections.toml`; for the
/// merged resolver used by the client see
/// [`backend_config_from_env_and_store`].
#[must_use]
pub fn backend_config_from_env() -> BackendConfig {
    let env = EnvSnapshot::from_process();
    resolve_from_env_only(&env)
}

/// Resolve the backend from the environment first, then fall back to
/// `connections.toml` resolved through `store`. Priority order:
///
/// 1. `DBBOARD_AURORA_DSQL_URL` — wins outright (Aurora DSQL-flavored
///    Postgres, ADR-0021; first by alphabetical tiebreaker between the
///    three pg-wire specific labels).
/// 2. `DBBOARD_NEON_URL` — wins outright (Neon-flavored Postgres,
///    ADR-0018; ranks above Supabase and generic `DBBOARD_PG_URL`).
/// 3. `DBBOARD_SUPABASE_URL` — wins outright (Supabase-flavored
///    Postgres, ADR-0019; ranks above generic `DBBOARD_PG_URL`).
/// 4. `DBBOARD_PG_URL` — wins outright.
/// 5. `DBBOARD_MYSQL_URL` — wins outright (`MySQL` / `MariaDB`, ADR-0068;
///    a distinct dialect ranked just below the Postgres-wire family).
/// 6. The `DBBOARD_D1_*` trio — wins outright.
/// 7. `DBBOARD_FIRESTORE_PROJECT_ID` — wins outright (Cloud Firestore,
///    ADR-0093; the emulator when no service account accompanies it).
/// 8. `DBBOARD_MONGODB_URI` — wins outright (`MongoDB`, ADR-0096; ranked
///    below Firestore so an environment configured before it existed keeps
///    resolving to what it did).
/// 9. `DBBOARD_TURSO_PATH` — wins outright (explicit local path).
/// 10. `DBBOARD_CONNECTION=<id>` — picks the matching entry from `file`.
/// 11. If `file` has exactly one entry — auto-select it.
/// 12. Otherwise Turso `:memory:` (the unchanged default).
///
/// Secret-bearing entries (D1, Postgres, `MySQL`, `MongoDB`) resolve their credentials
/// through `secrets`, propagating [`ConfigError::Secret`] on miss so
/// the binary aborts before the loopback server binds.
///
/// # Errors
///
/// - [`ConfigError::DuplicateId`] never reaches here (caught at load
///   time) but is listed for completeness of the error surface.
/// - [`ConfigError::NoConfigDir`] when `DBBOARD_CONNECTION` names an id
///   the file does not contain — the resolver refuses to silently fall
///   back to a different backend than the user asked for.
/// - [`ConfigError::Secret`] when the secret store cannot resolve a
///   `keyring_*_ref`.
pub fn backend_config_from_env_and_store(
    file: &ConnectionFile,
    secrets: &dyn SecretStore,
) -> Result<BackendConfig, ConfigError> {
    let env = EnvSnapshot::from_process();
    resolve_backend(&env, file, secrets)
}

/// Captured view of every env var the resolver reads. Sourced once at
/// resolution time so the rest of the logic is pure and testable
/// without touching the process environment.
#[derive(Debug, Default, Clone)]
struct EnvSnapshot {
    aurora_dsql_url: Option<String>,
    neon_url: Option<String>,
    supabase_url: Option<String>,
    pg_url: Option<String>,
    mysql_url: Option<String>,
    d1_account_id: Option<String>,
    d1_database_id: Option<String>,
    d1_token: Option<String>,
    d1_base_url: Option<String>,
    firestore_project_id: Option<String>,
    firestore_service_account: Option<String>,
    firestore_database_id: Option<String>,
    firestore_base_url: Option<String>,
    mongodb_uri: Option<String>,
    mongodb_database: Option<String>,
    turso_path: Option<String>,
    connection_selector: Option<String>,
    ssh: SshEnv,
}

impl EnvSnapshot {
    fn from_process() -> Self {
        Self {
            aurora_dsql_url: non_empty(std::env::var(AURORA_DSQL_URL_ENV).ok()),
            neon_url: non_empty(std::env::var(NEON_URL_ENV).ok()),
            supabase_url: non_empty(std::env::var(SUPABASE_URL_ENV).ok()),
            pg_url: non_empty(std::env::var(PG_URL_ENV).ok()),
            mysql_url: non_empty(std::env::var(MYSQL_URL_ENV).ok()),
            d1_account_id: non_empty(std::env::var(D1_ACCOUNT_ID_ENV).ok()),
            d1_database_id: non_empty(std::env::var(D1_DATABASE_ID_ENV).ok()),
            d1_token: non_empty(std::env::var(D1_TOKEN_ENV).ok()),
            d1_base_url: non_empty(std::env::var(D1_BASE_URL_ENV).ok()),
            firestore_project_id: non_empty(std::env::var(FIRESTORE_PROJECT_ID_ENV).ok()),
            firestore_service_account: non_empty(std::env::var(FIRESTORE_SERVICE_ACCOUNT_ENV).ok()),
            firestore_database_id: non_empty(std::env::var(FIRESTORE_DATABASE_ID_ENV).ok()),
            firestore_base_url: non_empty(std::env::var(FIRESTORE_BASE_URL_ENV).ok()),
            mongodb_uri: non_empty(std::env::var(MONGODB_URI_ENV).ok()),
            mongodb_database: non_empty(std::env::var(MONGODB_DATABASE_ENV).ok()),
            turso_path: non_empty(std::env::var(TURSO_PATH_ENV).ok()),
            connection_selector: non_empty(std::env::var(CONNECTION_SELECTOR_ENV).ok()),
            ssh: SshEnv {
                host: non_empty(std::env::var(SSH_HOST_ENV).ok()),
                // A non-numeric DBBOARD_SSH_PORT is ignored (falls back to 22)
                // rather than failing the whole boot on a typo.
                port: non_empty(std::env::var(SSH_PORT_ENV).ok()).and_then(|p| p.parse().ok()),
                user: non_empty(std::env::var(SSH_USER_ENV).ok()),
                key_path: non_empty(std::env::var(SSH_KEY_PATH_ENV).ok()),
                key_passphrase: non_empty(std::env::var(SSH_KEY_PASSPHRASE_ENV).ok()),
                password: non_empty(std::env::var(SSH_PASSWORD_ENV).ok()),
                fingerprint: non_empty(std::env::var(SSH_FINGERPRINT_ENV).ok()),
                known_hosts: non_empty(std::env::var(SSH_KNOWN_HOSTS_ENV).ok()),
            },
        }
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

/// Attach an env-provided SSH tunnel (`DBBOARD_SSH_*`) to a backend resolved
/// from an env-var URL. A no-op when no bastion host is set. Errors if the
/// bastion env is malformed, or if it names a backend that cannot be tunneled
/// (Turso local file, D1 and Firestore HTTPS) — the same policy the stored
/// path enforces.
fn attach_env_ssh(backend: BackendConfig, env: &EnvSnapshot) -> Result<BackendConfig, ConfigError> {
    let Some(resolved) = resolved_ssh_from_env(&env.ssh)? else {
        return Ok(backend);
    };
    match backend {
        BackendConfig::Postgres { url, .. } => Ok(BackendConfig::Postgres {
            url,
            ssh: Some(resolved),
        }),
        BackendConfig::MySql { url, .. } => Ok(BackendConfig::MySql {
            url,
            ssh: Some(resolved),
        }),
        BackendConfig::Neon { url, .. } => Ok(BackendConfig::Neon {
            url,
            ssh: Some(resolved),
        }),
        BackendConfig::Supabase { url, .. } => Ok(BackendConfig::Supabase {
            url,
            ssh: Some(resolved),
        }),
        BackendConfig::AuroraDsql { url, .. } => Ok(BackendConfig::AuroraDsql {
            url,
            ssh: Some(resolved),
        }),
        // Turso is a local file and D1 is an HTTPS API; neither routes through
        // a forwarded TCP port. Mirror the stored-entry policy and refuse
        // rather than silently ignore the tunnel the user configured.
        BackendConfig::Turso { .. } => Err(ConfigError::SshUnsupportedKind {
            id: "env".to_string(),
            kind: "Turso",
        }),
        BackendConfig::D1(_) => Err(ConfigError::SshUnsupportedKind {
            id: "env".to_string(),
            kind: "Cloudflare D1",
        }),
        BackendConfig::AuroraDsqlIam { .. } => Err(ConfigError::SshUnsupportedKind {
            id: "env".to_string(),
            kind: "Aurora DSQL (IAM)",
        }),
        BackendConfig::Firestore(_) => Err(ConfigError::SshUnsupportedKind {
            id: "env".to_string(),
            kind: "Firestore",
        }),
        // MongoDB does speak TCP, but a URI may list several hosts and
        // `mongodb+srv://` discovers a replica set from DNS. Rewriting one host
        // to a loopback forward leaves the driver failing over to the members
        // the tunnel does not front — working at first, then silently not.
        BackendConfig::MongoDb(_) => Err(ConfigError::SshUnsupportedKind {
            id: "env".to_string(),
            kind: "MongoDB",
        }),
    }
}

/// A Firestore backend from `DBBOARD_FIRESTORE_*`, or `None` when the
/// project id is absent.
///
/// The project id alone is a complete configuration: with no
/// `DBBOARD_FIRESTORE_SERVICE_ACCOUNT` this is the local emulator, exactly
/// as a stored entry with no `keyring_service_account_ref` is. So the
/// service account cannot be part of the trigger condition the way the D1
/// trio's token is — requiring it would make the emulator unreachable from
/// the environment.
fn firestore_from_env(env: &EnvSnapshot) -> Option<FirestoreConfig> {
    let project_id = env.firestore_project_id.clone()?;
    Some(FirestoreConfig {
        project_id,
        database_id: env.firestore_database_id.clone(),
        credentials: env.firestore_service_account.clone().map_or(
            FirestoreCredentials::Emulator,
            FirestoreCredentials::ServiceAccountJson,
        ),
        base_url: env.firestore_base_url.clone(),
    })
}

fn resolve_from_env_only(env: &EnvSnapshot) -> BackendConfig {
    // Env-var backends carry no tunnel here; any `DBBOARD_SSH_*` bastion is
    // attached by `attach_env_ssh` on the fallible resolver path so an invalid
    // env tunnel surfaces as an error rather than being silently dropped.
    if let Some(url) = env.aurora_dsql_url.clone() {
        return BackendConfig::AuroraDsql { url, ssh: None };
    }
    if let Some(url) = env.neon_url.clone() {
        return BackendConfig::Neon { url, ssh: None };
    }
    if let Some(url) = env.supabase_url.clone() {
        return BackendConfig::Supabase { url, ssh: None };
    }
    if let Some(url) = env.pg_url.clone() {
        return BackendConfig::Postgres { url, ssh: None };
    }
    if let Some(url) = env.mysql_url.clone() {
        return BackendConfig::MySql { url, ssh: None };
    }
    if let (Some(account_id), Some(database_id), Some(api_token)) = (
        env.d1_account_id.clone(),
        env.d1_database_id.clone(),
        env.d1_token.clone(),
    ) {
        return BackendConfig::D1(D1Config {
            account_id,
            database_id,
            api_token,
            base_url: env.d1_base_url.clone(),
        });
    }
    if let Some(firestore) = firestore_from_env(env) {
        return BackendConfig::Firestore(firestore);
    }
    if let Some(uri) = env.mongodb_uri.clone() {
        return BackendConfig::MongoDb(MongoConfig {
            uri,
            database: env.mongodb_database.clone(),
        });
    }
    BackendConfig::Turso {
        path: env
            .turso_path
            .clone()
            .unwrap_or_else(|| DEFAULT_TURSO_PATH.to_owned()),
    }
}

fn resolve_backend(
    env: &EnvSnapshot,
    file: &ConnectionFile,
    secrets: &dyn SecretStore,
) -> Result<BackendConfig, ConfigError> {
    // Rule 1-6: env-only wins. Aurora DSQL URL (alphabetically first
    // among the specific pg-wire labels), then Neon URL, then Supabase
    // URL, then generic Postgres URL, then the D1 trio, then an
    // explicit TURSO_PATH all short-circuit the file-backed store.
    if env.aurora_dsql_url.is_some()
        || env.neon_url.is_some()
        || env.supabase_url.is_some()
        || env.pg_url.is_some()
        || env.mysql_url.is_some()
    {
        // A URL-bearing env backend can front a `DBBOARD_SSH_*` bastion.
        return attach_env_ssh(resolve_from_env_only(env), env);
    }
    if env.d1_account_id.is_some() && env.d1_database_id.is_some() && env.d1_token.is_some() {
        // D1 cannot be tunneled; surface a configured bastion as an error
        // rather than dropping it silently.
        return attach_env_ssh(resolve_from_env_only(env), env);
    }
    if env.firestore_project_id.is_some() {
        // Firestore is an HTTPS API; same refusal as D1.
        return attach_env_ssh(resolve_from_env_only(env), env);
    }
    if env.mongodb_uri.is_some() {
        // MongoDB refuses a tunnel too, for the multi-host reason in
        // `attach_env_ssh`; route it through the same check.
        return attach_env_ssh(resolve_from_env_only(env), env);
    }
    if env.turso_path.is_some() {
        // Likewise a local Turso file cannot be tunneled.
        return attach_env_ssh(resolve_from_env_only(env), env);
    }

    // Rule 4: explicit selector by id. Missing id is a hard error so we
    // do not silently swap to `:memory:` when the user asked for a
    // specific named entry.
    if let Some(id) = env.connection_selector.as_deref() {
        let entry = file
            .connections
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| ConfigError::DuplicateId(format!("no connection with id={id}")))?;
        return entry_to_backend(entry, secrets);
    }

    // Rule 5: a single entry is unambiguous, so auto-select it.
    if file.connections.len() == 1 {
        return entry_to_backend(&file.connections[0], secrets);
    }

    // Rule 6: no env, no selector, no single entry — fall back to the
    // memory database so a fresh checkout always boots.
    Ok(BackendConfig::Turso {
        path: DEFAULT_TURSO_PATH.to_owned(),
    })
}

/// Label naming the connection the server is about to resolve, derived
/// by the same precedence rules as [`backend_config_from_env_and_store`].
///
/// Used by the desktop binary to populate the `conn` field on ADR-0017
/// history records: it identifies *which* connection produced each
/// recorded query so a multi-connection user can grep their `history.jsonl`
/// for one target.
///
/// The label is intentionally lightweight (no I/O, no secret resolution)
/// because it is computed at boot before the loopback server binds. The
/// shape:
///
/// - Env-only wins (env var path of [`backend_config_from_env_and_store`]):
///   `"env:postgres"`, `"env:d1"`, `"env:firestore"`, `"env:turso"` so the user can see at
///   a glance that the connection came from an environment variable.
/// - Explicit `DBBOARD_CONNECTION=<id>` returns `<id>` when the id
///   exists in the file; an unknown id falls through to the in-memory
///   default (matching how the resolver errors at a deeper layer).
/// - Single-entry auto-select returns that entry's id.
/// - Otherwise `"in-memory"` for the `:memory:` Turso fallback.
#[must_use]
pub fn resolved_connection_label(file: &ConnectionFile) -> String {
    let env = EnvSnapshot::from_process();
    label_for(&env, file)
}

fn label_for(env: &EnvSnapshot, file: &ConnectionFile) -> String {
    if env.aurora_dsql_url.is_some() {
        return "env:aurora-dsql".to_string();
    }
    if env.neon_url.is_some() {
        return "env:neon".to_string();
    }
    if env.supabase_url.is_some() {
        return "env:supabase".to_string();
    }
    if env.pg_url.is_some() {
        return "env:postgres".to_string();
    }
    if env.mysql_url.is_some() {
        return "env:mysql".to_string();
    }
    if env.d1_account_id.is_some() && env.d1_database_id.is_some() && env.d1_token.is_some() {
        return "env:d1".to_string();
    }
    if env.firestore_project_id.is_some() {
        return "env:firestore".to_string();
    }
    if env.mongodb_uri.is_some() {
        return "env:mongodb".to_string();
    }
    if env.turso_path.is_some() {
        return "env:turso".to_string();
    }
    if let Some(id) = env.connection_selector.as_deref() {
        // A selector that names an existing entry resolves to that id;
        // a selector that names a missing entry must NOT silently fall
        // through to single-entry auto-select — the deeper resolver
        // errors on that case, and the label is a display mirror of it.
        return if file.connections.iter().any(|e| e.id == id) {
            id.to_string()
        } else {
            "in-memory".to_string()
        };
    }
    if file.connections.len() == 1 {
        return file.connections[0].id.clone();
    }
    "in-memory".to_string()
}

/// Translate a single connection-store entry into the [`BackendConfig`]
/// the server needs to connect it. Looks up any secret-field references
/// in `secrets`. Used by the runtime connection switcher (ADR-0020) and
/// by [`resolve_backend`] internally.
///
/// # Errors
///
/// Propagates [`ConfigError`] when a referenced keyring entry cannot be
/// read (missing, denied, or the OS keychain is unreachable).
pub fn backend_config_for_entry(
    entry: &ConnectionEntry,
    secrets: &dyn SecretStore,
) -> Result<BackendConfig, ConfigError> {
    entry_to_backend(entry, secrets)
}

/// Resolve an entry's optional `[connections.ssh]` block into a
/// [`ResolvedSsh`], pulling its passphrase/password from `secrets`. Only the
/// URL-bearing kinds reach this with a `Some` block — the loader rejects a
/// tunnel on an un-tunnelable kind at parse time ([`ConnectionKind::supports_ssh_tunnel`]).
fn entry_ssh(
    entry: &ConnectionEntry,
    secrets: &dyn SecretStore,
) -> Result<Option<ResolvedSsh>, ConfigError> {
    match &entry.ssh {
        Some(ssh) => Ok(Some(ResolvedSsh::from_toml(ssh, &entry.id, secrets)?)),
        None => Ok(None),
    }
}

fn entry_to_backend(
    entry: &ConnectionEntry,
    secrets: &dyn SecretStore,
) -> Result<BackendConfig, ConfigError> {
    match &entry.kind {
        ConnectionKind::Turso { path } => Ok(BackendConfig::Turso { path: path.clone() }),
        ConnectionKind::D1 {
            account_id,
            database_id,
            base_url,
            keyring_token_ref,
        } => {
            let api_token = secrets.get(keyring_token_ref)?;
            Ok(BackendConfig::D1(D1Config {
                account_id: account_id.clone(),
                database_id: database_id.clone(),
                api_token,
                base_url: base_url.clone(),
            }))
        }
        ConnectionKind::Postgres { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::Postgres {
                url,
                ssh: entry_ssh(entry, secrets)?,
            })
        }
        ConnectionKind::MySql { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::MySql {
                url,
                ssh: entry_ssh(entry, secrets)?,
            })
        }
        ConnectionKind::Neon { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::Neon {
                url,
                ssh: entry_ssh(entry, secrets)?,
            })
        }
        ConnectionKind::Supabase { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::Supabase {
                url,
                ssh: entry_ssh(entry, secrets)?,
            })
        }
        ConnectionKind::AuroraDsql { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::AuroraDsql {
                url,
                ssh: entry_ssh(entry, secrets)?,
            })
        }
        ConnectionKind::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            keyring_secret_key_ref,
        } => {
            // Only the AWS secret access key lives in the keychain; every
            // other field is non-secret and stored inline in the config.
            let secret_key = secrets.get(keyring_secret_key_ref)?;
            Ok(BackendConfig::AuroraDsqlIam {
                endpoint: endpoint.clone(),
                region: region.clone(),
                database: database.clone(),
                username: username.clone(),
                access_key_id: access_key_id.clone(),
                secret_key,
            })
        }
        ConnectionKind::Firestore {
            project_id,
            database_id,
            base_url,
            keyring_service_account_ref,
        } => {
            // No reference means the emulator, which has no credential —
            // so the secret store is never consulted, and an emulator
            // connection cannot fail on a keychain miss.
            let credentials = match keyring_service_account_ref {
                Some(key_ref) => FirestoreCredentials::ServiceAccountJson(secrets.get(key_ref)?),
                None => FirestoreCredentials::Emulator,
            };
            Ok(BackendConfig::Firestore(FirestoreConfig {
                project_id: project_id.clone(),
                database_id: database_id.clone(),
                credentials,
                base_url: base_url.clone(),
            }))
        }
        ConnectionKind::MongoDb {
            keyring_url_ref,
            database,
        } => {
            let uri = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::MongoDb(MongoConfig {
                uri,
                database: database.clone(),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_config::{ConnectionEntry, ConnectionFile, InMemorySecretStore, CONFIG_VERSION};

    fn empty_env() -> EnvSnapshot {
        EnvSnapshot::default()
    }

    fn empty_file() -> ConnectionFile {
        ConnectionFile::empty()
    }

    fn file_with(entries: Vec<ConnectionEntry>) -> ConnectionFile {
        ConnectionFile {
            version: CONFIG_VERSION,
            connections: entries,
        }
    }

    fn turso_entry(id: &str, path: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("turso {id}"),
            kind: ConnectionKind::Turso {
                path: path.to_string(),
            },
        }
    }

    fn d1_entry(id: &str, token_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("d1 {id}"),
            kind: ConnectionKind::D1 {
                account_id: "acct".to_string(),
                database_id: "db".to_string(),
                base_url: None,
                keyring_token_ref: token_ref.to_string(),
            },
        }
    }

    fn pg_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("pg {id}"),
            kind: ConnectionKind::Postgres {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn mysql_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("mysql {id}"),
            kind: ConnectionKind::MySql {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn neon_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("neon {id}"),
            kind: ConnectionKind::Neon {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn supabase_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("supabase {id}"),
            kind: ConnectionKind::Supabase {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn aurora_dsql_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("aurora-dsql {id}"),
            kind: ConnectionKind::AuroraDsql {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn aurora_dsql_iam_entry(id: &str, secret_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("aurora-dsql-iam {id}"),
            kind: ConnectionKind::AuroraDsqlIam {
                endpoint: "abc123.dsql.ap-northeast-1.on.aws".to_string(),
                region: "ap-northeast-1".to_string(),
                database: "postgres".to_string(),
                username: "admin".to_string(),
                access_key_id: "AKIAEXAMPLE".to_string(),
                keyring_secret_key_ref: secret_ref.to_string(),
            },
        }
    }

    /// `service_account_ref: None` is the emulator — the one kind whose
    /// keychain reference is optional (see `ConnectionKind::Firestore`).
    fn firestore_entry(id: &str, service_account_ref: Option<&str>) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("firestore {id}"),
            kind: ConnectionKind::Firestore {
                project_id: "demo-project".to_string(),
                database_id: None,
                base_url: None,
                keyring_service_account_ref: service_account_ref.map(ToString::to_string),
            },
        }
    }

    #[test]
    fn firestore_entry_resolves_the_service_account_through_the_secret_store() {
        let file = file_with(vec![firestore_entry(
            "fs",
            Some("dbboard.fs.service_account"),
        )]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set(
                "dbboard.fs.service_account",
                r#"{"type":"service_account"}"#,
            )
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::Firestore(fs) => {
                assert_eq!(fs.project_id, "demo-project");
                assert_eq!(fs.database_id, None);
                assert_eq!(fs.base_url, None);
                match fs.credentials {
                    FirestoreCredentials::ServiceAccountJson(json) => {
                        assert_eq!(json, r#"{"type":"service_account"}"#);
                    }
                    other @ FirestoreCredentials::Emulator => {
                        panic!("expected a service account, got {other:?}")
                    }
                }
            }
            other => panic!("expected Firestore, got {other:?}"),
        }
    }

    #[test]
    fn firestore_emulator_entry_resolves_with_no_credential_at_all() {
        // No ref means the emulator. An empty secret store proves the
        // resolver never reaches for a credential that does not exist —
        // an emulator connection must not fail on a keychain miss.
        let file = file_with(vec![ConnectionEntry {
            kind: ConnectionKind::Firestore {
                project_id: "demo-project".to_string(),
                database_id: Some("(default)".to_string()),
                base_url: Some("http://127.0.0.1:8080/v1".to_string()),
                keyring_service_account_ref: None,
            },
            ..firestore_entry("emu", None)
        }]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::Firestore(fs) => {
                assert_eq!(fs.database_id.as_deref(), Some("(default)"));
                assert_eq!(fs.base_url.as_deref(), Some("http://127.0.0.1:8080/v1"));
                assert!(
                    matches!(fs.credentials, FirestoreCredentials::Emulator),
                    "a missing service-account ref means the emulator"
                );
            }
            other => panic!("expected Firestore, got {other:?}"),
        }
    }

    #[test]
    fn firestore_entry_with_missing_secret_propagates_secret_error() {
        let file = file_with(vec![firestore_entry(
            "fs",
            Some("dbboard.fs.service_account"),
        )]);
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&empty_env(), &file, &secrets)
            .expect_err("a referenced but absent secret must surface");
        assert!(
            matches!(err, ConfigError::Secret(_)),
            "expected ConfigError::Secret, got {err:?}"
        );
    }

    #[test]
    fn firestore_env_vars_win_over_the_file_store() {
        let mut env = empty_env();
        env.firestore_project_id = Some("env-project".to_string());
        env.firestore_service_account = Some(r#"{"type":"service_account"}"#.to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::Firestore(fs) => {
                assert_eq!(fs.project_id, "env-project");
                assert!(matches!(
                    fs.credentials,
                    FirestoreCredentials::ServiceAccountJson(_)
                ));
            }
            other => panic!("expected Firestore, got {other:?}"),
        }
    }

    #[test]
    fn firestore_env_without_a_service_account_targets_the_emulator() {
        // Same rule as the stored entry: project id alone is a complete
        // emulator configuration, so it must not fall through to Turso.
        let mut env = empty_env();
        env.firestore_project_id = Some("env-project".to_string());
        env.firestore_base_url = Some("http://127.0.0.1:8080/v1".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        match cfg {
            BackendConfig::Firestore(fs) => {
                assert!(matches!(fs.credentials, FirestoreCredentials::Emulator));
                assert_eq!(fs.base_url.as_deref(), Some("http://127.0.0.1:8080/v1"));
            }
            other => panic!("expected Firestore, got {other:?}"),
        }
    }

    #[test]
    fn d1_env_outranks_firestore_env() {
        let mut env = empty_env();
        env.d1_account_id = Some("acct".to_string());
        env.d1_database_id = Some("db".to_string());
        env.d1_token = Some("tok".to_string());
        env.firestore_project_id = Some("env-project".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::D1(_)),
            "D1 ranks above Firestore, got {cfg:?}"
        );
    }

    #[test]
    fn firestore_env_outranks_turso_env() {
        let mut env = empty_env();
        env.firestore_project_id = Some("env-project".to_string());
        env.turso_path = Some("/tmp/x.db".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Firestore(_)),
            "Firestore ranks above the Turso fallback, got {cfg:?}"
        );
    }

    #[test]
    fn resolved_label_firestore_env_wins() {
        let mut env = empty_env();
        env.firestore_project_id = Some("env-project".to_string());
        assert_eq!(label_for(&env, &empty_file()), "env:firestore");
    }

    #[test]
    fn env_ssh_on_firestore_is_rejected() {
        // Firestore is an HTTPS API; a configured bastion must be an error
        // rather than silently dropped, exactly as for D1.
        let mut env = empty_env();
        env.firestore_project_id = Some("env-project".to_string());
        env.ssh.host = Some("bastion.example".to_string());
        env.ssh.user = Some("ec2-user".to_string());
        env.ssh.key_path = Some("/tmp/key.pem".to_string());
        // A host-key policy is validated before the kind is even looked at,
        // so supply one — otherwise this asserts the wrong refusal.
        env.ssh.fingerprint = Some("SHA256:AAAA".to_string());
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&env, &empty_file(), &secrets)
            .expect_err("a tunnel on Firestore must be refused");
        assert!(
            matches!(
                err,
                ConfigError::SshUnsupportedKind {
                    kind: "Firestore",
                    ..
                }
            ),
            "expected SshUnsupportedKind for Firestore, got {err:?}"
        );
    }

    fn mongodb_entry(id: &str, url_ref: &str, database: Option<&str>) -> ConnectionEntry {
        ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("mongo {id}"),
            kind: ConnectionKind::MongoDb {
                keyring_url_ref: url_ref.to_string(),
                database: database.map(ToString::to_string),
            },
        }
    }

    #[test]
    fn mongodb_entry_resolves_the_uri_through_the_secret_store() {
        let file = file_with(vec![mongodb_entry(
            "mongo",
            "dbboard.mongo.url",
            Some("orders"),
        )]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.mongo.url", "mongodb://user:pw@127.0.0.1:27117")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::MongoDb(m) => {
                assert_eq!(m.uri, "mongodb://user:pw@127.0.0.1:27117");
                assert_eq!(m.database.as_deref(), Some("orders"));
            }
            other => panic!("expected MongoDb, got {other:?}"),
        }
    }

    #[test]
    fn mongodb_entry_with_missing_secret_propagates_secret_error() {
        let file = file_with(vec![mongodb_entry("mongo", "dbboard.mongo.url", None)]);
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&empty_env(), &file, &secrets)
            .expect_err("a referenced but absent secret must surface");
        assert!(
            matches!(err, ConfigError::Secret(_)),
            "expected ConfigError::Secret, got {err:?}"
        );
    }

    /// The URI carries the password in its authority, so it must never reach a
    /// log line — the same guarantee the Postgres and D1 variants make.
    #[test]
    fn mongodb_debug_never_prints_the_uri() {
        let cfg = BackendConfig::MongoDb(MongoConfig {
            uri: "mongodb://user:hunter2@127.0.0.1:27117".to_string(),
            database: Some("orders".to_string()),
        });
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("hunter2"), "leaked: {rendered}");
        assert!(!rendered.contains("127.0.0.1"), "leaked: {rendered}");
    }

    #[test]
    fn mongodb_env_uri_wins_over_the_file_store() {
        let mut env = empty_env();
        env.mongodb_uri = Some("mongodb://127.0.0.1:27117".to_string());
        env.mongodb_database = Some("orders".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::MongoDb(m) => {
                assert_eq!(m.uri, "mongodb://127.0.0.1:27117");
                assert_eq!(m.database.as_deref(), Some("orders"));
            }
            other => panic!("expected MongoDb, got {other:?}"),
        }
    }

    #[test]
    fn firestore_env_outranks_mongodb_env() {
        // Firestore was resolved before MongoDB existed; keeping it above
        // preserves what an already-configured environment resolves to.
        let mut env = empty_env();
        env.firestore_project_id = Some("env-project".to_string());
        env.mongodb_uri = Some("mongodb://127.0.0.1:27117".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Firestore(_)),
            "Firestore ranks above MongoDB, got {cfg:?}"
        );
    }

    #[test]
    fn mongodb_env_outranks_turso_env() {
        let mut env = empty_env();
        env.mongodb_uri = Some("mongodb://127.0.0.1:27117".to_string());
        env.turso_path = Some("/tmp/x.db".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::MongoDb(_)),
            "MongoDB ranks above the Turso fallback, got {cfg:?}"
        );
    }

    #[test]
    fn resolved_label_mongodb_env_wins() {
        let mut env = empty_env();
        env.mongodb_uri = Some("mongodb://127.0.0.1:27117".to_string());
        assert_eq!(label_for(&env, &empty_file()), "env:mongodb");
    }

    #[test]
    fn env_ssh_on_mongodb_is_rejected() {
        // Not because MongoDB lacks a TCP port, but because a URI may name
        // several hosts (and `mongodb+srv://` a whole replica set); forwarding
        // one of them would fail over to the untunnelled rest.
        let mut env = empty_env();
        env.mongodb_uri = Some("mongodb://127.0.0.1:27117".to_string());
        env.ssh.host = Some("bastion.example".to_string());
        env.ssh.user = Some("ec2-user".to_string());
        env.ssh.key_path = Some("/tmp/key.pem".to_string());
        env.ssh.fingerprint = Some("SHA256:AAAA".to_string());
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&env, &empty_file(), &secrets)
            .expect_err("a tunnel on MongoDB must be refused");
        assert!(
            matches!(
                err,
                ConfigError::SshUnsupportedKind {
                    kind: "MongoDB",
                    ..
                }
            ),
            "expected SshUnsupportedKind for MongoDB, got {err:?}"
        );
    }

    #[test]
    fn firestore_entry_with_an_ssh_block_is_rejected_by_the_store_layer() {
        // The kind refuses a tunnel at parse time; assert the property the
        // resolver relies on rather than duplicating the loader's test.
        assert!(!ConnectionKind::Firestore {
            project_id: "demo-project".to_string(),
            database_id: None,
            base_url: None,
            keyring_service_account_ref: None,
        }
        .supports_ssh_tunnel());
    }

    #[test]
    fn empty_env_and_empty_file_yields_in_memory_turso() {
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&empty_env(), &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Turso { path } if path == ":memory:"),
            "expected default in-memory turso"
        );
    }

    #[test]
    fn pg_env_var_wins_over_the_file_store() {
        let mut env = empty_env();
        env.pg_url = Some("postgres://from-env".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Postgres { url, .. } if url == "postgres://from-env"),
            "PG_URL must short-circuit the store"
        );
    }

    #[test]
    fn d1_trio_env_var_wins_over_the_file_store() {
        let mut env = empty_env();
        env.d1_account_id = Some("acct-env".to_string());
        env.d1_database_id = Some("db-env".to_string());
        env.d1_token = Some("tok-env".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::D1(d1) => {
                assert_eq!(d1.account_id, "acct-env");
                assert_eq!(d1.api_token, "tok-env");
            }
            other => panic!("expected D1 from env, got {other:?}"),
        }
    }

    #[test]
    fn partial_d1_env_falls_through_to_the_file_store() {
        let mut env = empty_env();
        env.d1_account_id = Some("acct-env".to_string());
        // database_id and token deliberately absent
        let file = file_with(vec![turso_entry("local", "/tmp/single.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Turso { path } if path == "/tmp/single.db"),
            "partial D1 env must not block the file-backed entry"
        );
    }

    #[test]
    fn turso_path_env_var_wins_over_the_file_store() {
        let mut env = empty_env();
        env.turso_path = Some("/tmp/from-env.db".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/from-file.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Turso { path } if path == "/tmp/from-env.db"),
            "explicit TURSO_PATH must short-circuit the store"
        );
    }

    #[test]
    fn connection_selector_picks_the_matching_id() {
        let mut env = empty_env();
        env.connection_selector = Some("prod".to_string());
        let file = file_with(vec![
            turso_entry("dev", "/tmp/dev.db"),
            turso_entry("prod", "/tmp/prod.db"),
        ]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Turso { path } if path == "/tmp/prod.db"),
            "DBBOARD_CONNECTION must select by id"
        );
    }

    #[test]
    fn connection_selector_for_unknown_id_is_an_error() {
        let mut env = empty_env();
        env.connection_selector = Some("nope".to_string());
        let file = file_with(vec![turso_entry("dev", "/tmp/dev.db")]);
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&env, &file, &secrets)
            .expect_err("missing id must not silently fall back");
        let msg = err.to_string();
        assert!(
            msg.contains("nope"),
            "error must name the missing id: {msg}"
        );
    }

    #[test]
    fn single_entry_file_is_auto_selected() {
        let file = file_with(vec![turso_entry("only", "/tmp/only.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Turso { path } if path == "/tmp/only.db"),
            "single entry must be auto-selected"
        );
    }

    #[test]
    fn multi_entry_file_without_selector_falls_back_to_in_memory() {
        let file = file_with(vec![
            turso_entry("dev", "/tmp/dev.db"),
            turso_entry("prod", "/tmp/prod.db"),
        ]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Turso { path } if path == ":memory:"),
            "ambiguous file with no selector must not silently pick one"
        );
    }

    #[test]
    fn d1_entry_resolves_token_through_the_secret_store() {
        let file = file_with(vec![d1_entry("cf", "dbboard.cf.token")]);
        let secrets = InMemorySecretStore::new();
        secrets.set("dbboard.cf.token", "live-token").expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::D1(d1) => assert_eq!(d1.api_token, "live-token"),
            other => panic!("expected D1, got {other:?}"),
        }
    }

    #[test]
    fn d1_entry_with_missing_secret_propagates_secret_error() {
        let file = file_with(vec![d1_entry("cf", "dbboard.cf.token")]);
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&empty_env(), &file, &secrets)
            .expect_err("missing secret must surface");
        assert!(
            matches!(err, ConfigError::Secret(_)),
            "expected ConfigError::Secret, got {err:?}"
        );
    }

    #[test]
    fn neon_env_var_wins_over_pg_env_var_and_the_file_store() {
        // ADR-0018: DBBOARD_NEON_URL ranks above DBBOARD_PG_URL because
        // it is the more specific labelling. Both being set is rare but
        // we still need a defined precedence.
        let mut env = empty_env();
        env.neon_url = Some("postgres://from-neon-env".to_string());
        env.pg_url = Some("postgres://from-pg-env".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Neon { url, .. } if url == "postgres://from-neon-env"),
            "NEON_URL must short-circuit the store and outrank PG_URL"
        );
    }

    #[test]
    fn supabase_env_var_wins_over_pg_env_var_and_the_file_store() {
        // ADR-0019: DBBOARD_SUPABASE_URL ranks above DBBOARD_PG_URL
        // because it is the more specific labelling. It ranks below
        // DBBOARD_NEON_URL by alphabetical tiebreaker between the two
        // specific labels — see supabase_env_ranks_below_neon below.
        let mut env = empty_env();
        env.supabase_url = Some("postgres://from-supabase-env".to_string());
        env.pg_url = Some("postgres://from-pg-env".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Supabase { url, .. } if url == "postgres://from-supabase-env"),
            "SUPABASE_URL must short-circuit the store and outrank PG_URL"
        );
    }

    #[test]
    fn supabase_env_ranks_below_neon_env() {
        // Both Neon and Supabase set → Neon wins (alphabetical tiebreak,
        // codified by ADR-0019 §Decision).
        let mut env = empty_env();
        env.neon_url = Some("postgres://from-neon".to_string());
        env.supabase_url = Some("postgres://from-supabase".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Neon { url, .. } if url == "postgres://from-neon"),
            "NEON_URL must outrank SUPABASE_URL"
        );
    }

    #[test]
    fn aurora_dsql_env_var_wins_over_pg_env_var_and_the_file_store() {
        // ADR-0021: DBBOARD_AURORA_DSQL_URL ranks above DBBOARD_PG_URL
        // because it is the more specific labelling, and first among the
        // three pg-wire specific labels by alphabetical tiebreaker.
        let mut env = empty_env();
        env.aurora_dsql_url = Some("postgres://from-aurora-dsql-env".to_string());
        env.pg_url = Some("postgres://from-pg-env".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::AuroraDsql { url, .. } if url == "postgres://from-aurora-dsql-env"),
            "AURORA_DSQL_URL must short-circuit the store and outrank PG_URL"
        );
    }

    #[test]
    fn aurora_dsql_env_outranks_neon_and_supabase_envs() {
        // All three set → Aurora DSQL wins (alphabetical tiebreak among
        // aurora-dsql < neon < supabase, codified by ADR-0021).
        let mut env = empty_env();
        env.aurora_dsql_url = Some("postgres://from-aurora-dsql".to_string());
        env.neon_url = Some("postgres://from-neon".to_string());
        env.supabase_url = Some("postgres://from-supabase".to_string());
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &empty_file(), &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::AuroraDsql { url, .. } if url == "postgres://from-aurora-dsql"),
            "AURORA_DSQL_URL must outrank NEON_URL and SUPABASE_URL"
        );
    }

    #[test]
    fn aurora_dsql_entry_resolves_url_through_the_secret_store() {
        let file = file_with(vec![aurora_dsql_entry("dsql", "dbboard.dsql.url")]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.dsql.url", "postgres://from-store-as-aurora-dsql")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::AuroraDsql { url, .. } if url == "postgres://from-store-as-aurora-dsql"),
            "Aurora DSQL URL must be loaded from the secret store under the AuroraDsql variant"
        );
    }

    #[test]
    fn aurora_dsql_iam_entry_resolves_secret_key_through_the_secret_store() {
        let file = file_with(vec![aurora_dsql_iam_entry(
            "dsql-iam",
            "dbboard.dsql-iam.secret_key",
        )]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.dsql-iam.secret_key", "live-aws-secret")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        match cfg {
            BackendConfig::AuroraDsqlIam {
                endpoint,
                region,
                database,
                username,
                access_key_id,
                secret_key,
            } => {
                assert_eq!(endpoint, "abc123.dsql.ap-northeast-1.on.aws");
                assert_eq!(region, "ap-northeast-1");
                assert_eq!(database, "postgres");
                assert_eq!(username, "admin");
                assert_eq!(access_key_id, "AKIAEXAMPLE");
                assert_eq!(secret_key, "live-aws-secret");
            }
            other => panic!("expected AuroraDsqlIam, got {other:?}"),
        }
    }

    #[test]
    fn aurora_dsql_iam_entry_with_missing_secret_propagates_secret_error() {
        let file = file_with(vec![aurora_dsql_iam_entry(
            "dsql-iam",
            "dbboard.dsql-iam.secret_key",
        )]);
        let secrets = InMemorySecretStore::new();
        let err = resolve_backend(&empty_env(), &file, &secrets)
            .expect_err("missing secret must surface");
        assert!(
            matches!(err, ConfigError::Secret(_)),
            "expected ConfigError::Secret, got {err:?}"
        );
    }

    #[test]
    fn supabase_entry_resolves_url_through_the_secret_store() {
        let file = file_with(vec![supabase_entry("supabase", "dbboard.supabase.url")]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.supabase.url", "postgres://from-store-as-supabase")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Supabase { url, .. } if url == "postgres://from-store-as-supabase"),
            "Supabase URL must be loaded from the secret store under the Supabase variant"
        );
    }

    #[test]
    fn neon_entry_resolves_url_through_the_secret_store() {
        let file = file_with(vec![neon_entry("neon", "dbboard.neon.url")]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.neon.url", "postgres://from-store-as-neon")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Neon { url, .. } if url == "postgres://from-store-as-neon"),
            "Neon URL must be loaded from the secret store under the Neon variant"
        );
    }

    #[test]
    fn mysql_env_var_wins_over_the_file_store() {
        let mut env = empty_env();
        env.mysql_url = Some("mysql://from-env".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        let secrets = InMemorySecretStore::new();
        let cfg = resolve_backend(&env, &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::MySql { url, .. } if url == "mysql://from-env"),
            "MYSQL_URL must short-circuit the store"
        );
    }

    #[test]
    fn mysql_entry_resolves_url_through_the_secret_store() {
        let file = file_with(vec![mysql_entry("shop", "dbboard.shop.url")]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.shop.url", "mysql://from-store")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::MySql { url, .. } if url == "mysql://from-store"),
            "MySQL URL must be loaded from the secret store"
        );
    }

    #[test]
    fn resolved_label_mysql_env_wins() {
        let mut env = empty_env();
        env.mysql_url = Some("mysql://shop".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        assert_eq!(label_for(&env, &file), "env:mysql");
    }

    #[test]
    fn postgres_entry_resolves_url_through_the_secret_store() {
        let file = file_with(vec![pg_entry("neon", "dbboard.neon.url")]);
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.neon.url", "postgres://from-store")
            .expect("seed");
        let cfg = resolve_backend(&empty_env(), &file, &secrets).expect("resolve");
        assert!(
            matches!(cfg, BackendConfig::Postgres { url, .. } if url == "postgres://from-store"),
            "Postgres URL must be loaded from the secret store"
        );
    }

    #[test]
    fn resolved_label_aurora_dsql_env_wins() {
        let mut env = empty_env();
        env.aurora_dsql_url = Some("postgres://aurora".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        assert_eq!(label_for(&env, &file), "env:aurora-dsql");
    }

    #[test]
    fn resolved_label_aurora_dsql_env_outranks_neon_supabase_and_pg_env() {
        // ADR-0021: aurora-dsql < neon < supabase alphabetically, so the
        // tiebreaker between the three specific pg-wire labels makes
        // Aurora DSQL win when more than one is set.
        let mut env = empty_env();
        env.aurora_dsql_url = Some("postgres://aurora".to_string());
        env.neon_url = Some("postgres://neon".to_string());
        env.supabase_url = Some("postgres://supabase".to_string());
        env.pg_url = Some("postgres://generic".to_string());
        assert_eq!(label_for(&env, &empty_file()), "env:aurora-dsql");
    }

    #[test]
    fn resolved_label_neon_env_wins() {
        let mut env = empty_env();
        env.neon_url = Some("postgres://neon".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        assert_eq!(label_for(&env, &file), "env:neon");
    }

    #[test]
    fn resolved_label_neon_env_outranks_pg_env() {
        let mut env = empty_env();
        env.neon_url = Some("postgres://neon".to_string());
        env.pg_url = Some("postgres://generic".to_string());
        assert_eq!(label_for(&env, &empty_file()), "env:neon");
    }

    #[test]
    fn resolved_label_supabase_env_wins() {
        let mut env = empty_env();
        env.supabase_url = Some("postgres://supabase".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        assert_eq!(label_for(&env, &file), "env:supabase");
    }

    #[test]
    fn resolved_label_supabase_env_outranks_pg_env() {
        let mut env = empty_env();
        env.supabase_url = Some("postgres://supabase".to_string());
        env.pg_url = Some("postgres://generic".to_string());
        assert_eq!(label_for(&env, &empty_file()), "env:supabase");
    }

    #[test]
    fn resolved_label_neon_env_outranks_supabase_env() {
        // Alphabetical tiebreaker between the two specific labels.
        let mut env = empty_env();
        env.neon_url = Some("postgres://neon".to_string());
        env.supabase_url = Some("postgres://supabase".to_string());
        assert_eq!(label_for(&env, &empty_file()), "env:neon");
    }

    #[test]
    fn resolved_label_pg_env_wins() {
        let mut env = empty_env();
        env.pg_url = Some("postgres://x".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        assert_eq!(label_for(&env, &file), "env:postgres");
    }

    #[test]
    fn resolved_label_d1_env_wins() {
        let mut env = empty_env();
        env.d1_account_id = Some("a".to_string());
        env.d1_database_id = Some("b".to_string());
        env.d1_token = Some("c".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/x.db")]);
        assert_eq!(label_for(&env, &file), "env:d1");
    }

    #[test]
    fn resolved_label_turso_env_wins() {
        let mut env = empty_env();
        env.turso_path = Some("/tmp/x.db".to_string());
        let file = file_with(vec![turso_entry("local", "/tmp/y.db")]);
        assert_eq!(label_for(&env, &file), "env:turso");
    }

    #[test]
    fn resolved_label_selector_picks_the_matching_id() {
        let mut env = empty_env();
        env.connection_selector = Some("prod".to_string());
        let file = file_with(vec![
            turso_entry("dev", "/tmp/dev.db"),
            turso_entry("prod", "/tmp/prod.db"),
        ]);
        assert_eq!(label_for(&env, &file), "prod");
    }

    #[test]
    fn resolved_label_selector_for_unknown_id_falls_back_to_in_memory() {
        // The deeper resolver errors on this case; the label resolver is
        // just a display helper and must not paper over the mismatch by
        // silently picking some other entry, so it falls through to the
        // in-memory default just like rule 6 in the backend resolver.
        let mut env = empty_env();
        env.connection_selector = Some("nope".to_string());
        let file = file_with(vec![turso_entry("dev", "/tmp/dev.db")]);
        assert_eq!(label_for(&env, &file), "in-memory");
    }

    #[test]
    fn resolved_label_single_entry_uses_its_id() {
        let file = file_with(vec![turso_entry("only", "/tmp/only.db")]);
        assert_eq!(label_for(&empty_env(), &file), "only");
    }

    #[test]
    fn resolved_label_empty_env_and_empty_file_yields_in_memory() {
        assert_eq!(label_for(&empty_env(), &empty_file()), "in-memory");
    }

    #[test]
    fn resolved_label_multi_entry_no_selector_yields_in_memory() {
        let file = file_with(vec![
            turso_entry("dev", "/tmp/dev.db"),
            turso_entry("prod", "/tmp/prod.db"),
        ]);
        assert_eq!(label_for(&empty_env(), &file), "in-memory");
    }

    #[test]
    fn debug_redacts_d1_and_postgres_secrets() {
        let d1 = BackendConfig::D1(D1Config {
            account_id: "acct".to_string(),
            database_id: "db".to_string(),
            api_token: "should-never-appear".to_string(),
            base_url: None,
        });
        let rendered = format!("{d1:?}");
        assert!(!rendered.contains("should-never-appear"), "{rendered}");

        let pg = BackendConfig::Postgres {
            url: "postgres://user:pw@host/db".to_string(),
            ssh: None,
        };
        let rendered_pg = format!("{pg:?}");
        assert!(!rendered_pg.contains("pw@host"), "{rendered_pg}");

        let neon = BackendConfig::Neon {
            url: "postgres://user:neon-pw@neon.example/db".to_string(),
            ssh: None,
        };
        let rendered_neon = format!("{neon:?}");
        assert!(!rendered_neon.contains("neon-pw"), "{rendered_neon}");

        let supabase = BackendConfig::Supabase {
            url: "postgres://postgres:supa-pw@db.example.supabase.co/postgres".to_string(),
            ssh: None,
        };
        let rendered_supabase = format!("{supabase:?}");
        assert!(
            !rendered_supabase.contains("supa-pw"),
            "{rendered_supabase}"
        );

        let aurora_dsql = BackendConfig::AuroraDsql {
            url: "postgres://admin:dsql-iam-pw@example.dsql.us-east-1.on.aws/postgres".to_string(),
            ssh: None,
        };
        let rendered_aurora_dsql = format!("{aurora_dsql:?}");
        assert!(
            !rendered_aurora_dsql.contains("dsql-iam-pw"),
            "{rendered_aurora_dsql}"
        );

        let aurora_dsql_iam = BackendConfig::AuroraDsqlIam {
            endpoint: "abc123.dsql.ap-northeast-1.on.aws".to_string(),
            region: "ap-northeast-1".to_string(),
            database: "postgres".to_string(),
            username: "admin".to_string(),
            access_key_id: "AKIAEXAMPLE".to_string(),
            secret_key: "super-secret-aws-key".to_string(),
        };
        let rendered_aurora_dsql_iam = format!("{aurora_dsql_iam:?}");
        assert!(
            !rendered_aurora_dsql_iam.contains("super-secret-aws-key"),
            "{rendered_aurora_dsql_iam}"
        );

        let firestore = BackendConfig::Firestore(FirestoreConfig {
            project_id: "demo-project".to_string(),
            database_id: None,
            // Deliberately carries no PEM armour line: the assertion below is
            // about the sentinel, and armour in a tracked file only costs us a
            // blocking `pii-scan` finding plus an allowlist entry that would
            // weaken the check against a real key.
            credentials: FirestoreCredentials::ServiceAccountJson(
                r#"{"private_key":"never-appear"}"#.to_string(),
            ),
            base_url: None,
        });
        let rendered_firestore = format!("{firestore:?}");
        assert!(
            !rendered_firestore.contains("never-appear"),
            "{rendered_firestore}"
        );
    }
}
