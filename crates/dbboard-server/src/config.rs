//! Which database the server connects to, and how that choice is
//! resolved from the environment.
//!
//! This logic moved here from `apps/dbboard` in Phase 1.5 (ADR-0009):
//! the binary no longer reads database environment variables — the
//! server owns backend selection so the desktop and (future) headless
//! deployments share one source of truth.

use std::fmt;

use dbboard_d1::D1Config;

const TURSO_PATH_ENV: &str = "DBBOARD_TURSO_PATH";
const DEFAULT_TURSO_PATH: &str = ":memory:";

const D1_ACCOUNT_ID_ENV: &str = "DBBOARD_D1_ACCOUNT_ID";
const D1_DATABASE_ID_ENV: &str = "DBBOARD_D1_DATABASE_ID";
const D1_TOKEN_ENV: &str = "DBBOARD_D1_TOKEN";
const D1_BASE_URL_ENV: &str = "DBBOARD_D1_BASE_URL";

const PG_URL_ENV: &str = "DBBOARD_PG_URL";

/// What the server should connect to. Resolved cheaply (no I/O) and
/// handed to [`crate::serve`], which does the actual connecting inside
/// its tokio runtime.
pub enum BackendConfig {
    Turso { path: String },
    D1(D1Config),
    Postgres { url: String },
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
        }
    }
}

/// Resolve the backend from the environment, in priority order:
///
/// 1. `DBBOARD_PG_URL` — a PostgreSQL-wire database (`CockroachDB`, Neon).
/// 2. The `DBBOARD_D1_*` trio — Cloudflare D1 over REST.
/// 3. Otherwise local Turso/libSQL at `DBBOARD_TURSO_PATH` (default
///    `":memory:"`), so a fresh checkout runs without configuration.
#[must_use]
pub fn backend_config_from_env() -> BackendConfig {
    // An explicit Postgres URL is the most specific intent, so it wins
    // over everything else.
    if let Ok(url) = std::env::var(PG_URL_ENV) {
        if !url.trim().is_empty() {
            return BackendConfig::Postgres { url };
        }
    }

    // D1 wins only when its three required vars are all set; the fourth
    // (DBBOARD_D1_BASE_URL) is optional. A partial D1 setup falls back
    // to Turso rather than failing, so a stray env var can't lock the
    // app out of its default local mode.
    if let (Ok(account_id), Ok(database_id), Ok(api_token)) = (
        std::env::var(D1_ACCOUNT_ID_ENV),
        std::env::var(D1_DATABASE_ID_ENV),
        std::env::var(D1_TOKEN_ENV),
    ) {
        return BackendConfig::D1(D1Config {
            account_id,
            database_id,
            api_token,
            base_url: std::env::var(D1_BASE_URL_ENV).ok(),
        });
    }

    let path = std::env::var(TURSO_PATH_ENV).unwrap_or_else(|_| DEFAULT_TURSO_PATH.to_owned());
    BackendConfig::Turso { path }
}
