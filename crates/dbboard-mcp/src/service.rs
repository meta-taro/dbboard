//! The tool surface, independent of the MCP wire layer.
//!
//! [`McpService`] owns the security-sensitive work — resolving a
//! `connections.toml` entry plus its keyring secret into a connected
//! adapter, and running the operations exposed to an external agent
//! (ADR-0046 Decision 5, ADR-0053, ADR-0054, ADR-0087). It knows nothing
//! about `rmcp`, JSON-RPC, or stdio: [`crate::server`] wraps each method as
//! a tool and translates errors onto the MCP envelope. Keeping the logic
//! here means it is testable against a real (in-memory) adapter with no
//! transport.
//!
//! Three invariants this layer enforces:
//!
//! - **Secrets never leave.** [`list_connections`](McpService::list_connections)
//!   projects each entry to id/name/kind only; the keyring references and
//!   the resolved URLs/tokens are never serialized into a tool result.
//! - **Reading is read-only.** [`run_read_query`](McpService::run_read_query)
//!   goes through [`DatabaseAdapter::query_read_only`], enforced at the
//!   engine (Postgres `BEGIN READ ONLY`, libSQL `PRAGMA query_only`, D1 AST
//!   classification) rather than by inspecting the SQL.
//! - **Writing is off until a human turns it on, and never opens
//!   everything.** [`run_write`](McpService::run_write) needs `mcp_write` on
//!   the connection *and* an allowed statement; privilege and role changes,
//!   `TRUNCATE` and `DROP` are refused on every connection with no setting
//!   that enables them (ADR-0087).
//!
//! The desktop app also uses this service as its shared data-access layer
//! (it owns the adapter cache and connection resolution). Some methods
//! exist only for it and are **not** wrapped as MCP tools —
//! [`apply_row_update`](McpService::apply_row_update) (inline cell editing,
//! ADR-0042 / ADR-0062) and the restore pair, which runs a script verbatim
//! and so cannot honour the closed list. What an agent can reach is a
//! property of the exposed tool set, not of every method on the struct.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbboard_config::annotations::{self, AnnotationsError, TableAnnotations};
use dbboard_config::store::{self, ConnectionKind};
use dbboard_config::{ConfigError, SecretStore};
use dbboard_connect::{backend_config_for_entry, connect_adapter};
use dbboard_core::{
    build_update_sql, classify_write, dialect_for_adapter_id, plan_dump as core_plan_dump,
    plan_restore as core_plan_restore, run_dump as core_run_dump, run_restore as core_run_restore,
    Column, ColumnInfo, DatabaseAdapter, DbError, DumpControl, DumpError, DumpOutcome, DumpPlan,
    DumpProgress, DumpResult, DumpSink, ForeignKey, RestoreControl, RestoreOptions, RestoreOutcome,
    RestorePlan, Row, TableInfo, TableSchema, UpdatePlan, WriteBackError, WritePolicyViolation,
    WriteStatement,
};
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

/// Hard ceiling on the number of table matches [`McpService::search_schema`]
/// returns. A deliberately-broad pattern (`"id"`, `"a"`) on a large schema
/// would otherwise walk every table and return the whole catalog in one
/// blob; the search stops here and flags `truncated`, mirroring
/// `run_read_query`'s row cap. Reconnaissance, not export.
pub const MAX_SCHEMA_MATCHES: usize = 200;

/// Hard ceiling on the number of relationship edges
/// [`McpService::list_relationships`] returns. A wide schema can declare
/// far more foreign keys than it has tables; the walk stops here and flags
/// `truncated` rather than return an unbounded blob. Reconnaissance, not
/// export — an agent that hits the cap should filter to one table.
pub const MAX_RELATIONSHIPS: usize = 500;

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

/// Result of [`McpService::run_write`]. `statement` is `"data"` or
/// `"schema"` — the category the policy allowed it under, so an agent can
/// see that its `ALTER` was understood as schema and not silently treated
/// as something else. `rows_affected` is the engine's own count; DDL
/// generally reports zero, which is not a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteOutput {
    pub statement: String,
    pub rows_affected: u64,
}

/// Result of [`McpService::dump_to_file`].
///
/// Failures and truncations are reported as bare table names rather than
/// with the engine's message: a dump that skipped a table is a fact the
/// agent must see, but the engine's text is the one place a backup could
/// leak connection detail into a transcript, and the agent can reproduce
/// the error itself with `run_read_query` if it needs the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DumpFileOutcome {
    pub path: String,
    pub tables_dumped: usize,
    pub rows_written: u64,
    pub bytes_written: u64,
    /// Tables the engine refused to read. The dump is still valid SQL; it
    /// is simply missing these.
    pub failed_tables: Vec<String>,
    /// Tables whose data was cut short (keyless tables larger than one
    /// page cannot be keyset-paged).
    pub truncated_tables: Vec<String>,
    /// False when the file is valid SQL but does not cover the whole
    /// database, so a caller that only reads one field reads the one that
    /// matters.
    pub complete: bool,
}

/// A [`DumpSink`] writing to a newly-created file, counting what it wrote.
///
/// Mirrors the desktop app's sink (`apps/desktop/src-tauri/src/dump.rs`),
/// with one addition: the byte count. The desktop shows progress in rows
/// because a human watches it run; an agent gets no progress at all and a
/// size is the only cheap evidence the file is not empty.
struct FileSink {
    writer: BufWriter<File>,
    bytes_written: u64,
}

impl FileSink {
    /// Create `path`, failing if it already exists.
    ///
    /// The exclusivity is `create_new`, not a prior `exists()` check: the
    /// check-then-create version can still clobber a file that appeared in
    /// between, and the whole point of the destination rules is that a dump
    /// never destroys anything.
    fn create(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            bytes_written: 0,
        })
    }

    /// Flush and return the byte count. Called explicitly rather than left
    /// to `Drop`, because a `BufWriter` dropped with a full buffer discards
    /// the write error and would report a truncated dump as a complete one.
    fn finish(mut self) -> std::io::Result<u64> {
        self.writer.flush()?;
        Ok(self.bytes_written)
    }
}

impl DumpSink for FileSink {
    fn write_str(&mut self, chunk: &str) -> DumpResult<()> {
        self.writer
            .write_all(chunk.as_bytes())
            .map_err(|e| DumpError::Sink(e.to_string()))?;
        self.bytes_written += chunk.len() as u64;
        Ok(())
    }
}

/// A [`DumpControl`] for a dump nobody is watching: it discards progress
/// and never cancels. An MCP tool call has no channel to report on and no
/// second call that could interrupt the first.
struct UnattendedDump;

impl DumpControl for UnattendedDump {
    fn report(&self, _progress: &DumpProgress) {}

    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Check that `path` names a file a dump may create, returning it as a
/// string for the outcome.
///
/// The three rules exist because the agent, not the operator, chose this
/// path:
/// - **Absolute.** A relative path resolves against the server process's
///   working directory, which the agent cannot see and the operator did not
///   pick.
/// - **Parent must exist.** Creating directories would let a typo scatter
///   empty trees across the disk; an existing parent is the operator's
///   consent to write somewhere.
/// - **Must not exist.** The file is the only thing a dump could destroy.
fn check_dump_destination(path: &Path) -> Result<String, ServiceError> {
    if !path.is_absolute() {
        return Err(ServiceError::InvalidRequest(
            "output_path must be an absolute path".to_owned(),
        ));
    }
    if path.exists() {
        return Err(ServiceError::InvalidRequest(
            "output_path already exists; dumps never overwrite — choose another name".to_owned(),
        ));
    }
    match path.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => {
            return Err(ServiceError::InvalidRequest(
                "the directory of output_path does not exist; create it first".to_owned(),
            ))
        }
    }
    Ok(path.display().to_string())
}

/// One table returned by [`McpService::search_schema`]: the table itself,
/// whether its *name* matched the pattern, and the columns whose name
/// matched. A table-name-only hit carries empty `matched_columns` — the
/// flag is the signal, and the agent can `describe_table` for the full
/// column list rather than have every column echoed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaMatch {
    pub table: TableInfo,
    pub table_name_matched: bool,
    pub matched_columns: Vec<ColumnInfo>,
}

/// Result of [`McpService::search_schema`]: every table in the connection
/// whose name — or one of whose column names — contains the pattern.
/// `truncated` is set when the match cap ([`MAX_SCHEMA_MATCHES`]) was hit
/// and further tables were left unexamined, telling the agent to narrow
/// the pattern rather than assume it saw the whole schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaSearchView {
    pub connection_id: String,
    pub pattern: String,
    pub matches: Vec<SchemaMatch>,
    pub truncated: bool,
}

/// One foreign-key relationship as a directed edge, flattened from a
/// [`ForeignKey`] for [`McpService::list_relationships`]: the child
/// (`from`) table's columns point at the parent (`to`) table's columns.
/// `from_columns` and `to_columns` are aligned 1:1 in key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Relationship {
    pub from_table: TableInfo,
    pub from_columns: Vec<String>,
    pub to_table: TableInfo,
    pub to_columns: Vec<String>,
    pub constraint_name: Option<String>,
}

/// Result of [`McpService::list_relationships`]: the foreign-key edges of
/// a connection, optionally filtered to those touching one table (either
/// endpoint). `table` echoes the applied filter; `truncated` is set when
/// the edge cap ([`MAX_RELATIONSHIPS`]) was hit and further tables were
/// left unexamined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationshipView {
    pub connection_id: String,
    pub table: Option<String>,
    pub relationships: Vec<Relationship>,
    pub truncated: bool,
    /// Tables the sweep could list but not introspect — a denied `PRAGMA`,
    /// a permission-restricted table, one dropped mid-sweep. Their edges are
    /// missing from `relationships`, so this is reported rather than swallowed:
    /// "no foreign keys" and "we could not look" are different answers.
    pub unreadable_tables: Vec<TableInfo>,
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

    /// The caller's arguments were malformed (e.g. a blank search pattern).
    /// Distinct from a `Db` rejection: nothing reached the engine.
    #[error("{0}")]
    InvalidRequest(String),

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

    /// The core write-back layer refused to build the `UPDATE` (no edits,
    /// empty key, or a blob identity value). Desktop write path only.
    #[error(transparent)]
    WriteBack(#[from] WriteBackError),

    /// The connection's adapter has no known SQL dialect, so no write SQL
    /// can be built for it. Desktop write path only.
    #[error("adapter {0:?} has no known SQL dialect for editing")]
    NotEditable(String),

    /// The connection exists, but the operator has not set `mcp_write` on
    /// it, so the MCP write tools will not touch it (ADR-0087). Says where
    /// the switch is, because an agent cannot flip it and should ask rather
    /// than retry.
    #[error(
        "connection {0:?} is not enabled for writes over MCP; \
         a human must set mcp_write = true on it in connections.toml"
    )]
    WriteNotEnabled(String),

    /// The write policy refused the statement (ADR-0087). Carries
    /// [`WritePolicyViolation::is_permanent`], which tells an agent whether
    /// rephrasing could ever help.
    #[error(transparent)]
    WriteRefused(#[from] WritePolicyViolation),

    /// The connection's adapter has no known SQL dialect, so no dump can be
    /// produced for it. Desktop dump path only (ADR-0049).
    #[error("adapter {0:?} has no known SQL dialect to dump")]
    NotDumpable(String),

    /// Writing the dump's output file failed (a full disk, a revoked path).
    /// Fatal to a dump — a backup that cannot be written is worthless.
    /// Desktop dump path only.
    #[error("dump output failed: {0}")]
    Dump(String),

    /// The connection's adapter has no known SQL dialect, so an incoming
    /// `.sql` script cannot be classified for restore. Desktop restore path
    /// only (ADR-0051).
    #[error("adapter {0:?} has no known SQL dialect to restore into")]
    NotRestorable(String),

    /// A restore was refused or failed as a whole: a non-empty target left
    /// unconfirmed, an adapter that cannot execute writes, or an atomic
    /// batch that rolled back. Per-statement failures on the non-atomic path
    /// are non-fatal and travel in the outcome instead. Desktop restore path
    /// only.
    #[error("restore failed: {0}")]
    Restore(String),

    /// A `spawn_blocking` task panicked or was cancelled.
    #[error("background task failed: {0}")]
    Task(String),
}

/// Flatten one [`ForeignKey`] on `from_table` into a directed edge.
fn relationship_from_fk(from_table: &TableInfo, fk: ForeignKey) -> Relationship {
    Relationship {
        from_table: from_table.clone(),
        from_columns: fk.columns,
        to_table: fk.referenced_table,
        to_columns: fk.referenced_columns,
        constraint_name: fk.constraint_name,
    }
}

/// Does `edge` touch the table named `want` (already lower-cased) at
/// either endpoint? Matches the bare name and the `schema.name` key, so a
/// filter of `orders` finds `public.orders` too.
fn edge_touches(edge: &Relationship, want: &str) -> bool {
    table_matches(&edge.from_table, want) || table_matches(&edge.to_table, want)
}

fn table_matches(table: &TableInfo, want: &str) -> bool {
    if table.name.to_lowercase() == want {
        return true;
    }
    match &table.schema {
        Some(schema) => format!("{}.{}", schema.to_lowercase(), table.name.to_lowercase()) == want,
        None => false,
    }
}

/// The stable, agent-facing kind label for a connection.
fn kind_label(kind: &ConnectionKind) -> &'static str {
    match kind {
        ConnectionKind::Turso { .. } => "turso",
        ConnectionKind::D1 { .. } => "d1",
        ConnectionKind::Postgres { .. } => "postgres",
        ConnectionKind::MySql { .. } => "mysql",
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

    /// Run one write statement against `connection_id` on an agent's behalf
    /// (ADR-0087).
    ///
    /// Two gates, in this order:
    ///
    /// 1. The connection's `mcp_write` flag, read fresh from
    ///    `connections.toml` on every call. The adapter is cached but the
    ///    gate is not, so revoking it takes effect on the next statement
    ///    rather than at the next restart of a server an agent is holding
    ///    open.
    /// 2. [`classify_write`], which permits DML and table/view/index DDL and
    ///    refuses everything else — including reads, which would otherwise
    ///    be an uncapped [`run_read_query`](Self::run_read_query).
    ///
    /// Privilege changes, principal changes, `TRUNCATE` and `DROP` are
    /// refused whatever the flag says; no configuration opens them.
    ///
    /// # Errors
    ///
    /// - [`ServiceError::ConnectionNotFound`] for an unknown id.
    /// - [`ServiceError::WriteNotEnabled`] if the operator has not opted the
    ///   connection in.
    /// - [`ServiceError::NotEditable`] if the adapter has no known dialect,
    ///   so the statement cannot be classified at all.
    /// - [`ServiceError::WriteRefused`] if the policy refuses it.
    /// - [`ServiceError::Db`] if the engine rejects or fails it.
    pub async fn run_write(
        &self,
        connection_id: &str,
        sql: &str,
    ) -> Result<WriteOutput, ServiceError> {
        self.require_mcp_write(connection_id).await?;
        let adapter = self.adapter_for(connection_id).await?;
        let dialect = dialect_for_adapter_id(adapter.id())
            .ok_or_else(|| ServiceError::NotEditable(adapter.id().to_string()))?;
        let statement = classify_write(sql, dialect)?;
        let rows_affected = adapter.execute(sql).await?;
        Ok(WriteOutput {
            statement: match statement {
                WriteStatement::Data => "data".to_owned(),
                WriteStatement::Schema => "schema".to_owned(),
            },
            rows_affected,
        })
    }

    /// Dump `connection_id` to a new file at `output_path` on an agent's
    /// behalf (ADR-0087), so it can take a backup before it changes
    /// anything.
    ///
    /// Deliberately **outside** the `mcp_write` gate. A dump reads the
    /// database and writes somewhere else; requiring the write flag would
    /// mean the safest thing an agent can do costs the same permission as
    /// the least safe, which is how a flag ends up permanently on.
    ///
    /// The file is the one thing here an agent could destroy, so it is not
    /// allowed to: the path must be absolute, its parent must already
    /// exist, and it must not. There is no overwrite and no `mkdir -p`.
    ///
    /// Runs to completion with no progress reporting or cancellation —
    /// there is no channel to report on. A caller that needs either wants
    /// the desktop app.
    ///
    /// # Errors
    ///
    /// - [`ServiceError::ConnectionNotFound`] for an unknown id.
    /// - [`ServiceError::InvalidRequest`] if the path is relative, occupied,
    ///   or in a directory that does not exist.
    /// - [`ServiceError::NotDumpable`] if the adapter has no known dialect.
    /// - [`ServiceError::Dump`] if writing the file fails.
    pub async fn dump_to_file(
        &self,
        connection_id: &str,
        output_path: &Path,
    ) -> Result<DumpFileOutcome, ServiceError> {
        let path = check_dump_destination(output_path)?;
        let plan = self.plan_dump(connection_id).await?;

        let mut sink = FileSink::create(output_path).map_err(|e| {
            // Name the failure, not the path — the agent supplied the path
            // and a bare reason is enough to act on.
            ServiceError::Dump(format!("could not create the output file: {e}"))
        })?;
        let outcome = self
            .run_dump(connection_id, &plan, &mut sink, &UnattendedDump)
            .await?;
        let bytes_written = sink
            .finish()
            .map_err(|e| ServiceError::Dump(format!("could not flush the output file: {e}")))?;

        let failed_tables: Vec<String> = outcome.failures.iter().map(|f| f.table.clone()).collect();
        let truncated_tables: Vec<String> = outcome
            .truncations
            .iter()
            .map(|t| t.table.clone())
            .collect();
        Ok(DumpFileOutcome {
            path,
            tables_dumped: outcome.tables_dumped,
            rows_written: outcome.rows_written,
            bytes_written,
            complete: failed_tables.is_empty() && truncated_tables.is_empty(),
            failed_tables,
            truncated_tables,
        })
    }

    /// Fail unless the operator has set `mcp_write` on `connection_id`.
    ///
    /// Deliberately reads the file rather than any cached view — see
    /// [`run_write`](Self::run_write) for why the gate is not cached.
    async fn require_mcp_write(&self, connection_id: &str) -> Result<(), ServiceError> {
        let file = self.load_connection_file().await?;
        let entry = file
            .connections
            .iter()
            .find(|e| e.id == connection_id)
            .ok_or_else(|| ServiceError::ConnectionNotFound(connection_id.to_string()))?;
        if entry.mcp_write {
            Ok(())
        } else {
            Err(ServiceError::WriteNotEnabled(connection_id.to_string()))
        }
    }

    /// Apply one single-row `UPDATE` to `connection_id` (inline cell
    /// editing, ADR-0042). **Desktop write path** — deliberately not an MCP
    /// tool. Builds the fully-escaped statement from `plan` with the
    /// adapter's dialect (never string-concatenated user text), runs it via
    /// the adapter's `execute`, and returns the engine's affected-row count
    /// so the caller can confirm exactly one row changed.
    ///
    /// The `WHERE` key comes from the row's declared primary key, so a
    /// well-formed plan can only ever match zero or one row; the caller
    /// (the Tauri command) treats anything other than one as a conflict and
    /// re-reads, rather than this layer guessing.
    ///
    /// # Errors
    ///
    /// - [`ServiceError::ConnectionNotFound`] for an unknown id.
    /// - [`ServiceError::NotEditable`] if the adapter's dialect is unknown.
    /// - [`ServiceError::WriteBack`] if the core refuses the plan (no
    ///   edits, empty key, or a blob identity value).
    /// - [`ServiceError::Db`] if the engine rejects or fails the `UPDATE`.
    pub async fn apply_row_update(
        &self,
        connection_id: &str,
        plan: &UpdatePlan,
    ) -> Result<u64, ServiceError> {
        let adapter = self.adapter_for(connection_id).await?;
        let dialect = dialect_for_adapter_id(adapter.id())
            .ok_or_else(|| ServiceError::NotEditable(adapter.id().to_string()))?;
        let sql = build_update_sql(plan, dialect)?;
        Ok(adapter.execute(&sql).await?)
    }

    /// Preflight a logical dump of `connection_id` (ADR-0049): list its
    /// tables and `COUNT(*)` each, producing the [`DumpPlan`] the desktop
    /// uses to size progress and warn on a large database. Reads only.
    ///
    /// # Errors
    ///
    /// [`ServiceError::NotDumpable`] if the adapter has no known SQL
    /// dialect; otherwise whatever the connection/`list_tables` surfaces.
    pub async fn plan_dump(&self, connection_id: &str) -> Result<DumpPlan, ServiceError> {
        let adapter = self.adapter_for(connection_id).await?;
        let dialect = dialect_for_adapter_id(adapter.id())
            .ok_or_else(|| ServiceError::NotDumpable(adapter.id().to_string()))?;
        Ok(core_plan_dump(adapter.as_ref(), dialect).await?)
    }

    /// Run a whole-connection logical dump described by `plan`, writing SQL
    /// text to `sink` and reporting progress/cancellation through `control`
    /// (ADR-0049). Reads the database only; the sole write is to the
    /// caller-supplied output sink (a file on the desktop).
    ///
    /// Returns the [`DumpOutcome`] — including any per-table failures and
    /// truncations — unless the sink itself fails, which is fatal.
    ///
    /// Takes a caller-supplied sink and control because the desktop drives
    /// it with a file and a live progress channel. The MCP tool goes
    /// through [`dump_to_file`](Self::dump_to_file) instead, which supplies
    /// both itself: an agent may name a destination, but not hand this
    /// method somewhere arbitrary to write.
    ///
    /// # Errors
    ///
    /// [`ServiceError::NotDumpable`] if the adapter has no known SQL
    /// dialect; [`ServiceError::Dump`] if writing to `sink` fails.
    pub async fn run_dump(
        &self,
        connection_id: &str,
        plan: &DumpPlan,
        sink: &mut dyn DumpSink,
        control: &dyn DumpControl,
    ) -> Result<DumpOutcome, ServiceError> {
        let adapter = self.adapter_for(connection_id).await?;
        let dialect = dialect_for_adapter_id(adapter.id())
            .ok_or_else(|| ServiceError::NotDumpable(adapter.id().to_string()))?;
        core_run_dump(adapter.as_ref(), dialect, plan, sink, control)
            .await
            .map_err(|e| ServiceError::Dump(e.to_string()))
    }

    /// Preflight a logical restore of `script` into `connection_id`
    /// (ADR-0051): classify the script under the connection's dialect and
    /// list the target's existing tables, producing the [`RestorePlan`] the
    /// desktop uses to size progress and decide whether the empty-target
    /// safety gate needs a typed confirmation. Reads only.
    ///
    /// # Errors
    ///
    /// [`ServiceError::NotRestorable`] if the adapter has no known SQL
    /// dialect; otherwise whatever the connection/`list_tables` surfaces.
    pub async fn plan_restore(
        &self,
        connection_id: &str,
        script: &str,
    ) -> Result<RestorePlan, ServiceError> {
        let adapter = self.adapter_for(connection_id).await?;
        let dialect = dialect_for_adapter_id(adapter.id())
            .ok_or_else(|| ServiceError::NotRestorable(adapter.id().to_string()))?;
        Ok(core_plan_restore(adapter.as_ref(), dialect, script).await?)
    }

    /// Apply a preflighted logical restore to `connection_id`, reporting
    /// progress/cancellation through `control` (ADR-0051). Writes to the
    /// target database only. The whole script runs as one atomic batch on
    /// engines that support it, or statement-by-statement (honouring
    /// `options.on_error`) on those that do not (Cloudflare D1).
    ///
    /// Like [`apply_row_update`](Self::apply_row_update) and the dump
    /// methods, this is a desktop-only method and is deliberately **not**
    /// exposed as an MCP tool.
    ///
    /// # Errors
    ///
    /// [`ServiceError::Restore`] if the whole run is refused or fails — a
    /// non-empty target without `options.confirmed`, an adapter that cannot
    /// execute writes, or an atomic batch that rolled back. Per-statement
    /// failures on the non-atomic path are non-fatal and land in the
    /// returned [`RestoreOutcome`] instead.
    pub async fn run_restore(
        &self,
        connection_id: &str,
        plan: &RestorePlan,
        options: RestoreOptions,
        control: &dyn RestoreControl,
    ) -> Result<RestoreOutcome, ServiceError> {
        let adapter = self.adapter_for(connection_id).await?;
        core_run_restore(adapter.as_ref(), plan, options, control)
            .await
            .map_err(|e| ServiceError::Restore(e.to_string()))
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

    /// Find the tables and columns in `connection_id` whose name contains
    /// `pattern` (case-insensitive substring). Collapses the common
    /// `list_tables` + N×`describe_table` exploration an agent otherwise
    /// runs by hand (ADR-0053).
    ///
    /// Composed from the existing read-only introspection primitives — no
    /// `query` path, no secret ever serialized.
    ///
    /// # Errors
    ///
    /// [`ServiceError::InvalidRequest`] if `pattern` is blank (a blank
    /// needle would match the entire catalog — use `list_tables` for that).
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] if the adapter's catalog read fails.
    pub async fn search_schema(
        &self,
        connection_id: &str,
        pattern: &str,
    ) -> Result<SchemaSearchView, ServiceError> {
        let needle = pattern.trim().to_lowercase();
        if needle.is_empty() {
            return Err(ServiceError::InvalidRequest(
                "search pattern must not be blank".to_string(),
            ));
        }

        let adapter = self.adapter_for(connection_id).await?;
        let tables = adapter.list_tables().await?;

        let cap = tables.len().min(MAX_SCHEMA_MATCHES);
        let mut matches = Vec::with_capacity(cap);
        let mut truncated = false;
        for table in tables {
            // Stop once the cap is hit: further tables are left unexamined
            // and the agent is told to narrow, rather than paying N more
            // `describe_table` calls to build an oversized blob.
            if matches.len() >= MAX_SCHEMA_MATCHES {
                truncated = true;
                break;
            }
            let table_name_matched = table.name.to_lowercase().contains(&needle);
            let schema = adapter.describe_table(&table).await?;
            let matched_columns: Vec<ColumnInfo> = schema
                .columns
                .into_iter()
                .filter(|c| c.name.to_lowercase().contains(&needle))
                .collect();
            if table_name_matched || !matched_columns.is_empty() {
                matches.push(SchemaMatch {
                    table,
                    table_name_matched,
                    matched_columns,
                });
            }
        }

        Ok(SchemaSearchView {
            connection_id: connection_id.to_string(),
            pattern: pattern.to_string(),
            matches,
            truncated,
        })
    }

    /// Discover the foreign-key relationships in `connection_id` (ADR-0054):
    /// the directed edges of the schema, optionally filtered to those
    /// touching `table_filter` at *either* endpoint — so one call answers
    /// both "what does `orders` reference?" and "what references `orders`?".
    ///
    /// Composed from [`DatabaseAdapter::list_tables`] +
    /// [`DatabaseAdapter::foreign_keys`] — no `query` path, no secret ever
    /// serialized. A blank filter is treated as no filter.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] if the adapter cannot list tables or introspect
    /// a table's foreign keys.
    pub async fn list_relationships(
        &self,
        connection_id: &str,
        table_filter: Option<&str>,
    ) -> Result<RelationshipView, ServiceError> {
        // A blank filter means "no filter" — the tool takes an optional
        // string and an agent passing "" should not silently match nothing.
        let filter = table_filter
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);

        let adapter = self.adapter_for(connection_id).await?;
        let tables = adapter.list_tables().await?;

        let mut relationships = Vec::new();
        let mut unreadable_tables = Vec::new();
        let mut truncated = false;
        'outer: for table in &tables {
            if relationships.len() >= MAX_RELATIONSHIPS {
                truncated = true;
                break;
            }
            // One unreadable table must not take the whole schema with it.
            // Cloudflare's reserved `_cf_%` tables are listed by
            // `sqlite_master` but denied by the Workers authorizer, so this
            // sweep used to abort with `SQLITE_AUTH` and blank the structure
            // view for every *readable* table in the database. The same shape
            // shows up wherever a listed table is not introspectable: a
            // revoked grant, a table dropped between the list and the sweep.
            let fks = match adapter.foreign_keys(table).await {
                Ok(fks) => fks,
                Err(e) => {
                    tracing::debug!(table = %table.name, error = %e, "skipping unreadable table");
                    unreadable_tables.push(table.clone());
                    continue;
                }
            };
            for fk in fks {
                let edge = relationship_from_fk(table, fk);
                // Keep an edge only if it touches the requested table at
                // either endpoint (a relationship is inherently two-sided).
                if filter
                    .as_deref()
                    .is_some_and(|want| !edge_touches(&edge, want))
                {
                    continue;
                }
                relationships.push(edge);
                if relationships.len() >= MAX_RELATIONSHIPS {
                    truncated = true;
                    break 'outer;
                }
            }
        }

        Ok(RelationshipView {
            connection_id: connection_id.to_string(),
            table: filter,
            relationships,
            truncated,
            unreadable_tables,
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

    /// Evict any cached adapter for `connection_id`, forcing the next
    /// access to rebuild it from the current config and keyring.
    ///
    /// Call this after a connection's credentials change or it is
    /// removed, so a stale adapter (old password, or one pointing at a
    /// now-deleted entry) is never handed back. A miss is a no-op.
    pub async fn invalidate(&self, connection_id: &str) {
        self.cache.lock().await.remove(connection_id);
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
    use dbboard_core::{
        Capabilities, DbResult, ForeignKey as CoreForeignKey, QueryResult as CoreQueryResult,
    };
    use std::path::Path;
    use tempfile::TempDir;

    /// A service pointing at a fresh temp config dir, plus the paths so a
    /// test can write the two TOML files it needs.
    struct Fixture {
        /// Held for its `Drop` — the temp tree must outlive the service. The
        /// dump tests also name files inside it.
        dir: TempDir,
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
            dir,
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
        seeded_turso_fixture_with_write(false).await
    }

    /// The same fixture, with the connection's `mcp_write` gate set to
    /// `writable` so the write tools can be exercised either side of it.
    async fn seeded_turso_fixture_with_write(writable: bool) -> Fixture {
        let fx = fixture();
        let gate = if writable { "mcp_write = true\n" } else { "" };
        write(
            &fx.config_path,
            &format!(
                "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n{gate}"
            ),
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
    async fn invalidate_drops_the_cached_adapter() {
        // The seeded fixture holds a cached in-memory adapter with one table.
        let fx = seeded_turso_fixture().await;
        assert_eq!(
            fx.service.list_tables("mem").await.expect("seeded").len(),
            1
        );
        // After invalidation the next access rebuilds a fresh `:memory:`
        // adapter — the seeded table is gone, proving the stale instance was
        // evicted rather than reused.
        fx.service.invalidate("mem").await;
        assert!(fx
            .service
            .list_tables("mem")
            .await
            .expect("rebuilt")
            .is_empty());
    }

    #[tokio::test]
    async fn list_tables_sees_the_seeded_table() {
        let fx = seeded_turso_fixture().await;
        let tables = fx.service.list_tables("mem").await.expect("list tables");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "items");
    }

    // ---- apply_row_update (desktop write path, ADR-0042) ----------------

    use dbboard_core::{CellValue, RowKey, Value};

    fn update_name(id: i64, new: CellValue) -> UpdatePlan {
        UpdatePlan {
            table: TableInfo::unqualified("items"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(id))]),
            edits: vec![("name".to_owned(), new)],
        }
    }

    /// Read one cell back through the read-only path so the write is
    /// confirmed against the same cached adapter.
    async fn name_of(fx: &Fixture, id: i64) -> Option<Value> {
        let out = fx
            .service
            .run_read_query(
                "mem",
                &format!("SELECT name FROM items WHERE id = {id}"),
                None,
            )
            .await
            .expect("read back");
        out.rows.first().map(|r| r.values()[0].clone())
    }

    #[tokio::test]
    async fn apply_row_update_writes_exactly_one_row_and_reports_it() {
        let fx = seeded_turso_fixture().await;
        let affected = fx
            .service
            .apply_row_update(
                "mem",
                &update_name(3, CellValue::Text("renamed".to_owned())),
            )
            .await
            .expect("update");
        assert_eq!(affected, 1);
        assert_eq!(
            name_of(&fx, 3).await,
            Some(Value::Text("renamed".to_owned()))
        );
        // A neighbouring row is untouched — the PK `WHERE` is exact.
        assert_eq!(name_of(&fx, 2).await, Some(Value::Text("n2".to_owned())));
    }

    #[tokio::test]
    async fn apply_row_update_can_clear_a_cell_to_null() {
        let fx = seeded_turso_fixture().await;
        let affected = fx
            .service
            .apply_row_update("mem", &update_name(2, CellValue::Null))
            .await
            .expect("update to null");
        assert_eq!(affected, 1);
        assert_eq!(name_of(&fx, 2).await, Some(Value::Null));
    }

    #[tokio::test]
    async fn apply_row_update_reports_zero_when_the_key_matches_no_row() {
        let fx = seeded_turso_fixture().await;
        // No row has id 999: a well-formed UPDATE simply affects nothing.
        // The caller (Tauri command) turns a non-1 count into a conflict.
        let affected = fx
            .service
            .apply_row_update("mem", &update_name(999, CellValue::Text("x".to_owned())))
            .await
            .expect("no-op update");
        assert_eq!(affected, 0);
    }

    #[tokio::test]
    async fn apply_row_update_surfaces_a_write_back_refusal() {
        let fx = seeded_turso_fixture().await;
        // A blob identity value can't be a safe WHERE key — the core
        // refuses before any SQL reaches the engine.
        let plan = UpdatePlan {
            table: TableInfo::unqualified("items"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Blob(vec![1, 2]))]),
            edits: vec![("name".to_owned(), CellValue::Text("x".to_owned()))],
        };
        let err = fx
            .service
            .apply_row_update("mem", &plan)
            .await
            .expect_err("blob key refused");
        assert!(matches!(err, ServiceError::WriteBack(_)), "got {err:?}");
    }

    // ---- plan_dump / run_dump (desktop backup path, ADR-0049) -----------

    /// Collect dump SQL into memory so a test can assert on its content
    /// without touching the filesystem.
    #[derive(Default)]
    struct VecSink(String);

    impl dbboard_core::DumpSink for VecSink {
        fn write_str(&mut self, s: &str) -> Result<(), dbboard_core::DumpError> {
            self.0.push_str(s);
            Ok(())
        }
    }

    /// A `DumpControl` that never cancels and discards progress — enough to
    /// drive `run_dump` in a unit test.
    struct NoopControl;

    impl dbboard_core::DumpControl for NoopControl {
        fn report(&self, _progress: &dbboard_core::DumpProgress) {}
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn plan_dump_counts_the_seeded_rows() {
        let fx = seeded_turso_fixture().await;
        let plan = fx.service.plan_dump("mem").await.expect("plan");
        assert_eq!(plan.tables.len(), 1, "one seeded table");
        assert_eq!(plan.tables[0].table.name, "items");
        assert_eq!(plan.total_rows(), 5, "5 seeded rows");
    }

    #[tokio::test]
    async fn plan_dump_rejects_an_unknown_connection() {
        let fx = seeded_turso_fixture().await;
        let err = fx.service.plan_dump("nope").await.expect_err("unknown id");
        assert!(
            matches!(err, ServiceError::ConnectionNotFound(_)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn run_dump_emits_data_inserts_and_reports_the_table() {
        let fx = seeded_turso_fixture().await;
        let plan = fx.service.plan_dump("mem").await.expect("plan");
        let mut sink = VecSink::default();
        let outcome = fx
            .service
            .run_dump("mem", &plan, &mut sink, &NoopControl)
            .await
            .expect("dump");

        assert_eq!(outcome.tables_dumped, 1);
        assert_eq!(outcome.rows_written, 5);
        assert!(!outcome.cancelled);
        assert!(outcome.failures.is_empty(), "no per-table failures");

        let sql = sink.0;
        // The SQLite/Turso dump is data-only by design (ADR-0049): no
        // CREATE TABLE, just the INSERTs, under a dialect header comment.
        assert!(
            !sql.contains("CREATE TABLE"),
            "sqlite dump must not emit DDL, got:\n{sql}"
        );
        assert!(
            sql.contains("dbboard logical dump"),
            "missing header:\n{sql}"
        );
        assert!(
            sql.contains("INSERT INTO") && sql.contains("items"),
            "expected inserts for items, got:\n{sql}"
        );
        // Every seeded value must appear in the dumped inserts.
        for i in 1..=5 {
            assert!(sql.contains(&format!("'n{i}'")), "missing n{i} in:\n{sql}");
        }
    }

    // ---- plan_restore / run_restore (desktop import path, ADR-0051) -----

    /// A `RestoreControl` that never cancels and discards progress — enough
    /// to drive `run_restore` in a unit test.
    struct NoopRestoreControl;

    impl dbboard_core::RestoreControl for NoopRestoreControl {
        fn report(&self, _progress: &dbboard_core::RestoreProgress) {}
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    /// A fresh, empty in-memory Turso connection — the unconfirmed-safe
    /// restore target. Establishing the adapter here caches it, so a later
    /// `run_restore` and the read-back query hit the same `:memory:` db.
    async fn empty_turso_fixture() -> Fixture {
        let fx = fixture();
        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        fx.service.adapter_for("mem").await.expect("connect mem");
        fx
    }

    const RESTORE_SCRIPT: &str = "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); \
         INSERT INTO items (id, name) VALUES (1, 'a'); \
         INSERT INTO items (id, name) VALUES (2, 'b')";

    #[tokio::test]
    async fn plan_restore_classifies_and_sees_an_empty_target() {
        let fx = empty_turso_fixture().await;
        let plan = fx
            .service
            .plan_restore("mem", RESTORE_SCRIPT)
            .await
            .expect("plan");
        assert!(plan.is_target_empty(), "a fresh :memory: db has no tables");
        assert_eq!(plan.runnable_count(), 3, "one CREATE + two INSERTs");
    }

    #[tokio::test]
    async fn plan_restore_rejects_an_unknown_connection() {
        let fx = empty_turso_fixture().await;
        let err = fx
            .service
            .plan_restore("nope", RESTORE_SCRIPT)
            .await
            .expect_err("unknown id");
        assert!(
            matches!(err, ServiceError::ConnectionNotFound(_)),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn run_restore_applies_the_script_to_an_empty_target() {
        let fx = empty_turso_fixture().await;
        let plan = fx
            .service
            .plan_restore("mem", RESTORE_SCRIPT)
            .await
            .expect("plan");
        let outcome = fx
            .service
            .run_restore("mem", &plan, RestoreOptions::default(), &NoopRestoreControl)
            .await
            .expect("restore");

        assert_eq!(outcome.statements_run, 3);
        assert_eq!(outcome.ddl_run, 1);
        assert_eq!(outcome.data_run, 2);
        assert!(!outcome.cancelled);
        assert!(outcome.failures.is_empty(), "no per-statement failures");

        // The restore landed in the same cached :memory: db: the rows are
        // now queryable through the read path.
        let out = fx
            .service
            .run_read_query("mem", "SELECT id, name FROM items ORDER BY id", None)
            .await
            .expect("query the restored table");
        assert_eq!(out.row_count, 2);
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

    /// A connection that has not opted in is refused before the statement is
    /// even classified — the gate is about the connection, not the SQL.
    #[tokio::test]
    async fn run_write_is_refused_when_the_connection_has_not_opted_in() {
        let fx = seeded_turso_fixture().await;
        let err = fx
            .service
            .run_write("mem", "UPDATE items SET name = 'x' WHERE id = 1")
            .await
            .expect_err("gate closed");
        assert!(
            matches!(err, ServiceError::WriteNotEnabled(ref id) if id == "mem"),
            "expected WriteNotEnabled, got {err:?}"
        );
        assert_eq!(name_of(&fx, 1).await, Some(Value::Text("n1".to_owned())));
    }

    #[tokio::test]
    async fn run_write_applies_dml_when_the_connection_has_opted_in() {
        let fx = seeded_turso_fixture_with_write(true).await;
        let out = fx
            .service
            .run_write("mem", "UPDATE items SET name = 'renamed' WHERE id = 1")
            .await
            .expect("update");
        assert_eq!(out.rows_affected, 1);
        assert_eq!(out.statement, "data");
        assert_eq!(
            name_of(&fx, 1).await,
            Some(Value::Text("renamed".to_owned()))
        );
    }

    /// The DDL the maintainer asked for by name (#137).
    #[tokio::test]
    async fn run_write_applies_the_ddl_the_maintainer_asked_for() {
        let fx = seeded_turso_fixture_with_write(true).await;
        for sql in [
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT)",
            "ALTER TABLE items ADD COLUMN note TEXT",
        ] {
            let out = fx.service.run_write("mem", sql).await.expect("ddl");
            assert_eq!(out.statement, "schema");
        }
        let tables = fx.service.list_tables("mem").await.expect("tables");
        assert_eq!(tables.len(), 2);
    }

    /// Opting in does not open the closed list. Both categories the
    /// maintainer named stay refused, and say they are refused permanently
    /// so an agent stops instead of rephrasing.
    #[tokio::test]
    async fn opting_in_does_not_open_the_permanently_closed_list() {
        let fx = seeded_turso_fixture_with_write(true).await;
        for sql in [
            "DROP TABLE items",
            "TRUNCATE TABLE items",
            "GRANT ALL ON items TO someone",
            "CREATE USER someone PASSWORD 'x'",
        ] {
            let err = fx
                .service
                .run_write("mem", sql)
                .await
                .expect_err("must stay closed");
            let ServiceError::WriteRefused(violation) = err else {
                panic!("expected WriteRefused for {sql:?}, got {err:?}");
            };
            assert!(violation.is_permanent(), "{sql:?} should be permanent");
        }
        // Every table survived.
        assert_eq!(
            fx.service.list_tables("mem").await.expect("tables").len(),
            1
        );
    }

    /// A `SELECT` through the write path would be an uncapped read wearing
    /// the wrong name, so it is refused even on an opted-in connection.
    #[tokio::test]
    async fn run_write_refuses_a_read_so_it_cannot_dodge_the_row_cap() {
        let fx = seeded_turso_fixture_with_write(true).await;
        let err = fx
            .service
            .run_write("mem", "SELECT * FROM items")
            .await
            .expect_err("read refused");
        assert!(matches!(err, ServiceError::WriteRefused(_)), "{err:?}");
    }

    /// The gate is re-read from disk on every call, while the adapter stays
    /// cached. Revoking it therefore takes effect on the next statement,
    /// without restarting the server the agent is holding open.
    #[tokio::test]
    async fn revoking_the_gate_takes_effect_without_a_restart() {
        let fx = seeded_turso_fixture_with_write(true).await;
        fx.service
            .run_write("mem", "UPDATE items SET name = 'a' WHERE id = 1")
            .await
            .expect("allowed while open");

        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );

        let err = fx
            .service
            .run_write("mem", "UPDATE items SET name = 'b' WHERE id = 1")
            .await
            .expect_err("gate now closed");
        assert!(matches!(err, ServiceError::WriteNotEnabled(_)), "{err:?}");
        // The cached adapter is still live, so this is the gate refusing and
        // not the connection having gone away.
        assert_eq!(name_of(&fx, 1).await, Some(Value::Text("a".to_owned())));
    }

    #[tokio::test]
    async fn run_write_rejects_an_unknown_connection() {
        let fx = seeded_turso_fixture_with_write(true).await;
        let err = fx
            .service
            .run_write("nope", "UPDATE items SET name = 'x'")
            .await
            .expect_err("unknown id");
        assert!(
            matches!(err, ServiceError::ConnectionNotFound(_)),
            "{err:?}"
        );
    }

    /// Dump is outside the write gate: taking a backup does not change the
    /// database, and needing the flag for it would mean the safest thing an
    /// agent can do costs the same permission as the least safe.
    #[tokio::test]
    async fn dump_to_file_writes_a_backup_without_the_write_gate() {
        let fx = seeded_turso_fixture().await;
        let out_path = fx.dir.path().join("backup.sql");
        let out = fx
            .service
            .dump_to_file("mem", &out_path)
            .await
            .expect("dump");
        assert_eq!(out.tables_dumped, 1);
        assert_eq!(out.rows_written, 5);
        assert!(out.failed_tables.is_empty());

        let text = std::fs::read_to_string(&out_path).expect("read back");
        // Data-only under SQLite/Turso, as `run_dump` already documents
        // (ADR-0049) — the file the tool writes is the file the desktop
        // writes, not a second dump format for agents.
        assert!(text.contains("INSERT INTO"), "should carry the rows");
        assert_eq!(out.bytes_written, text.len() as u64);
        assert!(out.complete, "nothing failed or truncated");
    }

    /// The one thing an agent could destroy through a read-only operation is
    /// whatever was already at the path. It cannot.
    #[tokio::test]
    async fn dump_to_file_never_overwrites() {
        let fx = seeded_turso_fixture().await;
        let out_path = fx.dir.path().join("taken.sql");
        std::fs::write(&out_path, "precious").expect("seed file");

        let err = fx
            .service
            .dump_to_file("mem", &out_path)
            .await
            .expect_err("must refuse");
        assert!(matches!(err, ServiceError::InvalidRequest(_)), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(&out_path).expect("still there"),
            "precious"
        );
    }

    /// A relative path resolves against the server's working directory,
    /// which the agent cannot see and did not choose.
    #[tokio::test]
    async fn dump_to_file_requires_an_absolute_path() {
        let fx = seeded_turso_fixture().await;
        let err = fx
            .service
            .dump_to_file("mem", Path::new("backup.sql"))
            .await
            .expect_err("must refuse");
        assert!(matches!(err, ServiceError::InvalidRequest(_)), "{err:?}");
    }

    /// Creating the directory would be dbboard deciding where a backup
    /// belongs; a missing parent usually means the agent guessed the path.
    #[tokio::test]
    async fn dump_to_file_does_not_create_the_directory() {
        let fx = seeded_turso_fixture().await;
        let out_path = fx.dir.path().join("nope").join("backup.sql");
        let err = fx
            .service
            .dump_to_file("mem", &out_path)
            .await
            .expect_err("must refuse");
        assert!(matches!(err, ServiceError::InvalidRequest(_)), "{err:?}");
        assert!(!out_path.parent().expect("parent").exists());
    }

    #[tokio::test]
    async fn dump_to_file_rejects_an_unknown_connection() {
        let fx = seeded_turso_fixture().await;
        let err = fx
            .service
            .dump_to_file("nope", &fx.dir.path().join("b.sql"))
            .await
            .expect_err("unknown id");
        assert!(
            matches!(err, ServiceError::ConnectionNotFound(_)),
            "{err:?}"
        );
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

    /// A two-table schema so search can distinguish a table-name hit from a
    /// column-name hit, and match a column that lives in only one table.
    async fn seeded_search_fixture() -> Fixture {
        let fx = fixture();
        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        let adapter = fx.service.adapter_for("mem").await.expect("connect mem");
        adapter
            .query("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .expect("create items");
        adapter
            .query(
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, item_id INTEGER, customer_email TEXT)",
            )
            .await
            .expect("create orders");
        fx
    }

    fn matched_col_names(m: &SchemaMatch) -> Vec<&str> {
        m.matched_columns.iter().map(|c| c.name.as_str()).collect()
    }

    #[tokio::test]
    async fn search_schema_matches_a_column_name() {
        let fx = seeded_search_fixture().await;
        let out = fx
            .service
            .search_schema("mem", "email")
            .await
            .expect("search");
        assert_eq!(out.matches.len(), 1, "only orders has an email column");
        let m = &out.matches[0];
        assert_eq!(m.table.name, "orders");
        assert!(
            !m.table_name_matched,
            "the table name does not contain 'email'"
        );
        assert_eq!(matched_col_names(m), vec!["customer_email"]);
    }

    #[tokio::test]
    async fn search_schema_matches_table_name_and_column_across_tables() {
        let fx = seeded_search_fixture().await;
        let out = fx
            .service
            .search_schema("mem", "item")
            .await
            .expect("search");
        // `items` matches by table name; `orders` matches via `item_id`.
        assert_eq!(out.matches.len(), 2);
        let items = out
            .matches
            .iter()
            .find(|m| m.table.name == "items")
            .expect("items present");
        assert!(items.table_name_matched);
        assert!(
            items.matched_columns.is_empty(),
            "no `items` column name contains 'item'; the flag carries the hit"
        );
        let orders = out
            .matches
            .iter()
            .find(|m| m.table.name == "orders")
            .expect("orders present");
        assert!(!orders.table_name_matched);
        assert_eq!(matched_col_names(orders), vec!["item_id"]);
    }

    #[tokio::test]
    async fn search_schema_is_case_insensitive() {
        let fx = seeded_search_fixture().await;
        let out = fx
            .service
            .search_schema("mem", "EMAIL")
            .await
            .expect("search");
        assert_eq!(out.matches.len(), 1);
        assert_eq!(matched_col_names(&out.matches[0]), vec!["customer_email"]);
    }

    #[tokio::test]
    async fn search_schema_no_match_is_empty() {
        let fx = seeded_search_fixture().await;
        let out = fx
            .service
            .search_schema("mem", "zzz")
            .await
            .expect("search");
        assert!(out.matches.is_empty());
        assert_eq!(out.pattern, "zzz");
        assert_eq!(out.connection_id, "mem");
    }

    #[tokio::test]
    async fn search_schema_rejects_a_blank_pattern() {
        let fx = seeded_search_fixture().await;
        for blank in ["", "   ", "\t"] {
            let err = fx
                .service
                .search_schema("mem", blank)
                .await
                .expect_err("blank pattern must be rejected");
            assert!(
                matches!(err, ServiceError::InvalidRequest(_)),
                "blank {blank:?} should be InvalidRequest, got {err:?}"
            );
        }
    }

    #[tokio::test]
    async fn search_schema_unknown_connection_is_not_found() {
        let fx = fixture();
        write(&fx.config_path, "version = 1\n");
        let err = fx
            .service
            .search_schema("nope", "x")
            .await
            .expect_err("unknown id");
        assert!(matches!(err, ServiceError::ConnectionNotFound(id) if id == "nope"));
    }

    #[tokio::test]
    async fn search_schema_caps_matches_and_flags_truncation() {
        let fx = fixture();
        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        let adapter = fx.service.adapter_for("mem").await.expect("connect mem");
        // One more table than the cap, every name containing the needle.
        for i in 0..=MAX_SCHEMA_MATCHES {
            adapter
                .query(&format!("CREATE TABLE match_{i} (id INTEGER PRIMARY KEY)"))
                .await
                .expect("create");
        }
        let out = fx
            .service
            .search_schema("mem", "match")
            .await
            .expect("search");
        assert_eq!(out.matches.len(), MAX_SCHEMA_MATCHES);
        assert!(
            out.truncated,
            "more tables than the cap must flag truncated"
        );
    }

    /// A three-table chain with two foreign keys:
    /// `order_items` → `orders` → `customers`.
    async fn seeded_relationship_fixture() -> Fixture {
        let fx = fixture();
        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        let adapter = fx.service.adapter_for("mem").await.expect("connect mem");
        adapter
            .query("CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT)")
            .await
            .expect("create customers");
        adapter
            .query(
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, \
                 customer_id INTEGER REFERENCES customers(id))",
            )
            .await
            .expect("create orders");
        adapter
            .query(
                "CREATE TABLE order_items (id INTEGER PRIMARY KEY, \
                 order_id INTEGER REFERENCES orders(id), sku TEXT)",
            )
            .await
            .expect("create order_items");
        fx
    }

    /// Find the edge from `from` (child) to `to` (parent) in a view.
    fn edge<'a>(view: &'a RelationshipView, from: &str, to: &str) -> &'a Relationship {
        view.relationships
            .iter()
            .find(|r| r.from_table.name == from && r.to_table.name == to)
            .unwrap_or_else(|| panic!("edge {from} -> {to} not found in {:?}", view.relationships))
    }

    #[tokio::test]
    async fn list_relationships_reports_every_foreign_key_edge() {
        let fx = seeded_relationship_fixture().await;
        let view = fx
            .service
            .list_relationships("mem", None)
            .await
            .expect("relationships");
        assert_eq!(view.connection_id, "mem");
        assert_eq!(view.table, None);
        assert!(!view.truncated);
        assert_eq!(view.relationships.len(), 2);

        let o = edge(&view, "orders", "customers");
        assert_eq!(o.from_columns, vec!["customer_id".to_owned()]);
        assert_eq!(o.to_columns, vec!["id".to_owned()]);

        let oi = edge(&view, "order_items", "orders");
        assert_eq!(oi.from_columns, vec!["order_id".to_owned()]);
        assert_eq!(oi.to_columns, vec!["id".to_owned()]);
    }

    #[tokio::test]
    async fn list_relationships_filter_matches_edges_on_either_side() {
        let fx = seeded_relationship_fixture().await;

        // `orders` is a child of `customers` and a parent of `order_items`,
        // so filtering on it must surface both edges (inbound + outbound).
        let view = fx
            .service
            .list_relationships("mem", Some("orders"))
            .await
            .expect("filtered");
        assert_eq!(view.table.as_deref(), Some("orders"));
        assert_eq!(view.relationships.len(), 2);
        edge(&view, "orders", "customers");
        edge(&view, "order_items", "orders");

        // `customers` is only ever a parent — one inbound edge.
        let leaf = fx
            .service
            .list_relationships("mem", Some("customers"))
            .await
            .expect("leaf");
        assert_eq!(leaf.relationships.len(), 1);
        edge(&leaf, "orders", "customers");
    }

    #[tokio::test]
    async fn list_relationships_filter_is_case_insensitive() {
        let fx = seeded_relationship_fixture().await;
        let view = fx
            .service
            .list_relationships("mem", Some("CUSTOMERS"))
            .await
            .expect("filtered");
        assert_eq!(view.relationships.len(), 1);
    }

    #[tokio::test]
    async fn list_relationships_blank_filter_is_treated_as_no_filter() {
        let fx = seeded_relationship_fixture().await;
        for blank in ["", "   ", "\t"] {
            let view = fx
                .service
                .list_relationships("mem", Some(blank))
                .await
                .expect("blank filter");
            assert_eq!(view.table, None, "blank {blank:?} should clear the filter");
            assert_eq!(view.relationships.len(), 2);
        }
    }

    /// An adapter whose `foreign_keys` fails for exactly one table, the way a
    /// D1 database's reserved `_cf_KV` does: `sqlite_master` lists it, but the
    /// Workers authorizer denies the `PRAGMA`, so the lookup comes back as
    /// `[7500] not authorized: SQLITE_AUTH`.
    struct OneDeniedTable;

    #[async_trait::async_trait]
    impl DatabaseAdapter for OneDeniedTable {
        fn id(&self) -> &'static str {
            "denied"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                has_foreign_keys: true,
                ..Default::default()
            }
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(vec![
                TableInfo::unqualified("_cf_KV"),
                TableInfo::unqualified("orders"),
            ])
        }
        async fn query(&self, _sql: &str) -> DbResult<CoreQueryResult> {
            Ok(CoreQueryResult::empty())
        }
        async fn foreign_keys(&self, table: &TableInfo) -> DbResult<Vec<CoreForeignKey>> {
            if table.name == "_cf_KV" {
                return Err(DbError::Query("[7500] not authorized: SQLITE_AUTH".into()));
            }
            Ok(vec![CoreForeignKey {
                columns: vec!["customer_id".into()],
                referenced_table: TableInfo::unqualified("customers"),
                referenced_columns: vec!["id".into()],
                constraint_name: None,
            }])
        }
    }

    /// One unreadable table must not take the whole schema down with it. The
    /// sweep visits every table, so aborting on the first error meant a single
    /// reserved D1 table blanked the desktop structure view for the *entire*
    /// database — the user saw nothing but `SQLITE_AUTH`, on tables that were
    /// perfectly readable. Skip what we cannot read; report the rest.
    #[tokio::test]
    async fn list_relationships_skips_a_table_whose_foreign_keys_are_denied() {
        let fx = fixture();
        write(&fx.config_path, "version = 1\n");
        fx.service
            .cache
            .lock()
            .await
            .insert("denied".to_string(), Arc::new(OneDeniedTable));

        let view = fx
            .service
            .list_relationships("denied", None)
            .await
            .expect("one denied table must not fail the whole call");
        assert_eq!(view.relationships.len(), 1);
        edge(&view, "orders", "customers");
        // Skipping must not be silent: the caller has to be able to tell
        // "this table has no foreign keys" from "we could not look".
        assert_eq!(
            view.unreadable_tables,
            vec![TableInfo::unqualified("_cf_KV")]
        );
    }

    /// The happy path reports nothing unreadable — the field is a real signal,
    /// not something that is always populated.
    #[tokio::test]
    async fn list_relationships_reports_no_unreadable_tables_when_all_succeed() {
        let fx = seeded_relationship_fixture().await;
        let view = fx
            .service
            .list_relationships("mem", None)
            .await
            .expect("relationships");
        assert!(view.unreadable_tables.is_empty());
    }

    #[tokio::test]
    async fn list_relationships_is_empty_when_no_foreign_keys() {
        let fx = seeded_turso_fixture().await; // one table, no FKs
        let view = fx
            .service
            .list_relationships("mem", None)
            .await
            .expect("relationships");
        assert!(view.relationships.is_empty());
        assert!(!view.truncated);
    }

    #[tokio::test]
    async fn list_relationships_unknown_connection_is_not_found() {
        let fx = fixture();
        write(&fx.config_path, "version = 1\n");
        let err = fx
            .service
            .list_relationships("nope", None)
            .await
            .expect_err("unknown id");
        assert!(matches!(err, ServiceError::ConnectionNotFound(id) if id == "nope"));
    }

    #[tokio::test]
    async fn list_relationships_caps_edges_and_flags_truncation() {
        let fx = fixture();
        write(
            &fx.config_path,
            "version = 1\n\n[[connections]]\nid=\"mem\"\nname=\"Mem\"\nkind=\"turso\"\npath=\":memory:\"\n",
        );
        let adapter = fx.service.adapter_for("mem").await.expect("connect mem");
        adapter
            .query("CREATE TABLE hub (id INTEGER PRIMARY KEY)")
            .await
            .expect("create hub");
        // One more child table than the cap, each with a single FK to hub.
        for i in 0..=MAX_RELATIONSHIPS {
            adapter
                .query(&format!(
                    "CREATE TABLE child_{i} (id INTEGER PRIMARY KEY, \
                     hub_id INTEGER REFERENCES hub(id))"
                ))
                .await
                .expect("create child");
        }
        let view = fx
            .service
            .list_relationships("mem", None)
            .await
            .expect("relationships");
        assert_eq!(view.relationships.len(), MAX_RELATIONSHIPS);
        assert!(
            view.truncated,
            "more edges than the cap must flag truncated"
        );
    }
}
