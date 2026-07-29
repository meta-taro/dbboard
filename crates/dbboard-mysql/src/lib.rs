//! `MySQL` adapter for dbboard.
//!
//! `MySQL` (and `MariaDB`, which speaks the same wire protocol) connects with an
//! ordinary `mysql://…` connection string. This adapter uses `sqlx` over a
//! [`MySqlPool`] and implements the workspace-wide [`DatabaseAdapter`]
//! contract (ADR-0012), mirroring the Postgres-wire adapter's shape but for a
//! genuinely different SQL dialect ([`SqlDialect::MySql`]): identifiers are
//! back-tick quoted, string literals double the backslash, and DDL is read
//! straight from `SHOW CREATE TABLE`.
//!
//! # Dynamic decoding
//!
//! `dbboard-core`'s [`Value`] has only the five SQLite storage classes, while
//! `MySQL` has a rich type system. Rather than enumerate every type, we run
//! statements through [`sqlx::raw_sql`] (the text protocol / `COM_QUERY`),
//! which makes the server return every value in its **text** representation.
//! Each cell is read as raw bytes and surfaced as [`Value::Text`] when the
//! bytes are valid UTF-8, or [`Value::Blob`] otherwise (so `BLOB`/`BINARY`
//! columns survive), with NULL becoming [`Value::Null`]. This is lossless for
//! numeric and temporal types and covers every type without pulling in
//! per-type decode features.

use async_trait::async_trait;
use dbboard_core::{
    classify_read_only, too_many_rows_error, Capabilities, Column, ColumnInfo, DatabaseAdapter,
    DbError, DbResult, ForeignKey, QueryResult, ReadOnlyStatement, Row, SqlDialect, TableInfo,
    TableSchema, Value, MAX_RESULT_ROWS,
};
use futures_util::TryStreamExt;
use sqlx::mysql::{
    MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow, MySqlSslMode, MySqlValueRef,
};
use sqlx::{Column as _, Connection as _, Either, Row as _, TypeInfo as _, ValueRef as _};

/// Small pool: a desktop client issues one statement at a time, so a handful
/// of connections is plenty and keeps server-side resource use modest.
const MAX_CONNECTIONS: u32 = 5;

/// `max_execution_time` (milliseconds) applied on the connection that runs a
/// read-only query ([`MySqlAdapter::query_read_only`], ADR-0046 §8). It is the
/// server-side cancellation backstop: an MCP client that drops a tool future
/// only cancels the Rust side at an await point, so this timeout is what stops
/// an abandoned query from pinning a pooled connection. `MySQL` applies
/// `max_execution_time` to read-only `SELECT`s only, so leaving it set on a
/// pooled connection never shortens a later write.
const READ_ONLY_STATEMENT_TIMEOUT_MS: u32 = 30_000;

/// Cap on error text surfaced into a [`DbError`], so a hostile or oversized
/// server message cannot dump an unbounded string into the UI.
const MAX_ERROR_DETAIL: usize = 2048;

/// User base tables of the connected database. `MySQL` scopes tables to a single
/// database per connection, so `DATABASE()` is the natural namespace; views and
/// system schemas are excluded.
const LIST_TABLES_SQL: &str = "SELECT table_schema, table_name FROM information_schema.tables \
     WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' \
     ORDER BY table_name";

/// Columns of one table in ordinal order (ADR-0028). `column_type` carries the
/// full declared type (e.g. `varchar(255)`, `int unsigned`); `ordinal_position`
/// is cast to `SIGNED` because `MySQL` reports it as an unsigned integer.
const DESCRIBE_COLUMNS_SQL: &str = "SELECT column_name, column_type, is_nullable, \
     column_default, CAST(ordinal_position AS SIGNED) \
     FROM information_schema.columns \
     WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? \
     ORDER BY ordinal_position";

/// Primary-key column names of one table in key order (ADR-0028). A `MySQL`
/// primary key is always named `PRIMARY`.
const DESCRIBE_PK_SQL: &str = "SELECT column_name \
     FROM information_schema.key_column_usage \
     WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? \
       AND constraint_name = 'PRIMARY' \
     ORDER BY ordinal_position";

/// Foreign keys of one table, one row per key column in key order (ADR-0054).
/// `referenced_table_name IS NOT NULL` selects the foreign-key rows;
/// `ordinal_position` keeps a composite key's columns in order so the assembler
/// can fold them without a secondary group pass.
const FOREIGN_KEYS_SQL: &str = "SELECT constraint_name, column_name, \
     referenced_table_schema, referenced_table_name, referenced_column_name \
     FROM information_schema.key_column_usage \
     WHERE table_schema = COALESCE(?, DATABASE()) AND table_name = ? \
       AND referenced_table_name IS NOT NULL \
     ORDER BY constraint_name, ordinal_position";

/// Connection parameters for a `MySQL` database.
///
/// `url` is a secret: it embeds the password and is never logged, never echoed
/// in a [`DbError`], and never derived into `Debug`.
pub struct MySqlConfig {
    pub url: String,
}

/// Stable adapter identifier reported by [`DatabaseAdapter::id`] for a `MySQL`
/// connection. Matches [`SqlDialect::MySql`]'s adapter-id mapping in
/// `dbboard-core`.
pub const FLAVOR_MYSQL: &str = "mysql";

pub struct MySqlAdapter {
    // Only the pool is retained; the connection URL (with its password) is
    // intentionally not stored, so it cannot leak through Debug.
    pool: MySqlPool,
}

impl MySqlAdapter {
    /// Connect to a `MySQL` database and build a connection pool.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] when the URL is empty or the pool cannot
    /// establish a connection (bad host, TLS failure, auth rejection,
    /// timeout, ...).
    pub async fn connect(config: MySqlConfig) -> DbResult<Self> {
        if config.url.trim().is_empty() {
            return Err(DbError::Connection(
                "MySQL connection URL is empty".to_string(),
            ));
        }

        // Parse the URL ourselves so we can harden the TLS policy before
        // connecting; a parse failure is reduced to a fixed string by
        // `classify_error` so the password cannot leak.
        let options: MySqlConnectOptions = config.url.parse().map_err(|e| classify_error(&e))?;
        Self::connect_options(options).await
    }

    /// Build the pool from already-parsed options, hardening the TLS policy
    /// first.
    async fn connect_options(options: MySqlConnectOptions) -> DbResult<Self> {
        let pool = MySqlPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(harden_ssl_mode(options))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl DatabaseAdapter for MySqlAdapter {
    fn id(&self) -> &'static str {
        FLAVOR_MYSQL
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            // DDL comes verbatim from `SHOW CREATE TABLE`.
            has_table_ddl: true,
            has_execute: true,
            has_foreign_keys: true,
            // InnoDB restores run as one multi-statement transaction. A DDL
            // statement causes an implicit commit in `MySQL`, so a dump that
            // mixes schema and data is not truly atomic; dbboard's logical dump
            // is data-only (ADR-0049), for which the InnoDB transaction is
            // all-or-nothing.
            has_atomic_restore: true,
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        sqlx::raw_sql("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| classify_error(&e))
            .map(|_| ())
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        // A failed introspection query is a schema error to the rest of the
        // system, not a user query error.
        let result = self
            .query(LIST_TABLES_SQL)
            .await
            .map_err(reclassify_schema)?;
        result
            .rows
            .iter()
            .map(|row| match (row.get(0), row.get(1)) {
                (Some(Value::Text(schema)), Some(Value::Text(name))) => {
                    Ok(tuple_to_table(schema.clone(), name.clone()))
                }
                other => Err(DbError::Schema(format!(
                    "unexpected row shape from information_schema.tables: {other:?}"
                ))),
            })
            .collect()
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        // An unqualified `TableInfo` falls back to `DATABASE()` — the single
        // database the connection is bound to.
        let schema = table.schema.as_deref();

        // Unlike `query`, this path uses the extended (prepared) protocol
        // (`sqlx::query` + binds): schema/table names come from introspection
        // data, and binding keeps them out of the SQL text.
        let column_rows = sqlx::query(DESCRIBE_COLUMNS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;
        // information_schema returns an empty set (not an error) for an unknown
        // table; surface it as a query error like the other adapters do.
        if column_rows.is_empty() {
            return Err(DbError::Query(format!(
                "table `{}` does not exist",
                table.name
            )));
        }

        let pk_rows = sqlx::query(DESCRIBE_PK_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;
        let primary_key = pk_rows
            .iter()
            .map(|row| row.try_get(0).map_err(|e| classify_error(&e)))
            .collect::<DbResult<Vec<String>>>()?;

        let columns = column_rows
            .iter()
            .map(|row| -> DbResult<ColumnInfo> {
                let name: String = row.try_get(0).map_err(|e| classify_error(&e))?;
                let data_type: String = row.try_get(1).map_err(|e| classify_error(&e))?;
                let is_nullable: String = row.try_get(2).map_err(|e| classify_error(&e))?;
                let default_value: Option<String> =
                    row.try_get(3).map_err(|e| classify_error(&e))?;
                let ordinal: i64 = row.try_get(4).map_err(|e| classify_error(&e))?;
                column_from_parts(
                    name,
                    data_type,
                    &is_nullable,
                    default_value,
                    ordinal,
                    &primary_key,
                )
            })
            .collect::<DbResult<Vec<_>>>()?;

        Ok(TableSchema {
            table: table.clone(),
            columns,
            primary_key,
        })
    }

    async fn foreign_keys(&self, table: &TableInfo) -> DbResult<Vec<ForeignKey>> {
        let schema = table.schema.as_deref();

        let rows = sqlx::query(FOREIGN_KEYS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;

        let fk_rows = rows
            .iter()
            .map(|row| -> DbResult<FkRow> {
                Ok(FkRow {
                    constraint_name: row.try_get(0).map_err(|e| classify_error(&e))?,
                    local_column: row.try_get(1).map_err(|e| classify_error(&e))?,
                    referenced_schema: row.try_get(2).map_err(|e| classify_error(&e))?,
                    referenced_table: row.try_get(3).map_err(|e| classify_error(&e))?,
                    referenced_column: row.try_get(4).map_err(|e| classify_error(&e))?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        Ok(assemble_foreign_keys(fk_rows))
    }

    async fn table_ddl(&self, table: &TableInfo) -> DbResult<String> {
        // `SHOW CREATE TABLE` does not accept placeholders, so the identifier
        // is quoted and injected. The name comes from introspection data, and
        // back-tick quoting (doubling an embedded back-tick) keeps a hostile
        // table name from breaking out of the identifier.
        let ident = qualified_ident(table);
        let sql = format!("SHOW CREATE TABLE {ident}");
        let row = sqlx::query(&sql)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;
        // `SHOW CREATE TABLE` returns two columns: the table name, then the
        // `CREATE TABLE` statement.
        row.try_get::<String, _>(1).map_err(|e| classify_error(&e))
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        // sqlx::raw_sql uses the text protocol, which streams row data and
        // command-completion counts in one pass — so SELECT and DML need no
        // separate routing. Row-returning statements expose rows and leave
        // `rows_affected` at 0; pure DML leaves `rows` empty and reports the
        // affected count.
        let mut stream = sqlx::raw_sql(sql).fetch_many(&self.pool);

        let mut columns: Option<Vec<Column>> = None;
        let mut rows: Vec<Row> = Vec::new();
        let mut affected: u64 = 0;

        while let Some(item) = stream.try_next().await.map_err(|e| classify_error(&e))? {
            match item {
                // Command-completion: carries the DML/DDL affected count.
                Either::Left(done) => affected = affected.saturating_add(done.rows_affected()),
                Either::Right(row) => {
                    if columns.is_none() {
                        columns = Some(columns_of(&row));
                    }
                    // Refuse to load past the workspace-wide cap before decoding
                    // the next row's cells (see dbboard-core::limits).
                    if rows.len() >= MAX_RESULT_ROWS {
                        return Err(too_many_rows_error());
                    }
                    rows.push(Row::new(row_to_values(&row)?));
                }
            }
        }

        let rows_affected = if rows.is_empty() { affected } else { 0 };
        Ok(QueryResult {
            columns: columns.unwrap_or_default(),
            rows,
            rows_affected,
        })
    }

    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        // Prove a single read-only statement under the `MySQL` grammar, and learn
        // whether it is a plain query or an EXPLAIN.
        let kind = classify_read_only(sql, SqlDialect::MySql)?;
        // The transaction body lives in a free `async fn`: nesting the sqlx
        // `Executor` borrows inside an `#[async_trait]` method trips the
        // "implementation of `Executor` is not general enough" HRTB error,
        // which a plain async fn with concrete lifetimes avoids.
        run_read_only_txn(self.pool.clone(), sql, max_rows, kind).await
    }

    async fn execute(&self, sql: &str) -> DbResult<u64> {
        // Reuse the text-protocol path: it already streams command-completion
        // counts, so a DML statement reports its affected count and a
        // row-returning statement (rare in a restore) runs and reports 0.
        self.query(sql).await.map(|result| result.rows_affected)
    }

    async fn execute_in_transaction(&self, statements: &[String]) -> DbResult<()> {
        // An empty batch would open and commit an empty transaction; skip it.
        if statements.is_empty() {
            return Ok(());
        }
        run_restore_txn(self.pool.clone(), statements).await
    }
}

/// Execute a validated read-only statement inside a server-side `READ ONLY`
/// transaction and return at most `max_rows` rows.
///
/// `SET TRANSACTION READ ONLY` (no SESSION/GLOBAL scope) marks only the *next*
/// transaction read-only, so the engine itself rejects every write for its
/// whole duration — defense-in-depth behind the pre-connection
/// [`classify_read_only`] AST guard, which already rejects every write,
/// multi-statement batch, and data-modifying CTE. The sqlx `Transaction` rolls
/// back on drop, so an early `?` return never leaves the pooled connection
/// mid-transaction.
///
/// Lives as a free `async fn` so the sqlx `Executor` borrows have concrete
/// lifetimes (see [`MySqlAdapter::query_read_only`]).
async fn run_read_only_txn(
    pool: MySqlPool,
    sql: &str,
    max_rows: usize,
    kind: ReadOnlyStatement,
) -> DbResult<QueryResult> {
    let mut conn = pool.acquire().await.map_err(|e| classify_error(&e))?;

    // Engine-level guards applied to the connection before the transaction
    // opens (both must precede `START TRANSACTION`). See the constant's docs
    // for why the session `max_execution_time` is safe to leave set.
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;
    sqlx::query(&format!(
        "SET max_execution_time = {READ_ONLY_STATEMENT_TIMEOUT_MS}"
    ))
    .execute(&mut *conn)
    .await
    .map_err(|e| classify_error(&e))?;

    let mut tx = conn.begin().await.map_err(|e| classify_error(&e))?;
    let fetched = match kind {
        // A plain query streams its result and stops after `max_rows` rows, so
        // at most `max_rows` cross the wire without wrapping arbitrary SQL in a
        // `LIMIT` subquery (which would break on duplicate output columns).
        ReadOnlyStatement::Query => fetch_capped_stream(&mut tx, sql, max_rows).await,
        // EXPLAIN returns a small, bounded plan; run it directly.
        ReadOnlyStatement::Explain => run_capped(&mut tx, sql).await,
    };

    // Read-only txn: nothing to commit. Roll back to release the snapshot
    // promptly. Surface a fetch failure ahead of a rollback failure so the
    // caller sees the real cause.
    let rollback = tx.rollback().await;
    let mut result = fetched?;
    rollback.map_err(|e| classify_error(&e))?;
    result.truncate_rows(max_rows);
    Ok(result)
}

/// Cap a read-only query: stream the result and stop after `max_rows` rows,
/// then drop the stream so the server stops producing more.
///
/// Takes a concrete `&mut MySqlConnection` (deref-coerced from the
/// transaction) so the executor borrow has a single, nameable lifetime — the
/// same `Executor` HRTB reason the Postgres adapter documents.
async fn fetch_capped_stream(
    conn: &mut sqlx::MySqlConnection,
    sql: &str,
    max_rows: usize,
) -> DbResult<QueryResult> {
    let mut stream = sqlx::query(sql).fetch(&mut *conn);
    let mut rows: Vec<MySqlRow> = Vec::with_capacity(max_rows.min(1024));
    while rows.len() < max_rows {
        match stream.try_next().await.map_err(|e| classify_error(&e))? {
            Some(row) => rows.push(row),
            None => break,
        }
    }
    // Drop the stream before returning so the transaction can be rolled back
    // cleanly without a half-consumed result set on the wire.
    drop(stream);
    mysql_rows_to_result(&rows)
}

/// Run `sql` directly on the connection (used for EXPLAIN, which returns a
/// small bounded plan) and materialise its rows.
async fn run_capped(conn: &mut sqlx::MySqlConnection, sql: &str) -> DbResult<QueryResult> {
    let rows = sqlx::query(sql)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;
    mysql_rows_to_result(&rows)
}

/// Apply `statements` as one atomic transaction on a pooled connection.
///
/// The sqlx `Transaction` rolls back on drop, so an early `?` return from a
/// failed statement leaves the target untouched rather than half-populated —
/// the all-or-nothing guarantee ADR-0051 relies on for `has_atomic_restore`.
/// (A DDL statement in the batch would trigger `MySQL`'s implicit commit; the
/// logical dump is data-only, so a restore batch is INSERTs only.)
async fn run_restore_txn(pool: MySqlPool, statements: &[String]) -> DbResult<()> {
    let mut tx = pool.begin().await.map_err(|e| classify_error(&e))?;
    for stmt in statements {
        exec_in_txn(&mut tx, stmt).await?;
    }
    tx.commit().await.map_err(|e| classify_error(&e))?;
    Ok(())
}

/// Run one statement inside the restore transaction via the extended query
/// protocol, which carries exactly one command per round-trip (the restore
/// splitter already guarantees one statement per string).
async fn exec_in_txn(conn: &mut sqlx::MySqlConnection, sql: &str) -> DbResult<()> {
    sqlx::query(sql)
        .execute(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;
    Ok(())
}

/// Build a [`QueryResult`] from already-fetched rows: columns come from the
/// first row (empty when there are none), matching the row-streaming path in
/// [`MySqlAdapter::query`].
fn mysql_rows_to_result(rows: &[MySqlRow]) -> DbResult<QueryResult> {
    let columns = rows.first().map(columns_of).unwrap_or_default();
    let rows = rows
        .iter()
        .map(|row| Ok(Row::new(row_to_values(row)?)))
        .collect::<DbResult<Vec<_>>>()?;
    Ok(QueryResult {
        columns,
        rows,
        rows_affected: 0,
    })
}

/// Build the column list from a row, recording the `MySQL` type name (e.g.
/// `BIGINT`, `VARCHAR`, `DATETIME`) as the declared type.
fn columns_of(row: &MySqlRow) -> Vec<Column> {
    row.columns()
        .iter()
        .map(|col| Column {
            name: col.name().to_string(),
            declared_type: Some(col.type_info().name().to_string()),
        })
        .collect()
}

/// Decode every cell of a row into a domain [`Value`].
fn row_to_values(row: &MySqlRow) -> DbResult<Vec<Value>> {
    let count = row.len();
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let raw = row.try_get_raw(i).map_err(|e| classify_error(&e))?;
        values.push(decode_cell(raw)?);
    }
    Ok(values)
}

/// Decode a single cell. Under the text protocol every value arrives as bytes
/// in its printed representation, so reading the raw bytes and interpreting
/// them as UTF-8 yields the same string `MySQL` itself prints; bytes that are not
/// valid UTF-8 (a `BLOB`/`BINARY` value) are kept as [`Value::Blob`]. NULL maps
/// to [`Value::Null`].
fn decode_cell(raw: MySqlValueRef<'_>) -> DbResult<Value> {
    if raw.is_null() {
        return Ok(Value::Null);
    }
    // Decode as bytes (not `try_get`) so the column's declared `MySQL` type does
    // not gate reading the text-format payload.
    let bytes = <Vec<u8> as sqlx::Decode<sqlx::MySql>>::decode(raw)
        .map_err(|e| DbError::TypeConversion(truncate(&e.to_string())))?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Value::Text(text)),
        Err(e) => Ok(Value::Blob(e.into_bytes())),
    }
}

fn tuple_to_table(schema: String, name: String) -> TableInfo {
    TableInfo::qualified(schema, name)
}

/// Quote one identifier for `MySQL`: wrap in back-ticks, doubling any embedded
/// back-tick. Used to build the `SHOW CREATE TABLE` identifier, which cannot
/// be bound as a placeholder.
fn quote_ident(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

/// The back-tick-quoted `` `schema`.`table` `` (or `` `table` `` when
/// unqualified) form for a `SHOW CREATE TABLE` statement.
fn qualified_ident(table: &TableInfo) -> String {
    match &table.schema {
        Some(schema) => format!("{}.{}", quote_ident(schema), quote_ident(&table.name)),
        None => quote_ident(&table.name),
    }
}

/// One decoded row of [`FOREIGN_KEYS_SQL`], before rows are grouped into
/// composite [`ForeignKey`]s. Rows arrive ordered by constraint name then key
/// position, so a composite key's columns are consecutive and in order.
struct FkRow {
    constraint_name: String,
    local_column: String,
    referenced_schema: String,
    referenced_table: String,
    referenced_column: String,
}

/// Fold [`FOREIGN_KEYS_SQL`] rows into one [`ForeignKey`] per constraint.
///
/// Rows are pre-sorted by `(constraint_name, key position)`, and a constraint
/// name is unique within a single table, so every row of one constraint is
/// consecutive and in key order — folding against the last-built edge is enough
/// to assemble composite keys without a secondary group pass.
fn assemble_foreign_keys(rows: Vec<FkRow>) -> Vec<ForeignKey> {
    let mut out: Vec<ForeignKey> = Vec::new();
    for r in rows {
        let extends_last = out
            .last()
            .and_then(|fk| fk.constraint_name.as_deref())
            .is_some_and(|name| name == r.constraint_name);
        if extends_last {
            let last = out.last_mut().expect("extends_last implies a last edge");
            last.columns.push(r.local_column);
            last.referenced_columns.push(r.referenced_column);
        } else {
            out.push(ForeignKey {
                columns: vec![r.local_column],
                referenced_table: tuple_to_table(r.referenced_schema, r.referenced_table),
                referenced_columns: vec![r.referenced_column],
                constraint_name: Some(r.constraint_name),
            });
        }
    }
    out
}

/// Assemble a [`ColumnInfo`] from one `information_schema.columns` row.
///
/// `is_nullable` is the SQL-standard `"YES"`/`"NO"` string, compared
/// case-insensitively. `ordinal` must be positive — `information_schema`
/// guarantees a 1-based `ordinal_position`, so anything else means a broken
/// catalog and is rejected instead of silently cast (ADR-0028 Decision 3).
fn column_from_parts(
    name: String,
    data_type: String,
    is_nullable: &str,
    default_value: Option<String>,
    ordinal: i64,
    primary_key: &[String],
) -> DbResult<ColumnInfo> {
    let ordinal = u32::try_from(ordinal)
        .ok()
        .filter(|o| *o > 0)
        .ok_or_else(|| {
            DbError::TypeConversion(format!(
                "non-positive ordinal_position {ordinal} for column {name}"
            ))
        })?;
    let in_primary_key = primary_key.iter().any(|k| k == &name);
    Ok(ColumnInfo {
        name,
        declared_type: Some(data_type),
        nullable: is_nullable.eq_ignore_ascii_case("YES"),
        primary_key: in_primary_key,
        ordinal,
        default_value,
    })
}

/// Harden the connection's TLS policy.
///
/// sqlx defaults an unspecified `ssl-mode` to [`MySqlSslMode::Preferred`],
/// which silently falls back to a plaintext connection when the server does not
/// offer TLS — sending the password in the clear with no error. Upgrade that
/// default to [`MySqlSslMode::Required`]. Any explicit choice (including
/// `ssl-mode=DISABLED` for a deliberately insecure local node) is preserved.
fn harden_ssl_mode(options: MySqlConnectOptions) -> MySqlConnectOptions {
    if matches!(options.get_ssl_mode(), MySqlSslMode::Preferred) {
        options.ssl_mode(MySqlSslMode::Required)
    } else {
        options
    }
}

/// Classify a sqlx error into a domain [`DbError`].
///
/// Server-reported SQL errors are [`DbError::Query`]; transport, TLS, and pool
/// failures are [`DbError::Connection`]; decode/type problems are
/// [`DbError::TypeConversion`]. The connection URL is never part of any message
/// — in particular [`sqlx::Error::Configuration`] (which can wrap the URL while
/// parsing it) is reduced to a fixed string so the password cannot leak.
fn classify_error(err: &sqlx::Error) -> DbError {
    match err {
        // Server-side SQL failure. The database message is safe to show (it
        // never contains the connection password).
        sqlx::Error::Database(db) => DbError::Query(truncate(db.message())),

        // URL parsing/configuration: the source may embed the URL with its
        // password, so do not surface it.
        sqlx::Error::Configuration(_) => {
            DbError::Connection("invalid MySQL connection configuration".to_string())
        }

        // Transport / availability failures.
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => DbError::Connection(truncate(&err.to_string())),

        // Decoding / type resolution problems.
        sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::TypeNotFound { .. } => DbError::TypeConversion(truncate(&err.to_string())),

        // `sqlx::Error` is `#[non_exhaustive]`; treat anything else as a
        // query-level failure with a bounded message.
        other => DbError::Query(truncate(&other.to_string())),
    }
}

/// Re-tag a `Query`/`TypeConversion` failure raised during introspection as a
/// `Schema` error, leaving connection failures intact.
fn reclassify_schema(err: DbError) -> DbError {
    match err {
        DbError::Query(msg) | DbError::TypeConversion(msg) => DbError::Schema(msg),
        other => other,
    }
}

/// Truncate `text` to [`MAX_ERROR_DETAIL`] bytes on a char boundary, appending
/// an ellipsis when shortened.
fn truncate(text: &str) -> String {
    if text.len() <= MAX_ERROR_DETAIL {
        return text.to_string();
    }
    let mut end = MAX_ERROR_DETAIL;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_foreign_keys, classify_error, column_from_parts, harden_ssl_mode, qualified_ident,
        quote_ident, reclassify_schema, truncate, FkRow, FLAVOR_MYSQL,
    };
    use dbboard_core::{DatabaseAdapter, DbError, TableInfo};
    use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};

    /// `id()` is part of the public contract: the adapter identifier `mysql`
    /// is a stable string that `SqlDialect`'s adapter-id mapping and capability
    /// consumers match on. It must keep its byte content stable across releases.
    #[test]
    fn flavor_constant_is_stable() {
        assert_eq!(FLAVOR_MYSQL, "mysql");
    }

    #[test]
    fn unspecified_ssl_mode_is_upgraded_to_required() {
        // A bare `MySqlConnectOptions` defaults to `Preferred`, the silent
        // plaintext-fallback mode we refuse to ship. (`MySqlSslMode` is not
        // `PartialEq`, so assert with `matches!`.)
        let opts = MySqlConnectOptions::new();
        assert!(matches!(opts.get_ssl_mode(), MySqlSslMode::Preferred));
        assert!(matches!(
            harden_ssl_mode(opts).get_ssl_mode(),
            MySqlSslMode::Required
        ));
    }

    #[test]
    fn explicit_ssl_mode_is_preserved() {
        // An explicit `Disabled` (deliberately insecure local node) and an
        // explicit `VerifyIdentity` both pass through untouched.
        let disabled = MySqlConnectOptions::new().ssl_mode(MySqlSslMode::Disabled);
        assert!(matches!(
            harden_ssl_mode(disabled).get_ssl_mode(),
            MySqlSslMode::Disabled
        ));
        let verified = MySqlConnectOptions::new().ssl_mode(MySqlSslMode::VerifyIdentity);
        assert!(matches!(
            harden_ssl_mode(verified).get_ssl_mode(),
            MySqlSslMode::VerifyIdentity
        ));
    }

    /// `connect_lazy_with` builds a pool without any network I/O, which is
    /// enough to read the adapter's static capability flags. It still needs a
    /// Tokio context to spawn the pool's background worker, hence
    /// `#[tokio::test]`.
    #[tokio::test]
    async fn capabilities_advertise_the_full_surface() {
        let pool = MySqlPoolOptions::new().connect_lazy_with(MySqlConnectOptions::new());
        let adapter = super::MySqlAdapter { pool };
        let caps = adapter.capabilities();
        assert!(caps.has_describe_table);
        assert!(caps.has_table_ddl);
        assert!(caps.has_execute);
        assert!(caps.has_foreign_keys);
        assert!(caps.has_atomic_restore);
        assert_eq!(adapter.id(), "mysql");
    }

    #[test]
    fn quote_ident_wraps_in_backticks_and_doubles_embedded() {
        assert_eq!(quote_ident("users"), "`users`");
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
    }

    #[test]
    fn qualified_ident_uses_the_database_when_present() {
        assert_eq!(
            qualified_ident(&TableInfo::qualified("shop", "orders")),
            "`shop`.`orders`"
        );
        assert_eq!(
            qualified_ident(&TableInfo::unqualified("orders")),
            "`orders`"
        );
    }

    #[test]
    fn column_from_parts_reads_nullability_and_primary_key() {
        let pk = vec!["id".to_string()];
        let id = column_from_parts("id".into(), "int unsigned".into(), "NO", None, 1, &pk).unwrap();
        assert!(id.primary_key);
        assert!(!id.nullable);
        assert_eq!(id.ordinal, 1);
        assert_eq!(id.declared_type.as_deref(), Some("int unsigned"));

        let name = column_from_parts(
            "name".into(),
            "varchar(255)".into(),
            "YES",
            Some("''".into()),
            2,
            &pk,
        )
        .unwrap();
        assert!(!name.primary_key);
        assert!(name.nullable);
        assert_eq!(name.default_value.as_deref(), Some("''"));
    }

    #[test]
    fn column_from_parts_rejects_a_non_positive_ordinal() {
        let err = column_from_parts("c".into(), "int".into(), "NO", None, 0, &[]).unwrap_err();
        assert!(matches!(err, DbError::TypeConversion(_)));
    }

    #[test]
    fn assemble_foreign_keys_folds_a_composite_key_into_one_edge() {
        let rows = vec![
            FkRow {
                constraint_name: "fk_order".into(),
                local_column: "a".into(),
                referenced_schema: "shop".into(),
                referenced_table: "parent".into(),
                referenced_column: "x".into(),
            },
            FkRow {
                constraint_name: "fk_order".into(),
                local_column: "b".into(),
                referenced_schema: "shop".into(),
                referenced_table: "parent".into(),
                referenced_column: "y".into(),
            },
        ];
        let fks = assemble_foreign_keys(rows);
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].columns, vec!["a", "b"]);
        assert_eq!(fks[0].referenced_columns, vec!["x", "y"]);
        assert_eq!(fks[0].constraint_name.as_deref(), Some("fk_order"));
        assert_eq!(fks[0].referenced_table.name, "parent");
    }

    #[test]
    fn assemble_foreign_keys_keeps_distinct_constraints_separate() {
        let rows = vec![
            FkRow {
                constraint_name: "fk_a".into(),
                local_column: "a".into(),
                referenced_schema: "shop".into(),
                referenced_table: "p1".into(),
                referenced_column: "x".into(),
            },
            FkRow {
                constraint_name: "fk_b".into(),
                local_column: "b".into(),
                referenced_schema: "shop".into(),
                referenced_table: "p2".into(),
                referenced_column: "y".into(),
            },
        ];
        let fks = assemble_foreign_keys(rows);
        assert_eq!(fks.len(), 2);
    }

    #[test]
    fn classify_error_maps_configuration_without_leaking_the_url() {
        let err = classify_error(&sqlx::Error::Configuration("bad url".into()));
        match err {
            DbError::Connection(msg) => {
                assert!(msg.contains("invalid MySQL connection configuration"));
                assert!(!msg.contains("bad url"));
            }
            other => panic!("expected Connection, got {other:?}"),
        }
    }

    #[test]
    fn classify_error_maps_pool_timeout_to_connection() {
        assert!(matches!(
            classify_error(&sqlx::Error::PoolTimedOut),
            DbError::Connection(_)
        ));
    }

    #[test]
    fn reclassify_schema_retags_query_and_type_errors_only() {
        assert!(matches!(
            reclassify_schema(DbError::Query("x".into())),
            DbError::Schema(_)
        ));
        assert!(matches!(
            reclassify_schema(DbError::TypeConversion("x".into())),
            DbError::Schema(_)
        ));
        assert!(matches!(
            reclassify_schema(DbError::Connection("x".into())),
            DbError::Connection(_)
        ));
    }

    #[test]
    fn truncate_shortens_on_a_char_boundary() {
        let long = "a".repeat(super::MAX_ERROR_DETAIL + 100);
        let out = truncate(&long);
        assert!(out.len() <= super::MAX_ERROR_DETAIL + "…".len());
        assert!(out.ends_with('…'));
        // A short string is returned unchanged.
        assert_eq!(truncate("short"), "short");
    }
}
