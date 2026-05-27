//! Turso / libSQL adapter for dbboard.
//!
//! Wraps the official `libsql` crate so the rest of dbboard sees only
//! the domain types from `dbboard-core`. Implements the workspace-wide
//! [`DatabaseAdapter`] contract (ADR-0012); Phase 2 advertises no
//! optional capabilities — Turso ships base catalog access only.

use async_trait::async_trait;
use dbboard_core::{
    too_many_rows_error, Capabilities, Column, DatabaseAdapter, DbError, DbResult, QueryResult,
    Row, TableInfo, Value, MAX_RESULT_ROWS,
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
}

#[async_trait]
impl DatabaseAdapter for TursoAdapter {
    fn id(&self) -> &'static str {
        "turso"
    }

    fn capabilities(&self) -> Capabilities {
        // Phase 2 ships base catalog access only; per-DB features land
        // alongside the adapters that need them.
        Capabilities::default()
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
    use super::{convert_value, is_row_returning, redact_path, Value};

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
}
