//! End-to-end smoke tests against an in-memory libSQL database.
//!
//! These cover the "connect → introspect → query" path that Phase 1
//! promises in the roadmap, without requiring any remote Turso
//! credentials or temp files on disk.

use dbboard_core::{DatabaseAdapter, Value};
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
