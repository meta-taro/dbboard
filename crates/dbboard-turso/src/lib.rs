//! Turso / libSQL adapter for dbboard.
//!
//! Wraps the official `libsql` crate so the rest of dbboard sees only
//! the domain types from `dbboard-core`. The adapter does not yet
//! implement a workspace-wide trait — Phase 2 of the roadmap extracts
//! that trait once the second adapter (Neon) gives it a real second
//! shape to honour.

use dbboard_core::{Column, DbError, DbResult, QueryResult, Row, TableInfo, Value};

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
        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;
        let conn = db
            .connect()
            .map_err(|e| DbError::Connection(e.to_string()))?;
        Ok(Self { conn, _db: db })
    }

    /// Cheap liveness probe: runs `SELECT 1` and discards the row.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Query`] when the probe statement fails to
    /// execute on the live connection.
    pub async fn ping(&self) -> DbResult<()> {
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

    /// List user tables (i.e. anything in `sqlite_master` that is not
    /// a `sqlite_*` internal table). Names are returned in ascending
    /// lexicographic order.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Schema`] when the underlying introspection
    /// query fails.
    pub async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
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

    /// Execute a SQL statement and collect every row into memory.
    ///
    /// Internally dispatches to libSQL's row-returning path for
    /// `SELECT`-shaped statements and to the affected-rows path for
    /// `INSERT`/`UPDATE`/`DELETE`/DDL, since libSQL exposes those as
    /// distinct entry points and rejects a statement sent through
    /// the wrong one.
    ///
    /// Phase 1 only targets the small result sets a developer pastes
    /// into the SQL editor; streaming and pagination land later.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Query`] or [`DbError::TypeConversion`]
    /// depending on the failure mode.
    pub async fn query(&self, sql: &str) -> DbResult<QueryResult> {
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
/// check is intentionally shallow (first non-empty, non-comment word
/// only) — enough to route Phase 1 traffic through the right libSQL
/// entry point without dragging in a SQL parser.
fn is_row_returning(sql: &str) -> bool {
    let first_word = sql
        .lines()
        .map(str::trim_start)
        .find(|line| !line.is_empty() && !line.starts_with("--"))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("");
    matches!(
        first_word.to_ascii_uppercase().as_str(),
        "SELECT" | "WITH" | "VALUES" | "PRAGMA" | "EXPLAIN"
    )
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
    use super::{convert_value, is_row_returning, Value};

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
}
