//! Wire a [`BackendConfig`] up to a concrete [`DatabaseAdapter`].
//!
//! Consumers hold an `Arc<dyn DatabaseAdapter>` produced here and
//! dispatch through the trait surface only; the adapter kind never
//! leaks. Adding a new adapter means a new match arm below and no
//! changes to any consumer (ADR-0012).
//!
//! Callers own the connected adapter for its lifetime — never
//! reconnecting per request. That is load-bearing for Turso `:memory:`,
//! where each fresh connection is its *own* empty database; reconnecting
//! would silently lose any `CREATE TABLE`. Both `dbboard-server` (one
//! adapter) and `dbboard-mcp` (a per-connection-id cache, ADR-0046) rely
//! on it.

use std::sync::Arc;

use dbboard_core::{DatabaseAdapter, DbError, DbResult};
use dbboard_d1::D1Adapter;
use dbboard_firestore::FirestoreAdapter;
use dbboard_mongodb::MongoAdapter;
use dbboard_mysql::{MySqlAdapter, MySqlConfig};
use dbboard_postgres::{AuroraDsqlIamParams, PostgresAdapter, PostgresConfig};
use dbboard_tunnel::SshTunnel;
use dbboard_turso::TursoAdapter;

use crate::config::BackendConfig;
use crate::ssh::{
    forward_target, rewrite_to_loopback, ResolvedSsh, DEFAULT_MYSQL_PORT, DEFAULT_POSTGRES_PORT,
};
use crate::tunneled::TunneledAdapter;

/// Open the SSH tunnel for a URL-bearing backend, if one is configured, and
/// return the URL the adapter should actually dial paired with the live tunnel
/// guard. With no tunnel the URL passes through unchanged and the guard is
/// `None`.
///
/// The forward target — the real database `host:port` — comes from the URL
/// itself; once the tunnel binds a loopback port, the URL is rewritten to that
/// port so the ordinary TCP adapter connects through the forward (ADR-0069).
///
/// # Errors
/// [`DbError::Connection`] if the URL cannot be parsed or the tunnel fails to
/// open (host-key mismatch, auth failure, unreachable bastion).
async fn open_tunnel(
    url: String,
    ssh: Option<ResolvedSsh>,
    default_port: u16,
) -> DbResult<(String, Option<SshTunnel>)> {
    let Some(ssh) = ssh else {
        return Ok((url, None));
    };
    let (forward_host, forward_port) = forward_target(&url, default_port)?;
    let tunnel = dbboard_tunnel::open(ssh.into_tunnel_config(forward_host, forward_port))
        .await
        .map_err(|e| DbError::Connection(format!("ssh tunnel: {e}")))?;
    let rewritten = rewrite_to_loopback(&url, tunnel.local_port())?;
    Ok((rewritten, Some(tunnel)))
}

/// Wrap a connected adapter so an open tunnel lives exactly as long as it. No
/// tunnel means no wrapper — the bare adapter is returned unchanged.
fn wrap<A: DatabaseAdapter + 'static>(
    adapter: A,
    tunnel: Option<SshTunnel>,
) -> Arc<dyn DatabaseAdapter> {
    match tunnel {
        None => Arc::new(adapter),
        Some(tunnel) => Arc::new(TunneledAdapter::new(Arc::new(adapter), tunnel)),
    }
}

/// Resolve a [`BackendConfig`] into a connected, trait-object adapter.
///
/// Connection failures surface here so a bad token, URL, or file path
/// is reported at startup rather than on the first request. For
/// non-self-validating drivers (D1, sqlx) this also runs `ping()` so
/// the fail-fast contract holds uniformly across adapters.
///
/// # Errors
///
/// Returns a [`DbError`] if the adapter cannot connect — a bad token,
/// URL, or file path, or a failed `ping()` reachability check.
///
/// [`DbError`]: dbboard_core::DbError
pub async fn connect_adapter(config: BackendConfig) -> DbResult<Arc<dyn DatabaseAdapter>> {
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
        BackendConfig::Postgres { url, ssh } => {
            let (url, tunnel) = open_tunnel(url, ssh, DEFAULT_POSTGRES_PORT).await?;
            let adapter = PostgresAdapter::connect(PostgresConfig { url }).await?;
            // sqlx lazily verifies the pool; force the first round-trip
            // here so a bad URL or rejected credentials surface as a
            // startup connection error. Through a tunnel this also proves
            // the forward end-to-end before the connection is handed out.
            adapter.ping().await?;
            Ok(wrap(adapter, tunnel))
        }
        BackendConfig::MySql { url, ssh } => {
            // A genuinely different dialect served by the dbboard-mysql
            // adapter (ADR-0068), not a Postgres-wire flavor. sqlx lazily
            // verifies the pool, so force the first round-trip here to
            // surface a bad URL or rejected credentials at startup.
            let (url, tunnel) = open_tunnel(url, ssh, DEFAULT_MYSQL_PORT).await?;
            let adapter = MySqlAdapter::connect(MySqlConfig { url }).await?;
            adapter.ping().await?;
            Ok(wrap(adapter, tunnel))
        }
        BackendConfig::Neon { url, ssh } => {
            // Same wire protocol as Postgres; the only difference is the
            // flavor label exposed by `id()` (ADR-0018).
            let (url, tunnel) = open_tunnel(url, ssh, DEFAULT_POSTGRES_PORT).await?;
            let adapter = PostgresAdapter::connect_neon(PostgresConfig { url }).await?;
            adapter.ping().await?;
            Ok(wrap(adapter, tunnel))
        }
        BackendConfig::Supabase { url, ssh } => {
            // Same wire protocol as Postgres; the only difference is the
            // flavor label exposed by `id()` (ADR-0019). Both the direct
            // (:5432) and transaction-pooler (:6543) endpoints route
            // through here — the URL itself encodes the choice.
            let (url, tunnel) = open_tunnel(url, ssh, DEFAULT_POSTGRES_PORT).await?;
            let adapter = PostgresAdapter::connect_supabase(PostgresConfig { url }).await?;
            adapter.ping().await?;
            Ok(wrap(adapter, tunnel))
        }
        BackendConfig::AuroraDsql { url, ssh } => {
            // Same wire protocol as Postgres; the only difference is the
            // flavor label exposed by `id()` (ADR-0021). The URL's
            // password segment is expected to carry a short-lived IAM
            // authentication token (~15 min TTL); an expired token
            // surfaces here as a `DbError::Connection`.
            let (url, tunnel) = open_tunnel(url, ssh, DEFAULT_POSTGRES_PORT).await?;
            let adapter = PostgresAdapter::connect_aurora_dsql(PostgresConfig { url }).await?;
            adapter.ping().await?;
            Ok(wrap(adapter, tunnel))
        }
        BackendConfig::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_key,
        } => {
            // Aurora DSQL flavor (ADR-0021), but the adapter mints its own
            // SigV4 IAM token here from the AWS credentials rather than
            // being handed a pre-signed URL (ADR-0036). A background task
            // re-mints the token and swaps in a freshly authenticated pool
            // before expiry (ADR-0037 段階B), so an unattended 24/7
            // connection survives Aurora DSQL's idle recycle. The secret_key
            // came from the OS keychain; the refresh task retains it for the
            // adapter's lifetime and it is never logged.
            let adapter = PostgresAdapter::connect_aurora_dsql_iam(AuroraDsqlIamParams {
                endpoint,
                region,
                database,
                username,
                access_key_id,
                secret_key,
            })
            .await?;
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::Firestore(cfg) => {
            let adapter = FirestoreAdapter::connect(cfg)?;
            // Like D1, `connect` only builds the HTTP client — and for a
            // service account it has not yet exchanged the signed assertion
            // for an access token. `ping` is what proves the credentials
            // actually work, so a bad key fails at startup rather than on
            // the first query.
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
        BackendConfig::MongoDb(cfg) => {
            // `connect` parses the URI (and resolves SRV) but opens no socket
            // for a plain `mongodb://`, so without this ping an unreachable
            // server would only surface on the first query.
            let adapter = MongoAdapter::connect(cfg).await?;
            adapter.ping().await?;
            Ok(Arc::new(adapter))
        }
    }
}
