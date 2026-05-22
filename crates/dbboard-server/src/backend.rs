//! The connected adapter behind the HTTP handlers.
//!
//! Moved here from `apps/dbboard` in Phase 1.5. The server holds a
//! single connected [`Backend`] in an `Arc` and shares it across every
//! request — never reconnecting per request. This is load-bearing for
//! Turso `:memory:`, where each fresh connection is its *own* empty
//! database; reconnecting would silently lose any `CREATE TABLE`.

use dbboard_core::{DbResult, QueryResult, TableInfo};
use dbboard_d1::D1Adapter;
use dbboard_postgres::{PostgresAdapter, PostgresConfig};
use dbboard_turso::TursoAdapter;

use crate::config::BackendConfig;

/// A connected adapter. The variants share the small command surface
/// the handlers need; statement dispatch is a plain `match`.
pub(crate) enum Backend {
    Turso(TursoAdapter),
    D1(D1Adapter),
    Postgres(PostgresAdapter),
}

impl Backend {
    pub(crate) async fn connect(config: BackendConfig) -> DbResult<Self> {
        match config {
            BackendConfig::Turso { path } => {
                Ok(Self::Turso(TursoAdapter::connect_local(&path).await?))
            }
            BackendConfig::D1(cfg) => {
                let adapter = D1Adapter::connect(cfg)?;
                // Verify connectivity up front so a bad token or id
                // surfaces as a connection error at startup, matching
                // how the Turso path fails fast on a bad file.
                adapter.ping().await?;
                Ok(Self::D1(adapter))
            }
            BackendConfig::Postgres { url } => {
                let adapter = PostgresAdapter::connect(PostgresConfig { url }).await?;
                // Same fail-fast contract: surface a bad URL or rejected
                // credentials as a startup connection error.
                adapter.ping().await?;
                Ok(Self::Postgres(adapter))
            }
        }
    }

    pub(crate) async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        match self {
            Self::Turso(a) => a.list_tables().await,
            Self::D1(a) => a.list_tables().await,
            Self::Postgres(a) => a.list_tables().await,
        }
    }

    pub(crate) async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        match self {
            Self::Turso(a) => a.query(sql).await,
            Self::D1(a) => a.query(sql).await,
            Self::Postgres(a) => a.query(sql).await,
        }
    }
}
