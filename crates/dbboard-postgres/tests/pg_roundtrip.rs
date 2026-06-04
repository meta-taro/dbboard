//! Live round-trip test against a real PostgreSQL-wire database
//! (`CockroachDB` Cloud, a self-hosted node, or any Postgres).
//!
//! Network-bound, so it is gated behind an environment variable (see
//! `docs/architecture.md`): it self-skips unless `DBBOARD_PG_URL` is set.
//! With it set it exercises the full
//! `connect → ping → DDL → DML → SELECT → list_tables` path and asserts
//! the text-format value mapping (every value comes back as `Value::Text`,
//! NULL as `Value::Null`).

use dbboard_core::{DatabaseAdapter, Value};
use dbboard_postgres::{PostgresAdapter, PostgresConfig};

fn config_from_env() -> Option<PostgresConfig> {
    Some(PostgresConfig {
        url: std::env::var("DBBOARD_PG_URL").ok()?,
    })
}

#[tokio::test]
async fn select_one_round_trips() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };

    let adapter = PostgresAdapter::connect(config).await.expect("connect");
    adapter.ping().await.expect("ping");

    let result = adapter.query("SELECT 1 AS one").await.expect("query");
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.columns[0].name, "one");
    assert_eq!(result.rows.len(), 1);
    // Text protocol: the integer arrives as its textual representation.
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

#[tokio::test]
async fn dml_and_select_round_trip() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };

    let adapter = PostgresAdapter::connect(config).await.expect("connect");

    // Unique name so concurrent / repeated runs don't collide.
    let table = format!("dbboard_pg_it_{}", std::process::id());
    let drop_sql = format!("DROP TABLE IF EXISTS {table}");

    adapter.query(&drop_sql).await.expect("pre-drop");
    adapter
        .query(&format!(
            "CREATE TABLE {table} (id INT PRIMARY KEY, name TEXT)"
        ))
        .await
        .expect("create");

    let inserted = adapter
        .query(&format!(
            "INSERT INTO {table} (id, name) VALUES (1, 'alice'), (2, NULL)"
        ))
        .await
        .expect("insert");
    assert_eq!(inserted.rows_affected, 2);
    assert!(inserted.rows.is_empty());

    let selected = adapter
        .query(&format!("SELECT id, name FROM {table} ORDER BY id"))
        .await
        .expect("select");
    assert_eq!(selected.rows.len(), 2);
    assert_eq!(selected.rows[0].get(0), Some(&Value::Text("1".to_string())));
    assert_eq!(
        selected.rows[0].get(1),
        Some(&Value::Text("alice".to_string()))
    );
    // NULL stays NULL rather than the string "NULL".
    assert_eq!(selected.rows[1].get(1), Some(&Value::Null));

    // The new table shows up in introspection.
    let tables = adapter.list_tables().await.expect("list tables");
    assert!(
        tables.iter().any(|t| t.name == table),
        "created table {table} not found in {tables:?}"
    );

    adapter.query(&drop_sql).await.expect("cleanup drop");
}

/// Exactly at the row cap: `generate_series(1, MAX_RESULT_ROWS)` returns
/// `MAX_RESULT_ROWS` rows and must succeed.
#[tokio::test]
async fn query_at_the_row_cap_returns_all_rows() {
    use dbboard_core::MAX_RESULT_ROWS;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };

    let adapter = PostgresAdapter::connect(config).await.expect("connect");
    let sql = format!("SELECT n FROM generate_series(1, {MAX_RESULT_ROWS}) AS s(n)");
    let result = adapter.query(&sql).await.expect("query at cap");
    assert_eq!(result.rows.len(), MAX_RESULT_ROWS);
}

/// Neon round-trip: same wire protocol as Postgres, but
/// `connect_neon` flips the runtime adapter id to `"neon"` (ADR-0018).
/// Gated on its own env var so the `DBBOARD_PG_URL` test can stay
/// pointed at `CockroachDB` / vanilla Postgres while this one targets a
/// real Neon endpoint. Neon enforces TLS — the URL must include
/// `sslmode=require`.
#[tokio::test]
async fn neon_round_trip_reports_neon_flavor() {
    let Some(url) = std::env::var("DBBOARD_NEON_URL").ok() else {
        eprintln!("skipping: DBBOARD_NEON_URL not set");
        return;
    };
    let adapter = PostgresAdapter::connect_neon(PostgresConfig { url })
        .await
        .expect("connect_neon");
    adapter.ping().await.expect("ping");
    assert_eq!(
        adapter.id(),
        "neon",
        "connect_neon must surface the neon flavor at runtime"
    );

    let result = adapter.query("SELECT 1 AS one").await.expect("query");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

/// Supabase round-trip: same wire protocol as Postgres, but
/// `connect_supabase` flips the runtime adapter id to `"supabase"`
/// (ADR-0019). Gated on its own env var; both the direct `:5432` host
/// and the transaction-pooler `:6543` host satisfy this test — the URL
/// itself picks. Supabase enforces TLS, so the URL must include
/// `sslmode=require`.
#[tokio::test]
async fn supabase_round_trip_reports_supabase_flavor() {
    let Some(url) = std::env::var("DBBOARD_SUPABASE_URL").ok() else {
        eprintln!("skipping: DBBOARD_SUPABASE_URL not set");
        return;
    };
    let adapter = PostgresAdapter::connect_supabase(PostgresConfig { url })
        .await
        .expect("connect_supabase");
    adapter.ping().await.expect("ping");
    assert_eq!(
        adapter.id(),
        "supabase",
        "connect_supabase must surface the supabase flavor at runtime"
    );

    let result = adapter.query("SELECT 1 AS one").await.expect("query");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

/// Aurora DSQL round-trip: same wire protocol as Postgres, but
/// `connect_aurora_dsql` flips the runtime adapter id to `"aurora-dsql"`
/// (ADR-0021). Gated on its own env var so the other pg-wire round-trips
/// keep pointing at their respective backends. Aurora DSQL enforces TLS
/// and IAM auth — the URL must include `sslmode=require` and a fresh
/// short-lived IAM authentication token in the password segment
/// (~15 min TTL). An expired token surfaces as `DbError::Connection`
/// at `connect`/`ping` time.
#[tokio::test]
async fn aurora_dsql_round_trip_reports_aurora_dsql_flavor() {
    let Some(url) = std::env::var("DBBOARD_AURORA_DSQL_URL").ok() else {
        eprintln!("skipping: DBBOARD_AURORA_DSQL_URL not set");
        return;
    };
    let adapter = PostgresAdapter::connect_aurora_dsql(PostgresConfig { url })
        .await
        .expect("connect_aurora_dsql");
    adapter.ping().await.expect("ping");
    assert_eq!(
        adapter.id(),
        "aurora-dsql",
        "connect_aurora_dsql must surface the aurora-dsql flavor at runtime"
    );

    let result = adapter.query("SELECT 1 AS one").await.expect("query");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

/// One row past the cap must surface as `DbError::Query` rather than a
/// truncated result. The Postgres adapter streams rows, so the check
/// fires mid-stream once `MAX_RESULT_ROWS` rows have been buffered.
#[tokio::test]
async fn query_over_the_row_cap_is_a_query_error() {
    use dbboard_core::MAX_RESULT_ROWS;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };

    let adapter = PostgresAdapter::connect(config).await.expect("connect");
    let over = MAX_RESULT_ROWS + 1;
    let sql = format!("SELECT n FROM generate_series(1, {over}) AS s(n)");
    let Err(err) = adapter.query(&sql).await else {
        panic!("query over cap should fail");
    };
    let dbboard_core::DbError::Query(msg) = err else {
        panic!("expected DbError::Query, got {err:?}");
    };
    assert!(
        msg.contains(&MAX_RESULT_ROWS.to_string()),
        "error should mention the cap, got: {msg}"
    );
}
