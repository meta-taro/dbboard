//! The read-only tool surface, independent of the MCP wire layer.
//!
//! [`McpService`] owns the security-sensitive work — resolving a
//! `connections.toml` entry plus its keyring secret into a connected
//! adapter, and running the five read-only operations exposed to an
//! external agent (ADR-0046 Decision 5). It knows nothing about `rmcp`,
//! JSON-RPC, or stdio: [`crate::server`] wraps each method as a tool and
//! translates errors onto the MCP envelope. Keeping the logic here means
//! it is testable against a real (in-memory) adapter with no transport.
//!
//! Two invariants this layer enforces:
//!
//! - **Secrets never leave.** [`list_connections`](McpService::list_connections)
//!   projects each entry to id/name/kind only; the keyring references and
//!   the resolved URLs/tokens are never serialized into a tool result.
//! - **Reads only.** Every query goes through
//!   [`DatabaseAdapter::query_read_only`], which each adapter enforces at
//!   the engine (Postgres `BEGIN READ ONLY`, libSQL `PRAGMA query_only`,
//!   D1 AST classification). This layer never calls the plain `query`
//!   path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use dbboard_config::annotations::{self, AnnotationsError, TableAnnotations};
use dbboard_config::store::{self, ConnectionKind};
use dbboard_config::{ConfigError, SecretStore};
use dbboard_connect::{backend_config_for_entry, connect_adapter};
use dbboard_core::{Column, DatabaseAdapter, DbError, Row, TableInfo, TableSchema};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;

/// Default number of rows returned when the caller does not specify
/// `max_rows`. Small enough that an agent's first exploratory query does
/// not haul back a whole table, large enough to be useful.
pub const DEFAULT_MAX_ROWS: usize = 200;

/// Hard ceiling on `max_rows`. A caller asking for more is silently
/// clamped to this — the read path is for reconnaissance, not bulk
/// export, and an unbounded fetch could exhaust memory on a wide table.
pub const MAX_MAX_ROWS: usize = 1000;

/// A connection as an agent is allowed to see it: the stable id, the
/// human label, and the adapter kind. Deliberately **not** the keyring
/// references or any resolved secret — those never appear in a tool
/// result (ADR-0046 Decision 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionView {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// Result of [`McpService::run_read_query`]. `truncated` tells the agent
/// the table had more rows than were returned, so it can page with a
/// tighter `WHERE`/`LIMIT` rather than assume it saw everything.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueryOutput {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub row_count: usize,
    pub truncated: bool,
}

/// Result of [`McpService::get_annotations`]: the local table/column
/// notes (ADR-0045) for one connection, filtered to the requested table
/// and/or column. Empty `tables` when the connection has no notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnotationsView {
    pub connection_id: String,
    pub tables: Vec<TableAnnotations>,
}

/// Failure modes surfaced by the tool layer. [`crate::server`] maps each
/// onto an MCP error; none of these messages embed a secret (the
/// underlying types redact URLs/tokens before they reach here).
#[derive(Debug, Error)]
pub enum ServiceError {
    /// The requested `connection_id` is not present in `connections.toml`.
    #[error("no connection with id {0:?} in the connection store")]
    ConnectionNotFound(String),

    /// Reading `connections.toml` or resolving a keyring secret failed.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// Reading `annotations.toml` failed.
    #[error(transparent)]
    Annotations(#[from] AnnotationsError),

    /// The adapter rejected the request — a non-read-only statement, a
    /// connection failure, or a query error.
    #[error(transparent)]
    Db(#[from] DbError),

    /// A `spawn_blocking` task panicked or was cancelled.
    #[error("background task failed: {0}")]
    Task(String),
}

/// The stable, agent-facing kind label for a connection.
fn kind_label(kind: &ConnectionKind) -> &'static str {
    match kind {
        ConnectionKind::Turso { .. } => "turso",
        ConnectionKind::D1 { .. } => "d1",
        ConnectionKind::Postgres { .. } => "postgres",
        ConnectionKind::Neon { .. } => "neon",
        ConnectionKind::Supabase { .. } => "supabase",
        ConnectionKind::AuroraDsql { .. } => "aurora-dsql",
        ConnectionKind::AuroraDsqlIam { .. } => "aurora-dsql-iam",
    }
}

/// Owns the config paths, the secret store, and a per-connection-id
/// adapter cache. One instance backs the whole server; it is `Send +
/// Sync` so `rmcp` can share it across concurrent tool calls.
pub struct McpService {
    config_path: PathBuf,
    annotations_path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    // Adapters are connected lazily on first use and reused thereafter —
    // reconnecting per request would be wasteful and, for Turso
    // `:memory:`, would silently open a fresh empty database each time
    // (see `dbboard_connect::connect_adapter`). A tokio `Mutex` because
    // the miss path connects across an `.await`.
    cache: Mutex<HashMap<String, Arc<dyn DatabaseAdapter>>>,
}

impl McpService {
    /// Build a service reading connections from `config_path` and
    /// annotations from `annotations_path`, resolving secrets through
    /// `secrets`.
    #[must_use]
    pub fn new(
        config_path: PathBuf,
        annotations_path: PathBuf,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            config_path,
            annotations_path,
            secrets,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Build a service using the platform's default per-user config paths
    /// (the same `connections.toml` / `annotations.toml` the desktop GUI
    /// reads).
    ///
    /// # Errors
    ///
    /// [`ServiceError::Config`] / [`ServiceError::Annotations`] if the OS
    /// reports no usable per-user config directory.
    pub fn with_default_paths(secrets: Arc<dyn SecretStore>) -> Result<Self, ServiceError> {
        let config_path = store::default_path()?;
        let annotations_path = annotations::default_annotations_path()?;
        Ok(Self::new(config_path, annotations_path, secrets))
    }

    /// List every configured connection, projected to the non-secret
    /// id/name/kind view.
    ///
    /// Read fresh from disk on every call so an agent sees connections
    /// added while the server is running, without a restart.
    ///
    /// # Errors
    ///
    /// [`ServiceError::Config`] if `connections.toml` cannot be read or
    /// parsed.
    pub async fn list_connections(&self) -> Result<Vec<ConnectionView>, ServiceError> {
        let file = self.load_connection_file().await?;
        Ok(file
            .connections
            .iter()
            .map(|entry| ConnectionView {
                id: entry.id.clone(),
                name: entry.name.clone(),
                kind: kind_label(&entry.kind).to_string(),
            })
            .collect())
    }

    /// List the tables in `connection_id`'s database.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] if the adapter's catalog read fails.
    pub async fn list_tables(&self, connection_id: &str) -> Result<Vec<TableInfo>, ServiceError> {
        let adapter = self.adapter_for(connection_id).await?;
        Ok(adapter.list_tables().await?)
    }

    /// Describe one table's columns and primary key.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] if the adapter cannot introspect the table.
    pub async fn describe_table(
        &self,
        connection_id: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<TableSchema, ServiceError> {
        let table_info = match schema {
            Some(s) if !s.is_empty() => TableInfo::qualified(s, table),
            _ => TableInfo::unqualified(table),
        };
        let adapter = self.adapter_for(connection_id).await?;
        Ok(adapter.describe_table(&table_info).await?)
    }

    /// Run a single read-only SQL statement, returning at most
    /// `max_rows` rows (default [`DEFAULT_MAX_ROWS`], clamped to
    /// [`MAX_MAX_ROWS`]) plus a `truncated` flag.
    ///
    /// Enforcement is the adapter's [`DatabaseAdapter::query_read_only`],
    /// which rejects writes, DDL, multi-statement batches, and locking
    /// reads at the engine — this layer never touches the plain `query`
    /// path.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] if the statement is not a single read-only
    /// query or the adapter fails to run it.
    pub async fn run_read_query(
        &self,
        connection_id: &str,
        sql: &str,
        max_rows: Option<usize>,
    ) -> Result<QueryOutput, ServiceError> {
        let effective = max_rows.unwrap_or(DEFAULT_MAX_ROWS).min(MAX_MAX_ROWS);
        let adapter = self.adapter_for(connection_id).await?;
        // Fetch one extra row so we can tell a full-but-exact result from
        // a genuinely truncated one, then trim back to the cap.
        let probe = effective.saturating_add(1);
        let mut result = adapter.query_read_only(sql, probe).await?;
        let truncated = result.rows.len() > effective;
        result.truncate_rows(effective);
        Ok(QueryOutput {
            row_count: result.rows.len(),
            truncated,
            columns: result.columns,
            rows: result.rows,
        })
    }

    /// Fetch the local notes for `connection_id`, filtered to `table`
    /// and/or `column` when supplied. Unknown connection or table yields
    /// an empty result rather than an error — notes are optional
    /// documentation, not schema.
    ///
    /// # Errors
    ///
    /// [`ServiceError::Annotations`] if `annotations.toml` cannot be read
    /// or parsed.
    pub async fn get_annotations(
        &self,
        connection_id: &str,
        table: Option<&str>,
        column: Option<&str>,
    ) -> Result<AnnotationsView, ServiceError> {
        let file = self.load_annotations_file().await?;
        let tables = file
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .map(|conn| {
                conn.tables
                    .iter()
                    .filter(|t| table.is_none_or(|want| t.key == want))
                    .map(|t| filter_columns(t, column))
                    .collect()
            })
            .unwrap_or_default();
        Ok(AnnotationsView {
            connection_id: connection_id.to_string(),
            tables,
        })
    }

    /// Resolve (and cache) the adapter for `connection_id`.
    ///
    /// `pub(crate)` so tests can seed the returned adapter through its
    /// write path before exercising the read-only tools against the same
    /// cached instance.
    pub(crate) async fn adapter_for(
        &self,
        connection_id: &str,
    ) -> Result<Arc<dyn DatabaseAdapter>, ServiceError> {
        let mut cache = self.cache.lock().await;
        if let Some(adapter) = cache.get(connection_id) {
            return Ok(Arc::clone(adapter));
        }

        let file = self.load_connection_file().await?;
        let entry = file
            .connections
            .into_iter()
            .find(|e| e.id == connection_id)
            .ok_or_else(|| ServiceError::ConnectionNotFound(connection_id.to_string()))?;

        // Keyring reads (and the underlying platform prompts) are
        // blocking; keep them off the async worker thread.
        let secrets = Arc::clone(&self.secrets);
        let config =
            tokio::task::spawn_blocking(move || backend_config_for_entry(&entry, secrets.as_ref()))
                .await
                .map_err(|e| ServiceError::Task(e.to_string()))??;

        let adapter = connect_adapter(config).await?;
        cache.insert(connection_id.to_string(), Arc::clone(&adapter));
        Ok(adapter)
    }

    async fn load_connection_file(&self) -> Result<store::ConnectionFile, ServiceError> {
        let path = self.config_path.clone();
        tokio::task::spawn_blocking(move || store::load_or_empty(&path))
            .await
            .map_err(|e| ServiceError::Task(e.to_string()))?
            .map_err(ServiceError::Config)
    }

    async fn load_annotations_file(&self) -> Result<annotations::AnnotationsFile, ServiceError> {
        let path = self.annotations_path.clone();
        tokio::task::spawn_blocking(move || annotations::load_or_empty(&path))
            .await
            .map_err(|e| ServiceError::Task(e.to_string()))?
            .map_err(ServiceError::Annotations)
    }
}

/// Project a table's notes down to a single column when `column` is
/// given, keeping the table-level note as surrounding context.
fn filter_columns(table: &TableAnnotations, column: Option<&str>) -> TableAnnotations {
    match column {
        None => table.clone(),
        Some(want) => TableAnnotations {
            key: table.key.clone(),
            note: table.note.clone(),
            columns: table
                .columns
                .iter()
                .filter(|c| c.name == want)
                .cloned()
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_config::annotations::AnnotationsAdmin;
    use dbboard_config::InMemorySecretStore;
    use std::path::Path;
    use tempfile::TempDir;

    /// A service pointing at a fresh temp config dir, plus the paths so a
    /// test can write the two TOML files it needs.
    struct Fixture {
        _dir: TempDir,
        service: McpService,
        config_path: PathBuf,
        annotations_path: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let config_path = dir.path().join("connections.toml");
        let annotations_path = dir.path().join("annotations.toml");
        let secrets = Arc::new(InMemorySecretStore::default());
        let service = McpService::new(config_path.clone(), annotations_path.clone(), secrets);
        Fixture {
            _dir: dir,
            service,
            config_path,
            annotations_path,
        }
    }

    fn write(path: &Path, contents: &str) {
        std::fs::write(path, contents).expect("write toml");
    }

    #[test]
    fn kind_label_covers_every_variant() {
        // A compile-time-total match plus these spot checks means a new
        // ConnectionKind cannot be added without labelling it here.
        assert_eq!(
            kind_label(&ConnectionKind::Turso { path: "x".into() }),
            "turso"
        );
        assert_eq!(
            kind_label(&ConnectionKind::Postgres {
                keyring_url_ref: "r".into()
            }),
            "postgres"
        );
        assert_eq!(
            kind_label(&ConnectionKind::AuroraDsqlIam {
                endpoint: "e".into(),
                region: "r".into(),
                database: "d".into(),
                username: "u".into(),
                access_key_id: "a".into(),
                keyring_secret_key_ref: "s".into(),
            }),
            "aurora-dsql-iam"
        );
    }

    #[tokio::test]
    async fn list_connections_projects_id_name_kind_and_leaks_no_secret_refs() {
        let fx = fixture();
        write(
            &fx.config_path,
            r#"
version = 1

[[connections]]
id   = "local"
name = "Local libSQL"
kind = "turso"
path = ":memory:"

[[connections]]
id              = "prod-pg"
name            = "Prod Postgres"
kind            = "postgres"
keyring_url_ref = "dbboard.prod-pg.url"
"#,
        );

        let views = fx.service.list_connections().await.expect("list");
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].id, "local");
        assert_eq!(views[0].kind, "turso");
        assert_eq!(views[1].id, "prod-pg");
        assert_eq!(views[1].kind, "postgres");

        // The serialized tool payload must never carry a keyring
        // reference, a URL, or a filesystem path.
        let json = serde_json::to_string(&views).expect("serialize");
        assert!(!json.contains("keyring"), "leaked a keyring ref: {json}");
        assert!(!json.contains("url"), "leaked a url field: {json}");
        assert!(!json.contains("path"), "leaked a path field: {json}");
        assert!(!json.contains(":memory:"), "leaked a path value: {json}");
    }

    #[tokio::test]
    async fn list_connections_reads_the_file_fresh_on_each_call() {
        let fx = fixture();
        // No file yet: an empty store is not an error.
        assert!(fx
            .service
            .list_connections()
            .await
            .expect("empty")
            .is_empty());

        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"a\"\nname=\"A\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        let views = fx.service.list_connections().await.expect("after write");
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "a");
    }

    #[tokio::test]
    async fn unknown_connection_id_is_a_clean_not_found() {
        let fx = fixture();
        write(&fx.config_path, "version = 1\n");
        let err = fx
            .service
            .list_tables("does-not-exist")
            .await
            .expect_err("must not found");
        assert!(matches!(err, ServiceError::ConnectionNotFound(id) if id == "does-not-exist"));
    }

    /// Seed the cached in-memory Turso adapter through its write path,
    /// then exercise the read-only tools against the same instance.
    async fn seeded_turso_fixture() -> Fixture {
        let fx = fixture();
        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        let adapter = fx.service.adapter_for("mem").await.expect("connect mem");
        adapter
            .query("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .expect("create");
        for i in 1..=5 {
            adapter
                .query(&format!(
                    "INSERT INTO items (id, name) VALUES ({i}, 'n{i}')"
                ))
                .await
                .expect("insert");
        }
        fx
    }

    #[tokio::test]
    async fn list_tables_sees_the_seeded_table() {
        let fx = seeded_turso_fixture().await;
        let tables = fx.service.list_tables("mem").await.expect("list tables");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "items");
    }

    #[tokio::test]
    async fn describe_table_returns_columns_and_primary_key() {
        let fx = seeded_turso_fixture().await;
        let schema = fx
            .service
            .describe_table("mem", None, "items")
            .await
            .expect("describe");
        assert_eq!(schema.table.name, "items");
        assert_eq!(schema.columns.len(), 2);
        assert_eq!(schema.primary_key, vec!["id".to_string()]);
    }

    #[tokio::test]
    async fn run_read_query_returns_rows() {
        let fx = seeded_turso_fixture().await;
        let out = fx
            .service
            .run_read_query("mem", "SELECT id, name FROM items ORDER BY id", None)
            .await
            .expect("query");
        assert_eq!(out.row_count, 5);
        assert!(!out.truncated);
        assert_eq!(out.columns.len(), 2);
    }

    #[tokio::test]
    async fn run_read_query_truncates_and_flags_it() {
        let fx = seeded_turso_fixture().await;
        let out = fx
            .service
            .run_read_query("mem", "SELECT id FROM items ORDER BY id", Some(2))
            .await
            .expect("query");
        assert_eq!(out.row_count, 2);
        assert!(out.truncated, "5 rows capped at 2 must flag truncated");
    }

    #[tokio::test]
    async fn run_read_query_exact_fit_is_not_flagged_truncated() {
        let fx = seeded_turso_fixture().await;
        let out = fx
            .service
            .run_read_query("mem", "SELECT id FROM items ORDER BY id", Some(5))
            .await
            .expect("query");
        assert_eq!(out.row_count, 5);
        assert!(!out.truncated, "exactly max_rows must not flag truncated");
    }

    #[tokio::test]
    async fn run_read_query_rejects_a_write() {
        let fx = seeded_turso_fixture().await;
        let err = fx
            .service
            .run_read_query("mem", "DELETE FROM items", None)
            .await
            .expect_err("write must be rejected");
        assert!(matches!(err, ServiceError::Db(_)));
        // The rows survived — the write never reached the engine.
        let out = fx
            .service
            .run_read_query("mem", "SELECT id FROM items", None)
            .await
            .expect("still there");
        assert_eq!(out.row_count, 5);
    }

    #[tokio::test]
    async fn get_annotations_returns_and_filters_notes() {
        let fx = fixture();
        {
            let mut admin =
                AnnotationsAdmin::new_with_file(fx.annotations_path.clone()).expect("open");
            admin
                .set_table_note("mem", "items", "one row per item")
                .expect("table note");
            admin
                .set_column_note("mem", "items", "name", "display name")
                .expect("col note");
            admin
                .set_column_note("mem", "items", "id", "surrogate key")
                .expect("col note 2");
        }

        // No filter: the whole table, both columns.
        let all = fx
            .service
            .get_annotations("mem", None, None)
            .await
            .expect("all");
        assert_eq!(all.connection_id, "mem");
        assert_eq!(all.tables.len(), 1);
        assert_eq!(all.tables[0].note.as_deref(), Some("one row per item"));
        assert_eq!(all.tables[0].columns.len(), 2);

        // Column filter: table note kept as context, one column only.
        let one = fx
            .service
            .get_annotations("mem", Some("items"), Some("name"))
            .await
            .expect("filtered");
        assert_eq!(one.tables[0].columns.len(), 1);
        assert_eq!(one.tables[0].columns[0].name, "name");
    }

    #[tokio::test]
    async fn get_annotations_unknown_connection_is_empty_not_error() {
        let fx = fixture();
        let out = fx
            .service
            .get_annotations("nope", None, None)
            .await
            .expect("empty ok");
        assert!(out.tables.is_empty());
    }
}
