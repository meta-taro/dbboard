//! End-to-end smoke tests against an in-memory libSQL database.
//!
//! These cover the "connect → introspect → query" path that Phase 1
//! promises in the roadmap, without requiring any remote Turso
//! credentials or temp files on disk.

use dbboard_core::Value;
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
