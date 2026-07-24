//! Turso / libSQL adapter for dbboard.
//!
//! Wraps the official `libsql` crate so the rest of dbboard sees only
//! the domain types from `dbboard-core`. Implements the workspace-wide
//! [`DatabaseAdapter`] contract (ADR-0012); Phase 2 advertises no
//! optional capabilities — Turso ships base catalog access only.

use async_trait::async_trait;
use dbboard_core::{
    check_read_only, resolve_referenced_columns, too_many_rows_error, Capabilities, Column,
    ColumnInfo, DatabaseAdapter, DbError, DbResult, ForeignKey, QueryResult, Row, SqlDialect,
    TableInfo, TableSchema, Value, MAX_RESULT_ROWS,
};

pub struct TursoAdapter {
    // Field drop order matters: `conn` is dropped before `_db` so the
    // libSQL handle outlives anything that depends on it. The
    // database is also kept around so that `:memory:` instances
    // stay alive for the lifetime of the adapter — libSQL gives each
    // connection its own in-memory database, so we open exactly one
    // connection here and reuse it.
    conn: libsql::Connection,
    _db: libsql::Database,
}

impl TursoAdapter {
    /// Open a local libSQL database. Use `":memory:"` for an in-memory
    /// instance (handy in tests) or a filesystem path for persistence.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] when the underlying driver
    /// cannot open or initialise the database, or when establishing
    /// the initial connection fails.
    pub async fn connect_local(path: &str) -> DbResult<Self> {
        // libsql errors echo the supplied path back in their Display
        // output (sqlite's "unable to open database file: …"). The path
        // is the user's input, but it can carry directory layout or
        // credentials they would not expect to see in an HTTP error
        // envelope, so scrub it before surfacing the message.
        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| DbError::Connection(redact_path(e.to_string(), path)))?;
        let conn = db
            .connect()
            .map_err(|e| DbError::Connection(redact_path(e.to_string(), path)))?;
        Ok(Self { conn, _db: db })
    }

    /// Group raw `PRAGMA foreign_key_list` rows into composite
    /// [`ForeignKey`]s. Rows sharing an `id` form one constraint, ordered
    /// by `seq`; a row whose `to` is `None` referenced the parent's
    /// primary key implicitly, resolved here against the parent's PK in
    /// key order.
    async fn assemble_foreign_keys(&self, raw: Vec<RawFk>) -> DbResult<Vec<ForeignKey>> {
        // Group by fk id, preserving first-seen order so the output order
        // is stable rather than dependent on SQLite's row ordering.
        let mut groups: Vec<(i64, Vec<RawFk>)> = Vec::new();
        for r in raw {
            match groups.iter_mut().find(|(id, _)| *id == r.id) {
                Some((_, rows)) => rows.push(r),
                None => groups.push((r.id, vec![r])),
            }
        }

        let mut out = Vec::with_capacity(groups.len());
        for (_, mut rows) in groups {
            rows.sort_by_key(|r| r.seq);
            let referenced_table = TableInfo::unqualified(rows[0].referenced_table.clone());
            let columns: Vec<String> = rows.iter().map(|r| r.from.clone()).collect();

            // A `NULL` in `to` means the DDL omitted the parent column
            // list, so the reference is to the parent's primary key. One
            // describe of the parent resolves every such column at once.
            let to: Vec<Option<String>> = rows.iter().map(|r| r.to.clone()).collect();
            let referenced_columns = if to.iter().any(Option::is_none) {
                // A stale reference to a since-dropped table (SQLite does not
                // require the parent to exist) must not abort the whole
                // relationship walk — degrade to an unresolved PK for this one
                // edge instead of propagating the `describe_table` error.
                let pk = match self.describe_table(&referenced_table).await {
                    Ok(schema) => schema.primary_key,
                    Err(_) => Vec::new(),
                };
                resolve_referenced_columns(&to, &pk)
            } else {
                resolve_referenced_columns(&to, &[])
            };

            out.push(ForeignKey {
                columns,
                referenced_table,
                referenced_columns,
                // SQLite's PRAGMA does not report the constraint name.
                constraint_name: None,
            });
        }
        Ok(out)
    }
}

#[async_trait]
impl DatabaseAdapter for TursoAdapter {
    fn id(&self) -> &'static str {
        "turso"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            has_execute: true,
            has_atomic_restore: true,
            has_foreign_keys: true,
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        // `Connection::execute` is DML-only — passing a SELECT trips
        // libSQL's "Execute returned rows" guard, so the probe goes
        // through the row-returning path and discards the row.
        let mut rows = self
            .conn
            .query("SELECT 1", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let _ = rows
            .next()
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        Ok(())
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        let mut rows = self
            .conn
            .query(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
                (),
            )
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;

        let mut tables = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?
        {
            let name: String = row.get(0).map_err(|e| DbError::Schema(e.to_string()))?;
            tables.push(TableInfo::unqualified(name));
        }
        Ok(tables)
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        // libSQL splits row-returning and DML/DDL across two driver
        // entry points and rejects a statement sent through the wrong
        // one, so route by the first SQL keyword. Phase 1 targets the
        // small result sets a developer pastes into the SQL editor;
        // streaming and pagination land later.
        if is_row_returning(sql) {
            run_select(&self.conn, sql).await
        } else {
            run_execute(&self.conn, sql).await
        }
    }

    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        // Belt: prove it is a single read-only statement under the SQLite
        // grammar (also rejects the `SELECT 1; DELETE …` multi-statement
        // batch the bare `query` router would mis-handle).
        check_read_only(sql, SqlDialect::Sqlite)?;

        // Braces: SQLite's own `query_only` makes the *engine* reject any
        // write for the duration, covering anything the parser's grammar
        // might accept as a query but that still writes. The flag lives on
        // the shared connection, so it must be cleared again afterwards or
        // a later `query` write on this handle would be stuck read-only.
        set_query_only(&self.conn, true).await?;
        let result = run_select_capped(&self.conn, sql, max_rows).await;
        let reset = set_query_only(&self.conn, false).await;
        // Surface a query failure first, but never swallow a failed reset:
        // a connection left read-only would corrupt later writes silently.
        let mut result = result?;
        reset?;
        result.truncate_rows(max_rows);
        Ok(result)
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        // PRAGMA arguments cannot be bound as parameters, so the name is
        // embedded with single quotes doubled (SQLite string-literal
        // escaping). The name usually comes from `list_tables`, but a
        // hostile schema could put anything in it.
        let escaped = table.name.replace('\'', "''");
        let mut rows = self
            .conn
            .query(&format!("PRAGMA table_info('{escaped}')"), ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        let mut columns = Vec::new();
        // (pk position, column name) — collected out of order, sorted below.
        let mut pk_parts: Vec<(i64, String)> = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::Query(e.to_string()))?
        {
            columns.push(column_from_pragma_row(&row, &mut pk_parts)?);
        }

        // PRAGMA table_info returns zero rows for a missing table rather
        // than an engine error, so synthesise SQLite's own message shape
        // to satisfy the ADR-0028 "missing table is DbError::Query" rule.
        if columns.is_empty() {
            return Err(DbError::Query(format!("no such table: {}", table.name)));
        }

        pk_parts.sort_by_key(|&(position, _)| position);
        Ok(TableSchema {
            table: table.clone(),
            columns,
            primary_key: pk_parts.into_iter().map(|(_, name)| name).collect(),
        })
    }

    async fn foreign_keys(&self, table: &TableInfo) -> DbResult<Vec<ForeignKey>> {
        // `PRAGMA foreign_key_list('t')` rows: (id, seq, table, from, to,
        // on_update, on_delete, match). One row per key column; a composite
        // key shares an `id` across rows ordered by `seq`. As with
        // `describe_table`, the name is embedded with single quotes doubled —
        // it is not a bindable PRAGMA argument.
        let escaped = table.name.replace('\'', "''");
        let mut rows = self
            .conn
            .query(&format!("PRAGMA foreign_key_list('{escaped}')"), ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        let mut raw = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::Query(e.to_string()))?
        {
            raw.push(raw_fk_from_pragma_row(&row)?);
        }

        self.assemble_foreign_keys(raw).await
    }

    async fn execute(&self, sql: &str) -> DbResult<u64> {
        // Mirror `query`'s routing: libSQL rejects a row-returning statement
        // sent through `execute`. A restore script is overwhelmingly DDL/DML,
        // but an incidental row-returning statement still runs — it changes
        // nothing, so report zero rows affected.
        if is_row_returning(sql) {
            run_select(&self.conn, sql).await.map(|r| r.rows_affected)
        } else {
            self.conn
                .execute(sql, ())
                .await
                .map_err(|e| DbError::Query(e.to_string()))
        }
    }

    async fn execute_in_transaction(&self, statements: &[String]) -> DbResult<()> {
        // An empty batch would leave a dangling `BEGIN`, so treat it as a
        // no-op — the runner never hands us one, but a caller might.
        if statements.is_empty() {
            return Ok(());
        }

        self.conn
            .execute("BEGIN", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;

        for stmt in statements {
            let step = if is_row_returning(stmt) {
                run_select(&self.conn, stmt).await.map(|_| ())
            } else {
                self.conn
                    .execute(stmt, ())
                    .await
                    .map(|_| ())
                    .map_err(|e| DbError::Query(e.to_string()))
            };
            if let Err(e) = step {
                // Best-effort rollback; surface the original failure. A failed
                // rollback would only compound the same error, so it is dropped.
                let _ = self.conn.execute("ROLLBACK", ()).await;
                return Err(e);
            }
        }

        self.conn
            .execute("COMMIT", ())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        Ok(())
    }
}

/// Map one `PRAGMA table_info` row (`cid, name, type, notnull,
/// dflt_value, pk`) onto a [`ColumnInfo`], recording primary-key parts
/// into `pk_parts` as `(key position, column name)`.
fn column_from_pragma_row(
    row: &libsql::Row,
    pk_parts: &mut Vec<(i64, String)>,
) -> DbResult<ColumnInfo> {
    let type_error = |e: libsql::Error| DbError::TypeConversion(e.to_string());

    let cid: i64 = row.get(0).map_err(type_error)?;
    let name: String = row.get(1).map_err(type_error)?;
    let declared: String = row.get(2).map_err(type_error)?;
    let notnull: i64 = row.get(3).map_err(type_error)?;
    let default_value = match row.get_value(4).map_err(type_error)? {
        libsql::Value::Null => None,
        libsql::Value::Text(s) => Some(s),
        other => Some(format!("{other:?}")),
    };
    let pk: i64 = row.get(5).map_err(type_error)?;

    if pk > 0 {
        pk_parts.push((pk, name.clone()));
    }
    let ordinal = u32::try_from(cid)
        .map_err(|_| DbError::TypeConversion(format!("negative PRAGMA cid: {cid}")))?
        + 1; // cid is 0-based; ColumnInfo::ordinal is 1-based (ADR-0028).

    Ok(ColumnInfo {
        name,
        // Typeless SQLite columns report an empty string.
        declared_type: (!declared.is_empty()).then_some(declared),
        nullable: notnull == 0,
        primary_key: pk > 0,
        ordinal,
        default_value,
    })
}

/// One row of `PRAGMA foreign_key_list`, before rows are grouped into
/// composite constraints. `to` is `None` when the referencing DDL omitted
/// the parent column list (an implicit reference to the parent's primary
/// key), which [`TursoAdapter::assemble_foreign_keys`] resolves.
struct RawFk {
    id: i64,
    seq: i64,
    referenced_table: String,
    from: String,
    to: Option<String>,
}

fn raw_fk_from_pragma_row(row: &libsql::Row) -> DbResult<RawFk> {
    let type_error = |e: libsql::Error| DbError::TypeConversion(e.to_string());
    let to = match row.get_value(4).map_err(type_error)? {
        libsql::Value::Null => None,
        libsql::Value::Text(s) => Some(s),
        other => Some(format!("{other:?}")),
    };
    Ok(RawFk {
        id: row.get(0).map_err(type_error)?,
        seq: row.get(1).map_err(type_error)?,
        referenced_table: row.get(2).map_err(type_error)?,
        from: row.get(3).map_err(type_error)?,
        to,
    })
}

async fn run_select(conn: &libsql::Connection, sql: &str) -> DbResult<QueryResult> {
    let mut rows = conn
        .query(sql, ())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

    let column_count = rows.column_count();
    #[allow(clippy::cast_sign_loss)]
    let mut columns = Vec::with_capacity(column_count as usize);
    for i in 0..column_count {
        columns.push(Column {
            name: rows.column_name(i).unwrap_or_default().to_string(),
            declared_type: rows.column_type(i).ok().map(format_column_type),
        });
    }

    let mut result_rows = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DbError::Query(e.to_string()))?
    {
        // Refuse to load past the workspace-wide cap rather than
        // returning a silently truncated grid (see dbboard-core::limits).
        if result_rows.len() >= MAX_RESULT_ROWS {
            return Err(too_many_rows_error());
        }
        #[allow(clippy::cast_sign_loss)]
        let mut values = Vec::with_capacity(column_count as usize);
        for i in 0..column_count {
            let raw = row
                .get_value(i)
                .map_err(|e| DbError::TypeConversion(e.to_string()))?;
            values.push(convert_value(raw));
        }
        result_rows.push(Row::new(values));
    }

    Ok(QueryResult {
        columns,
        rows: result_rows,
        rows_affected: 0,
    })
}

/// Toggle SQLite's `query_only` PRAGMA on the connection. While ON, the
/// engine rejects every write with `SQLITE_READONLY` — the engine-level
/// half of [`TursoAdapter::query_read_only`]'s read-only guarantee.
async fn set_query_only(conn: &libsql::Connection, on: bool) -> DbResult<()> {
    let value = if on { "ON" } else { "OFF" };
    // A setter PRAGMA returns no rows, so it goes through `execute`.
    conn.execute(&format!("PRAGMA query_only = {value}"), ())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    Ok(())
}

/// Row-returning query that stops after `limit` rows rather than erroring
/// at the workspace cap. The read-only tool surface (ADR-0046) bounds an
/// agent's result set by truncation, so a broad `SELECT *` returns its
/// first `limit` rows instead of failing like [`run_select`].
async fn run_select_capped(
    conn: &libsql::Connection,
    sql: &str,
    limit: usize,
) -> DbResult<QueryResult> {
    let mut rows = conn
        .query(sql, ())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

    let column_count = rows.column_count();
    #[allow(clippy::cast_sign_loss)]
    let mut columns = Vec::with_capacity(column_count as usize);
    for i in 0..column_count {
        columns.push(Column {
            name: rows.column_name(i).unwrap_or_default().to_string(),
            declared_type: rows.column_type(i).ok().map(format_column_type),
        });
    }

    let mut result_rows = Vec::new();
    while result_rows.len() < limit {
        let Some(row) = rows
            .next()
            .await
            .map_err(|e| DbError::Query(e.to_string()))?
        else {
            break;
        };
        #[allow(clippy::cast_sign_loss)]
        let mut values = Vec::with_capacity(column_count as usize);
        for i in 0..column_count {
            let raw = row
                .get_value(i)
                .map_err(|e| DbError::TypeConversion(e.to_string()))?;
            values.push(convert_value(raw));
        }
        result_rows.push(Row::new(values));
    }

    Ok(QueryResult {
        columns,
        rows: result_rows,
        rows_affected: 0,
    })
}

async fn run_execute(conn: &libsql::Connection, sql: &str) -> DbResult<QueryResult> {
    let affected = conn
        .execute(sql, ())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    Ok(QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        rows_affected: affected,
    })
}

/// Return `true` when the SQL starts with a row-returning verb. The
/// check is intentionally shallow (first non-empty, non-comment token
/// only) — enough to route Phase 1 traffic through the right libSQL
/// entry point without dragging in a SQL parser.
fn is_row_returning(sql: &str) -> bool {
    let first_word = first_token(sql);
    matches!(
        first_word.to_ascii_uppercase().as_str(),
        "SELECT" | "WITH" | "VALUES" | "PRAGMA" | "EXPLAIN"
    )
}

/// Skip leading whitespace and SQL comments (`-- line` and `/* block */`)
/// and return the first whitespace-delimited token. An unterminated block
/// comment is treated as empty (defensive — no first-word match).
fn first_token(sql: &str) -> &str {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("--") {
            // Line comment: skip to the next newline, or end of input.
            rest = match after.find('\n') {
                Some(i) => &after[i + 1..],
                None => "",
            };
        } else if let Some(after) = rest.strip_prefix("/*") {
            // Block comment: skip to the closing `*/`. SQLite does not
            // support nested block comments, so a single-pass `find` is
            // sufficient. An unterminated comment short-circuits to "".
            rest = match after.find("*/") {
                Some(i) => &after[i + 2..],
                None => "",
            };
        } else {
            break;
        }
    }
    rest.split_whitespace().next().unwrap_or("")
}

/// Remove every occurrence of `path` from `message`, replacing it with a
/// fixed `<path>` placeholder. Used to scrub the user-supplied database
/// path out of libsql's error strings (see [`TursoAdapter::connect_local`]).
fn redact_path(message: String, path: &str) -> String {
    if path.is_empty() {
        return message;
    }
    message.replace(path, "<path>")
}

fn convert_value(v: libsql::Value) -> Value {
    match v {
        libsql::Value::Null => Value::Null,
        libsql::Value::Integer(n) => Value::Integer(n),
        libsql::Value::Real(x) => Value::Real(x),
        libsql::Value::Text(s) => Value::Text(s),
        libsql::Value::Blob(b) => Value::Blob(b),
    }
}

fn format_column_type(t: libsql::ValueType) -> String {
    match t {
        libsql::ValueType::Null => "NULL".to_string(),
        libsql::ValueType::Integer => "INTEGER".to_string(),
        libsql::ValueType::Real => "REAL".to_string(),
        libsql::ValueType::Text => "TEXT".to_string(),
        libsql::ValueType::Blob => "BLOB".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{convert_value, is_row_returning, redact_path, TursoAdapter, Value};
    use dbboard_core::{DatabaseAdapter, DbError, TableInfo};

    /// Open an in-memory adapter seeded with a `t(id INTEGER, label TEXT)`
    /// table holding `count` rows.
    async fn seeded(count: i64) -> TursoAdapter {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .query("CREATE TABLE t (id INTEGER PRIMARY KEY, label TEXT)")
            .await
            .unwrap();
        for i in 0..count {
            adapter
                .query(&format!("INSERT INTO t (id, label) VALUES ({i}, 'row{i}')"))
                .await
                .unwrap();
        }
        adapter
    }

    async fn row_count(adapter: &TursoAdapter) -> i64 {
        let result = adapter.query("SELECT count(*) FROM t").await.unwrap();
        match result.rows[0].get(0).unwrap() {
            Value::Integer(n) => *n,
            other => panic!("unexpected count value: {other:?}"),
        }
    }

    #[tokio::test]
    async fn query_read_only_returns_rows_and_truncates_to_max() {
        let adapter = seeded(5).await;
        let result = adapter
            .query_read_only("SELECT id, label FROM t ORDER BY id", 3)
            .await
            .unwrap();
        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.rows.len(), 3, "row cap should truncate to max_rows");
        assert_eq!(result.rows[0].get(0), Some(&Value::Integer(0)));
    }

    #[tokio::test]
    async fn query_read_only_rejects_a_write_and_leaves_data_intact() {
        let adapter = seeded(3).await;
        let err = adapter
            .query_read_only("DELETE FROM t", 100)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        assert_eq!(row_count(&adapter).await, 3, "the DELETE must not have run");
    }

    #[tokio::test]
    async fn query_read_only_rejects_multi_statement_batch() {
        let adapter = seeded(3).await;
        let err = adapter
            .query_read_only("SELECT 1; DELETE FROM t", 100)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        assert_eq!(row_count(&adapter).await, 3);
    }

    #[tokio::test]
    async fn query_read_only_clears_query_only_so_later_writes_succeed() {
        let adapter = seeded(2).await;
        // A read-only call flips PRAGMA query_only ON then must reset it.
        adapter
            .query_read_only("SELECT * FROM t", 100)
            .await
            .unwrap();
        // If the flag leaked, this ordinary write would fail SQLITE_READONLY.
        adapter
            .query("INSERT INTO t (id, label) VALUES (99, 'after')")
            .await
            .unwrap();
        assert_eq!(row_count(&adapter).await, 3);
    }

    #[tokio::test]
    async fn query_read_only_clears_query_only_even_after_a_rejected_write() {
        let adapter = seeded(1).await;
        // The classifier rejects this before touching the engine, but the
        // reset path must still run so the connection is writable again.
        let _ = adapter.query_read_only("DELETE FROM t", 100).await;
        adapter
            .query("INSERT INTO t (id, label) VALUES (42, 'ok')")
            .await
            .unwrap();
        assert_eq!(row_count(&adapter).await, 2);
    }

    #[test]
    fn convert_value_maps_null() {
        assert_eq!(convert_value(libsql::Value::Null), Value::Null);
    }

    #[test]
    fn convert_value_maps_integer() {
        assert_eq!(convert_value(libsql::Value::Integer(7)), Value::Integer(7));
    }

    #[test]
    fn convert_value_maps_text() {
        assert_eq!(
            convert_value(libsql::Value::Text("hi".into())),
            Value::Text("hi".into())
        );
    }

    #[test]
    fn convert_value_maps_blob() {
        assert_eq!(
            convert_value(libsql::Value::Blob(vec![1, 2, 3])),
            Value::Blob(vec![1, 2, 3])
        );
    }

    #[test]
    fn select_is_row_returning() {
        assert!(is_row_returning("SELECT 1"));
        assert!(is_row_returning("  select * from t"));
        assert!(is_row_returning(
            "\n\n  WITH x AS (SELECT 1) SELECT * FROM x"
        ));
        assert!(is_row_returning("VALUES (1, 2)"));
        assert!(is_row_returning("PRAGMA table_info(users)"));
        assert!(is_row_returning("EXPLAIN SELECT 1"));
    }

    #[test]
    fn dml_and_ddl_are_not_row_returning() {
        assert!(!is_row_returning("CREATE TABLE t (id INT)"));
        assert!(!is_row_returning("INSERT INTO t VALUES (1)"));
        assert!(!is_row_returning("UPDATE t SET id = 2"));
        assert!(!is_row_returning("DELETE FROM t"));
        assert!(!is_row_returning("DROP TABLE t"));
    }

    #[test]
    fn leading_comments_and_whitespace_are_skipped() {
        let sql = "-- pick the first user\n  -- (handy for smoke tests)\nSELECT 1";
        assert!(is_row_returning(sql));
    }

    #[test]
    fn empty_input_is_not_row_returning() {
        assert!(!is_row_returning(""));
        assert!(!is_row_returning("   \n  -- just a comment"));
    }

    #[test]
    fn leading_block_comment_is_skipped() {
        assert!(is_row_returning("/* pick the first user */ SELECT 1"));
        assert!(is_row_returning("/* multi\n   line */\nSELECT 1"));
        assert!(is_row_returning("/* a */ /* b */ SELECT 1"));
    }

    #[test]
    fn mixed_line_and_block_comments_are_skipped() {
        let sql = "-- header\n/* block */\n  -- footer\nSELECT 1";
        assert!(is_row_returning(sql));
    }

    #[test]
    fn unterminated_block_comment_short_circuits_to_no_match() {
        // No `*/`: defensive — fall through to an empty token rather than
        // pretending the SQL is row-returning.
        assert!(!is_row_returning("/* never closes  SELECT 1"));
    }

    #[test]
    fn redact_path_replaces_each_occurrence() {
        let msg = "open failed: /home/alice/db.sqlite [path: /home/alice/db.sqlite]";
        let out = redact_path(msg.to_string(), "/home/alice/db.sqlite");
        assert!(!out.contains("/home/alice"), "leaked path in: {out}");
        assert_eq!(out.matches("<path>").count(), 2);
    }

    #[test]
    fn redact_path_with_empty_path_is_a_noop() {
        // `:memory:` paths are non-empty; an empty path argument would be
        // a programming bug, but the helper must still degrade safely
        // rather than replacing every empty-string match.
        let msg = "boom".to_string();
        assert_eq!(redact_path(msg.clone(), ""), msg);
    }

    #[tokio::test]
    async fn capabilities_report_execute_and_atomic_restore() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        let caps = adapter.capabilities();
        assert!(caps.has_execute);
        assert!(caps.has_atomic_restore);
        assert!(caps.has_foreign_keys);
    }

    /// Open an in-memory adapter with a `parent` table and a `child`
    /// table carrying `child_ddl`'s foreign key(s).
    async fn with_child_fk(child_ddl: &str) -> TursoAdapter {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .query("CREATE TABLE parent (id INTEGER PRIMARY KEY, code TEXT)")
            .await
            .unwrap();
        adapter.query(child_ddl).await.unwrap();
        adapter
    }

    #[tokio::test]
    async fn foreign_keys_reports_a_single_column_reference() {
        let adapter = with_child_fk(
            "CREATE TABLE child (id INTEGER PRIMARY KEY, \
             parent_id INTEGER REFERENCES parent(id))",
        )
        .await;
        let fks = adapter
            .foreign_keys(&TableInfo::unqualified("child"))
            .await
            .unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].columns, vec!["parent_id".to_owned()]);
        assert_eq!(fks[0].referenced_table, TableInfo::unqualified("parent"));
        assert_eq!(fks[0].referenced_columns, vec!["id".to_owned()]);
        // SQLite's PRAGMA does not name the constraint.
        assert_eq!(fks[0].constraint_name, None);
    }

    #[tokio::test]
    async fn foreign_keys_reports_composite_key_columns_in_order() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .query("CREATE TABLE parent (a INTEGER, b INTEGER, PRIMARY KEY (a, b))")
            .await
            .unwrap();
        adapter
            .query(
                "CREATE TABLE child (x INTEGER, y INTEGER, \
                 FOREIGN KEY (x, y) REFERENCES parent(a, b))",
            )
            .await
            .unwrap();
        let fks = adapter
            .foreign_keys(&TableInfo::unqualified("child"))
            .await
            .unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].columns, vec!["x".to_owned(), "y".to_owned()]);
        assert_eq!(
            fks[0].referenced_columns,
            vec!["a".to_owned(), "b".to_owned()]
        );
    }

    #[tokio::test]
    async fn foreign_keys_resolves_an_implicit_reference_to_the_parent_primary_key() {
        // `REFERENCES parent` with no column list — SQLite reports `to` as
        // NULL; the adapter fills it from the parent's primary key.
        let adapter = with_child_fk(
            "CREATE TABLE child (id INTEGER PRIMARY KEY, \
             parent_id INTEGER REFERENCES parent)",
        )
        .await;
        let fks = adapter
            .foreign_keys(&TableInfo::unqualified("child"))
            .await
            .unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].referenced_columns, vec!["id".to_owned()]);
    }

    #[tokio::test]
    async fn foreign_keys_degrades_to_rowid_for_a_dangling_implicit_reference() {
        // A child with an implicit reference to a table that does not exist
        // (foreign-key enforcement is off by default, so the DDL is accepted).
        // `describe_table` on the missing parent fails; the edge must still be
        // reported — degraded to `rowid` — rather than aborting the whole call.
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .query("CREATE TABLE child (id INTEGER PRIMARY KEY, ghost_id INTEGER REFERENCES ghost)")
            .await
            .unwrap();
        let fks = adapter
            .foreign_keys(&TableInfo::unqualified("child"))
            .await
            .unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].referenced_table, TableInfo::unqualified("ghost"));
        assert_eq!(fks[0].referenced_columns, vec!["rowid".to_owned()]);
    }

    #[tokio::test]
    async fn foreign_keys_is_empty_for_a_table_without_references() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .query("CREATE TABLE solo (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        let fks = adapter
            .foreign_keys(&TableInfo::unqualified("solo"))
            .await
            .unwrap();
        assert!(fks.is_empty());
    }

    #[tokio::test]
    async fn foreign_keys_reports_multiple_distinct_references() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .query("CREATE TABLE a (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        adapter
            .query("CREATE TABLE b (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        adapter
            .query(
                "CREATE TABLE j (id INTEGER PRIMARY KEY, \
                 a_id INTEGER REFERENCES a(id), \
                 b_id INTEGER REFERENCES b(id))",
            )
            .await
            .unwrap();
        let fks = adapter
            .foreign_keys(&TableInfo::unqualified("j"))
            .await
            .unwrap();
        assert_eq!(fks.len(), 2);
        let mut targets: Vec<&str> = fks
            .iter()
            .map(|f| f.referenced_table.name.as_str())
            .collect();
        targets.sort_unstable();
        assert_eq!(targets, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn execute_runs_ddl_and_dml_and_reports_rows_affected() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        let affected = adapter
            .execute("CREATE TABLE t (id INTEGER PRIMARY KEY, label TEXT)")
            .await
            .unwrap();
        assert_eq!(affected, 0, "DDL affects no rows");

        let inserted = adapter
            .execute("INSERT INTO t (id, label) VALUES (1, 'a'), (2, 'b')")
            .await
            .unwrap();
        assert_eq!(inserted, 2);
        assert_eq!(row_count(&adapter).await, 2);
    }

    #[tokio::test]
    async fn execute_in_transaction_commits_all_statements() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter
            .execute_in_transaction(&[
                "CREATE TABLE t (id INTEGER PRIMARY KEY, label TEXT)".to_owned(),
                "INSERT INTO t (id, label) VALUES (1, 'a')".to_owned(),
                "INSERT INTO t (id, label) VALUES (2, 'b')".to_owned(),
            ])
            .await
            .unwrap();
        assert_eq!(row_count(&adapter).await, 2);
    }

    #[tokio::test]
    async fn execute_in_transaction_rolls_back_on_a_failed_statement() {
        let adapter = seeded(0).await; // table t exists, empty
        let err = adapter
            .execute_in_transaction(&[
                "INSERT INTO t (id, label) VALUES (1, 'a')".to_owned(),
                // Duplicate primary key: fails, and the whole batch unwinds.
                "INSERT INTO t (id, label) VALUES (1, 'dup')".to_owned(),
            ])
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        // Nothing committed: the first insert was rolled back with the second.
        assert_eq!(row_count(&adapter).await, 0);
        // The connection is not stuck mid-transaction — a later write succeeds.
        adapter
            .execute("INSERT INTO t (id, label) VALUES (9, 'ok')")
            .await
            .unwrap();
        assert_eq!(row_count(&adapter).await, 1);
    }

    #[tokio::test]
    async fn execute_in_transaction_treats_an_empty_batch_as_a_noop() {
        let adapter = TursoAdapter::connect_local(":memory:").await.unwrap();
        adapter.execute_in_transaction(&[]).await.unwrap();
        // The connection is still usable (no dangling BEGIN).
        adapter
            .execute("CREATE TABLE t (id INTEGER)")
            .await
            .unwrap();
    }
}
