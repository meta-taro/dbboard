//! Which database the server connects to, and how that choice is
//! resolved from the environment and the local connection store.
//!
//! This logic moved here from `apps/dbboard` in Phase 1.5 (ADR-0009):
//! the binary no longer reads database environment variables — the
//! server owns backend selection so the desktop and (future) headless
//! deployments share one source of truth. Phase 2 / ADR-0013 widens the
//! resolver to consult `connections.toml` after the environment has had
//! its say.

use std::fmt;

use dbboard_config::{ConfigError, ConnectionEntry, ConnectionFile, ConnectionKind, SecretStore};
use dbboard_d1::D1Config;

const TURSO_PATH_ENV: &str = "DBBOARD_TURSO_PATH";
const DEFAULT_TURSO_PATH: &str = ":memory:";

const D1_ACCOUNT_ID_ENV: &str = "DBBOARD_D1_ACCOUNT_ID";
const D1_DATABASE_ID_ENV: &str = "DBBOARD_D1_DATABASE_ID";
const D1_TOKEN_ENV: &str = "DBBOARD_D1_TOKEN";
const D1_BASE_URL_ENV: &str = "DBBOARD_D1_BASE_URL";

const PG_URL_ENV: &str = "DBBOARD_PG_URL";
const NEON_URL_ENV: &str = "DBBOARD_NEON_URL";
const SUPABASE_URL_ENV: &str = "DBBOARD_SUPABASE_URL";
const AURORA_DSQL_URL_ENV: &str = "DBBOARD_AURORA_DSQL_URL";

const CONNECTION_SELECTOR_ENV: &str = "DBBOARD_CONNECTION";

/// What the server should connect to. Resolved cheaply (no I/O) and
/// handed to [`crate::serve`], which does the actual connecting inside
/// its tokio runtime.
pub enum BackendConfig {
    Turso {
        path: String,
    },
    D1(D1Config),
    Postgres {
        url: String,
    },
    /// Postgres-wire connection labelled as Neon (ADR-0018). Wire shape
    /// is identical to [`BackendConfig::Postgres`]; the distinction is
    /// the flavor the adapter exposes through `id()`, so the connection
    /// picker and history records can name "neon" instead of generic
    /// "postgres".
    Neon {
        url: String,
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
    },
    /// Postgres-wire connection labelled as AWS Aurora DSQL (ADR-0021).
    /// Wire shape is identical to [`BackendConfig::Postgres`]; the
    /// distinction is the flavor the adapter exposes through `id()`. The
    /// URL is expected to embed a short-lived IAM authentication token
    /// (~15 min TTL) in its password field; automatic refresh via the
    /// AWS SDK is out of scope for v=1 and will land via a future ADR.
    AuroraDsql {
        url: String,
    },
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
            Self::Neon { .. } => f.write_str("Neon(<redacted>)"),
            Self::Supabase { .. } => f.write_str("Supabase(<redacted>)"),
            Self::AuroraDsql { .. } => f.write_str("AuroraDsql(<redacted>)"),
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
/// 5. The `DBBOARD_D1_*` trio — Cloudflare D1 over REST.
/// 6. Otherwise local Turso/libSQL at `DBBOARD_TURSO_PATH` (default
///    `":memory:"`), so a fresh checkout runs without configuration.
///
/// This entry point does not consult `connections.toml`; for the
/// merged resolver used by `apps/dbboard` see
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
/// 5. The `DBBOARD_D1_*` trio — wins outright.
/// 6. `DBBOARD_TURSO_PATH` — wins outright (explicit local path).
/// 7. `DBBOARD_CONNECTION=<id>` — picks the matching entry from `file`.
/// 8. If `file` has exactly one entry — auto-select it.
/// 9. Otherwise Turso `:memory:` (the unchanged default).
///
/// Secret-bearing entries (D1, Postgres) resolve their credentials
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
    d1_account_id: Option<String>,
    d1_database_id: Option<String>,
    d1_token: Option<String>,
    d1_base_url: Option<String>,
    turso_path: Option<String>,
    connection_selector: Option<String>,
}

impl EnvSnapshot {
    fn from_process() -> Self {
        Self {
            aurora_dsql_url: non_empty(std::env::var(AURORA_DSQL_URL_ENV).ok()),
            neon_url: non_empty(std::env::var(NEON_URL_ENV).ok()),
            supabase_url: non_empty(std::env::var(SUPABASE_URL_ENV).ok()),
            pg_url: non_empty(std::env::var(PG_URL_ENV).ok()),
            d1_account_id: non_empty(std::env::var(D1_ACCOUNT_ID_ENV).ok()),
            d1_database_id: non_empty(std::env::var(D1_DATABASE_ID_ENV).ok()),
            d1_token: non_empty(std::env::var(D1_TOKEN_ENV).ok()),
            d1_base_url: non_empty(std::env::var(D1_BASE_URL_ENV).ok()),
            turso_path: non_empty(std::env::var(TURSO_PATH_ENV).ok()),
            connection_selector: non_empty(std::env::var(CONNECTION_SELECTOR_ENV).ok()),
        }
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

fn resolve_from_env_only(env: &EnvSnapshot) -> BackendConfig {
    if let Some(url) = env.aurora_dsql_url.clone() {
        return BackendConfig::AuroraDsql { url };
    }
    if let Some(url) = env.neon_url.clone() {
        return BackendConfig::Neon { url };
    }
    if let Some(url) = env.supabase_url.clone() {
        return BackendConfig::Supabase { url };
    }
    if let Some(url) = env.pg_url.clone() {
        return BackendConfig::Postgres { url };
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
    if env.aurora_dsql_url.is_some() {
        return Ok(resolve_from_env_only(env));
    }
    if env.neon_url.is_some() {
        return Ok(resolve_from_env_only(env));
    }
    if env.supabase_url.is_some() {
        return Ok(resolve_from_env_only(env));
    }
    if env.pg_url.is_some() {
        return Ok(resolve_from_env_only(env));
    }
    if env.d1_account_id.is_some() && env.d1_database_id.is_some() && env.d1_token.is_some() {
        return Ok(resolve_from_env_only(env));
    }
    if env.turso_path.is_some() {
        return Ok(resolve_from_env_only(env));
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
///   `"env:postgres"`, `"env:d1"`, `"env:turso"` so the user can see at
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
    if env.d1_account_id.is_some() && env.d1_database_id.is_some() && env.d1_token.is_some() {
        return "env:d1".to_string();
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
            Ok(BackendConfig::Postgres { url })
        }
        ConnectionKind::Neon { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::Neon { url })
        }
        ConnectionKind::Supabase { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::Supabase { url })
        }
        ConnectionKind::AuroraDsql { keyring_url_ref } => {
            let url = secrets.get(keyring_url_ref)?;
            Ok(BackendConfig::AuroraDsql { url })
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
            id: id.to_string(),
            name: format!("turso {id}"),
            kind: ConnectionKind::Turso {
                path: path.to_string(),
            },
        }
    }

    fn d1_entry(id: &str, token_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
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
            id: id.to_string(),
            name: format!("pg {id}"),
            kind: ConnectionKind::Postgres {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn neon_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            id: id.to_string(),
            name: format!("neon {id}"),
            kind: ConnectionKind::Neon {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn supabase_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            id: id.to_string(),
            name: format!("supabase {id}"),
            kind: ConnectionKind::Supabase {
                keyring_url_ref: url_ref.to_string(),
            },
        }
    }

    fn aurora_dsql_entry(id: &str, url_ref: &str) -> ConnectionEntry {
        ConnectionEntry {
            id: id.to_string(),
            name: format!("aurora-dsql {id}"),
            kind: ConnectionKind::AuroraDsql {
                keyring_url_ref: url_ref.to_string(),
            },
        }
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
            matches!(cfg, BackendConfig::Postgres { url } if url == "postgres://from-env"),
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
            matches!(cfg, BackendConfig::Neon { url } if url == "postgres://from-neon-env"),
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
            matches!(cfg, BackendConfig::Supabase { url } if url == "postgres://from-supabase-env"),
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
            matches!(cfg, BackendConfig::Neon { url } if url == "postgres://from-neon"),
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
            matches!(cfg, BackendConfig::AuroraDsql { url } if url == "postgres://from-aurora-dsql-env"),
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
            matches!(cfg, BackendConfig::AuroraDsql { url } if url == "postgres://from-aurora-dsql"),
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
            matches!(cfg, BackendConfig::AuroraDsql { url } if url == "postgres://from-store-as-aurora-dsql"),
            "Aurora DSQL URL must be loaded from the secret store under the AuroraDsql variant"
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
            matches!(cfg, BackendConfig::Supabase { url } if url == "postgres://from-store-as-supabase"),
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
            matches!(cfg, BackendConfig::Neon { url } if url == "postgres://from-store-as-neon"),
            "Neon URL must be loaded from the secret store under the Neon variant"
        );
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
            matches!(cfg, BackendConfig::Postgres { url } if url == "postgres://from-store"),
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
        };
        let rendered_pg = format!("{pg:?}");
        assert!(!rendered_pg.contains("pw@host"), "{rendered_pg}");

        let neon = BackendConfig::Neon {
            url: "postgres://user:neon-pw@neon.example/db".to_string(),
        };
        let rendered_neon = format!("{neon:?}");
        assert!(!rendered_neon.contains("neon-pw"), "{rendered_neon}");

        let supabase = BackendConfig::Supabase {
            url: "postgres://postgres:supa-pw@db.example.supabase.co/postgres".to_string(),
        };
        let rendered_supabase = format!("{supabase:?}");
        assert!(
            !rendered_supabase.contains("supa-pw"),
            "{rendered_supabase}"
        );

        let aurora_dsql = BackendConfig::AuroraDsql {
            url: "postgres://admin:dsql-iam-pw@example.dsql.us-east-1.on.aws/postgres".to_string(),
        };
        let rendered_aurora_dsql = format!("{aurora_dsql:?}");
        assert!(
            !rendered_aurora_dsql.contains("dsql-iam-pw"),
            "{rendered_aurora_dsql}"
        );
    }
}
