//! Wire a [`BackendConfig`] up to a concrete [`DatabaseAdapter`].
//!
//! The HTTP handlers never see the adapter kind: they hold an
//! `Arc<dyn DatabaseAdapter>` produced here and dispatch through the
//! trait surface only. Adding a new adapter means a new match arm
//! below and no changes to the handlers (ADR-0012).
//!
//! The server owns the single connected adapter for the lifetime of
//! the process — never reconnecting per request. That is load-bearing
//! for Turso `:memory:`, where each fresh connection is its *own* empty
//! database; reconnecting would silently lose any `CREATE TABLE`.

use std::sync::Arc;

use dbboard_core::{DatabaseAdapter, DbResult};
use dbboard_d1::D1Adapter;
use dbboard_postgres::{PostgresAdapter, PostgresConfig};
use dbboard_turso::TursoAdapter;

use crate::config::BackendConfig;

/// Resolve a [`BackendConfig`] into a connected, trait-object adapter.
///
/// Connection failures surface here so a bad token, URL, or file path
/// is reported at startup rather than on the first request. For
/// non-self-validating drivers (D1, sqlx) this also runs `ping()` so
/// the fail-fast contract holds uniformly across adapters.
pub(crate) async fn connect_adapter(config: BackendConfig) -> DbResult<Arc<dyn DatabaseAdapter>> {
    match config {
        BackendConfig::Turso { path } => {
            let adapter = TursoAdapter::connect_local(&path).await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::D1(cfg) => {
            let adapter = D1Adapter::connect(cfg)?;
            // D1Adapter::connect builds the HTTP client without touching
            // the network, so verify reachability up front to match how
            // the Turso path fails fast on a bad file.
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::Postgres { url } => {
            let adapter = PostgresAdapter::connect(PostgresConfig { url }).await?;
            // sqlx lazily verifies the pool; force the first round-trip
            // here so a bad URL or rejected credentials surface as a
            // startup connection error.
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::Neon { url } => {
            // Same wire protocol as Postgres; the only difference is the
            // flavor label exposed by `id()` (ADR-0018).
            let adapter = PostgresAdapter::connect_neon(PostgresConfig { url }).await?;
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::Supabase { url } => {
            // Same wire protocol as Postgres; the only difference is the
            // flavor label exposed by `id()` (ADR-0019). Both the direct
            // (:5432) and transaction-pooler (:6543) endpoints route
            // through here — the URL itself encodes the choice.
            let adapter = PostgresAdapter::connect_supabase(PostgresConfig { url }).await?;
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::AuroraDsql { url } => {
            // Same wire protocol as Postgres; the only difference is the
            // flavor label exposed by `id()` (ADR-0021). The URL's
            // password segment is expected to carry a short-lived IAM
            // authentication token (~15 min TTL); an expired token
            // surfaces here as a `DbError::Connection`.
            let adapter = PostgresAdapter::connect_aurora_dsql(PostgresConfig { url }).await?;
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
    }
}
