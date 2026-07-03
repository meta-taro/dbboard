//! End-to-end smoke tests against an in-memory libSQL database.
//!
//! These cover the "connect → introspect → query" path that Phase 1
//! promises in the roadmap, without requiring any remote Turso
//! credentials or temp files on disk.

use dbboard_core::{DatabaseAdapter, DbError, TableInfo, Value};
use dbboard_turso::TursoAdapter;

async fn fresh_db() -> TursoAdapter {
    TursoAdapter::connect_local(":memory:")
        .await
        .expect("connect to in-memory libSQL")
}

#[tokio::test]
async fn ping_succeeds_on_fresh_in_memory_db() {
    let adapter = fresh_db().await;
    adapter.ping().await.expect("ping should succeed");
}

#[tokio::test]
async fn list_tables_returns_empty_for_fresh_db() {
    let adapter = fresh_db().await;
    let tables = adapter.list_tables().await.expect("list_tables");
    assert!(
        tables.is_empty(),
        "fresh in-memory DB should have no user tables, got {tables:?}"
    );
}

#[tokio::test]
async fn list_tables_returns_user_tables_in_alphabetical_order() {
    let adapter = fresh_db().await;
    adapter
        .query("CREATE TABLE zebras (id INTEGER PRIMARY KEY)")
        .await
        .expect("create zebras");
    adapter
        .query("CREATE TABLE apples (id INTEGER PRIMARY KEY)")
        .await
        .expect("create apples");

    let tables = adapter.list_tables().await.expect("list_tables");
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["apples", "zebras"]);
}

#[tokio::test]
async fn query_returns_column_names_and_typed_rows() {
    let adapter = fresh_db().await;
    adapter
        .query("CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT)")
        .await
        .expect("create users");
    adapter
        .query("INSERT INTO users (id, email) VALUES (1, 'a@example.com')")
        .await
        .expect("insert users");

    let result = adapter
        .query("SELECT id, email FROM users ORDER BY id")
        .await
        .expect("select users");

    let column_names: Vec<&str> = result.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(column_names, vec!["id", "email"]);

    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.get(0), Some(&Value::Integer(1)));
    assert_eq!(row.get(1), Some(&Value::Text("a@example.com".into())));
}

#[tokio::test]
async fn query_surfaces_sql_errors_as_query_category() {
    let adapter = fresh_db().await;
    let err = adapter
        .query("SELEC bogus")
        .await
        .expect_err("malformed SQL should fail");
    assert!(
        matches!(err, dbboard_core::DbError::Query(_)),
        "expected DbError::Query, got {err:?}",
    );
}

/// Exactly at the row cap: a recursive CTE that produces
/// `MAX_RESULT_ROWS` rows must succeed and return them all.
#[tokio::test]
async fn query_at_the_row_cap_returns_all_rows() {
    use dbboard_core::MAX_RESULT_ROWS;
    let adapter = fresh_db().await;

    // `WHERE n < N` with the base case `SELECT 1` produces 1..=N rows.
    let sql = format!(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < {MAX_RESULT_ROWS}) SELECT n FROM seq",
    );
    let result = adapter
        .query(&sql)
        .await
        .expect("query at cap should succeed");
    assert_eq!(result.rows.len(), MAX_RESULT_ROWS);
}

/// One row past the cap must surface as `DbError::Query` (HTTP 400),
/// not a truncated result that the UI would silently render.
#[tokio::test]
async fn query_over_the_row_cap_is_a_query_error() {
    use dbboard_core::MAX_RESULT_ROWS;
    let adapter = fresh_db().await;

    let over = MAX_RESULT_ROWS + 1;
    let sql = format!(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < {over}) SELECT n FROM seq",
    );
    let err = adapter
        .query(&sql)
        .await
        .expect_err("query over cap should fail");
    let dbboard_core::DbError::Query(msg) = err else {
        panic!("expected DbError::Query, got {err:?}");
    };
    assert!(
        msg.contains(&MAX_RESULT_ROWS.to_string()),
        "error should mention the cap, got: {msg}"
    );
}

#[tokio::test]
async fn capabilities_advertise_describe_table() {
    let adapter = fresh_db().await;
    assert!(adapter.capabilities().has_describe_table);
}

#[tokio::test]
async fn describe_table_reports_columns_in_ordinal_order() {
    let adapter = fresh_db().await;
    adapter
        .query(
            "CREATE TABLE users (\
             id INTEGER PRIMARY KEY, \
             email TEXT NOT NULL DEFAULT 'nobody@example.com', \
             note TEXT)",
        )
        .await
        .expect("create users");

    let schema = adapter
        .describe_table(&TableInfo::unqualified("users"))
        .await
        .expect("describe users");

    assert_eq!(schema.table, TableInfo::unqualified("users"));
    let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["id", "email", "note"]);
    // SQLite's PRAGMA cid is 0-based; the adapter normalises to 1-based.
    let ordinals: Vec<u32> = schema.columns.iter().map(|c| c.ordinal).collect();
    assert_eq!(ordinals, vec![1, 2, 3]);

    let id = &schema.columns[0];
    assert_eq!(id.declared_type.as_deref(), Some("INTEGER"));
    assert!(id.primary_key);
    assert_eq!(id.default_value, None);

    let email = &schema.columns[1];
    assert!(!email.nullable);
    assert!(!email.primary_key);
    // SQLite reports the default as the literal DDL expression text,
    // quotes included.
    assert_eq!(email.default_value.as_deref(), Some("'nobody@example.com'"));

    let note = &schema.columns[2];
    assert!(note.nullable);
    assert_eq!(note.default_value, None);

    assert_eq!(schema.primary_key, vec!["id".to_owned()]);
}

#[tokio::test]
async fn describe_table_materialises_composite_pk_in_key_order() {
    let adapter = fresh_db().await;
    adapter
        .query(
            "CREATE TABLE order_items (\
             sku TEXT, \
             order_id INTEGER, \
             line_no INTEGER, \
             PRIMARY KEY (order_id, line_no))",
        )
        .await
        .expect("create order_items");

    let schema = adapter
        .describe_table(&TableInfo::unqualified("order_items"))
        .await
        .expect("describe order_items");

    // Key order (order_id, line_no), not column declaration order.
    assert_eq!(
        schema.primary_key,
        vec!["order_id".to_owned(), "line_no".to_owned()]
    );
    let pk_flags: Vec<bool> = schema.columns.iter().map(|c| c.primary_key).collect();
    assert_eq!(pk_flags, vec![false, true, true]);
}

#[tokio::test]
async fn describe_table_missing_table_is_a_query_error() {
    let adapter = fresh_db().await;
    let err = adapter
        .describe_table(&TableInfo::unqualified("ghost"))
        .await
        .expect_err("describing a missing table should fail");
    let DbError::Query(msg) = err else {
        panic!("expected DbError::Query, got {err:?}");
    };
    assert!(
        msg.contains("no such table") && msg.contains("ghost"),
        "unexpected message: {msg}"
    );
}

/// A single quote in the table name must not break out of the PRAGMA
/// argument (escaping guard — the name comes back from `list_tables`
/// but could be attacker-influenced via a hostile schema).
#[tokio::test]
async fn describe_table_handles_quoted_table_names() {
    let adapter = fresh_db().await;
    adapter
        .query("CREATE TABLE \"we'ird\" (id INTEGER PRIMARY KEY)")
        .await
        .expect("create we'ird");

    let schema = adapter
        .describe_table(&TableInfo::unqualified("we'ird"))
        .await
        .expect("describe we'ird");
    assert_eq!(schema.columns.len(), 1);
    assert_eq!(schema.primary_key, vec!["id".to_owned()]);
}

/// Opening a path that does not exist must not echo the path back in
/// the error message (it would otherwise leak directory layout into the
/// HTTP error envelope).
#[tokio::test]
async fn connect_failure_does_not_leak_the_supplied_path() {
    // A nested non-existent directory is the simplest way to force
    // libsql's "unable to open" path on every platform.
    let secret = "claude-test-marker-DO-NOT-LOG";
    let path = format!("./nonexistent-dir/{secret}/dbboard.sqlite");
    // `TursoAdapter` is not `Debug`, so unwrap_err / expect_err are off
    // the table — pattern-match via let-else.
    let Err(err) = TursoAdapter::connect_local(&path).await else {
        panic!("opening a path under a missing directory should fail");
    };
    let dbboard_core::DbError::Connection(msg) = err else {
        panic!("expected DbError::Connection, got {err:?}");
    };
    assert!(
        !msg.contains(secret),
        "supplied path leaked into error message: {msg}"
    );
}
