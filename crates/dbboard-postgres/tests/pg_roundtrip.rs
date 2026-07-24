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

/// `describe_table` round-trip: columns arrive in ordinal order with
/// nullability, defaults, and the composite primary key in key order
/// (ADR-0028). Missing tables surface as `DbError::Query`.
#[tokio::test]
async fn describe_table_round_trips_columns_and_composite_pk() {
    use dbboard_core::TableInfo;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };

    let adapter = PostgresAdapter::connect(config).await.expect("connect");

    let table = format!("dbboard_pg_describe_{}", std::process::id());
    let drop_sql = format!("DROP TABLE IF EXISTS {table}");
    adapter.query(&drop_sql).await.expect("pre-drop");
    adapter
        .query(&format!(
            "CREATE TABLE {table} (\
             order_id INT, \
             line_no INT, \
             sku TEXT NOT NULL DEFAULT 'unknown', \
             PRIMARY KEY (order_id, line_no))"
        ))
        .await
        .expect("create");

    let info = TableInfo::qualified("public", &table);
    let schema = adapter.describe_table(&info).await.expect("describe");

    assert_eq!(schema.table, info);
    let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["order_id", "line_no", "sku"]);
    let ordinals: Vec<u32> = schema.columns.iter().map(|c| c.ordinal).collect();
    assert_eq!(ordinals, vec![1, 2, 3]);
    assert_eq!(
        schema.primary_key,
        vec!["order_id".to_owned(), "line_no".to_owned()]
    );

    let sku = &schema.columns[2];
    assert!(!sku.nullable);
    assert!(!sku.primary_key);
    assert!(
        sku.default_value
            .as_deref()
            .is_some_and(|d| d.contains("unknown")),
        "expected a default mentioning 'unknown', got {:?}",
        sku.default_value
    );

    adapter.query(&drop_sql).await.expect("cleanup drop");

    let err = adapter
        .describe_table(&TableInfo::qualified("public", &table))
        .await
        .expect_err("describing a dropped table should fail");
    assert!(
        matches!(err, dbboard_core::DbError::Query(_)),
        "expected DbError::Query, got {err:?}"
    );
}

/// `foreign_keys` round-trip (ADR-0054): a child table with a single-column
/// and a composite reference reports both, with local/referenced columns
/// aligned in key order and the constraint name preserved. A table without
/// references reports none.
#[tokio::test]
async fn foreign_keys_round_trip_reports_single_and_composite_edges() {
    use dbboard_core::TableInfo;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };

    let adapter = PostgresAdapter::connect(config).await.expect("connect");

    let pid = std::process::id();
    let parent = format!("dbboard_pg_fk_parent_{pid}");
    let composite = format!("dbboard_pg_fk_composite_{pid}");
    let child = format!("dbboard_pg_fk_child_{pid}");
    // Drop children before parents to satisfy referential order.
    let drop_all = format!(
        "DROP TABLE IF EXISTS {child}; DROP TABLE IF EXISTS {composite}; \
         DROP TABLE IF EXISTS {parent}"
    );

    // Statements run one at a time — the read-only batch guard only applies
    // to the query path, and `query` here drives plain DDL sequentially.
    for stmt in [
        format!("DROP TABLE IF EXISTS {child}"),
        format!("DROP TABLE IF EXISTS {composite}"),
        format!("DROP TABLE IF EXISTS {parent}"),
        format!("CREATE TABLE {parent} (id INT PRIMARY KEY)"),
        format!("CREATE TABLE {composite} (a INT, b INT, PRIMARY KEY (a, b))"),
        format!(
            "CREATE TABLE {child} (\
             id INT PRIMARY KEY, \
             parent_id INT REFERENCES {parent} (id), \
             ca INT, cb INT, \
             CONSTRAINT {child}_composite_fk FOREIGN KEY (ca, cb) \
             REFERENCES {composite} (a, b))"
        ),
    ] {
        adapter.query(&stmt).await.expect("setup ddl");
    }

    let edges = adapter
        .foreign_keys(&TableInfo::qualified("public", &child))
        .await
        .expect("foreign_keys");
    assert_eq!(edges.len(), 2, "expected two edges, got {edges:?}");

    let single = edges
        .iter()
        .find(|e| e.columns == vec!["parent_id".to_string()])
        .expect("single-column edge");
    assert_eq!(
        single.referenced_table,
        TableInfo::qualified("public", &parent)
    );
    assert_eq!(single.referenced_columns, vec!["id".to_string()]);

    let comp = edges
        .iter()
        .find(|e| e.columns == vec!["ca".to_string(), "cb".to_string()])
        .expect("composite edge");
    assert_eq!(
        comp.referenced_table,
        TableInfo::qualified("public", &composite)
    );
    assert_eq!(
        comp.referenced_columns,
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(
        comp.constraint_name.as_deref(),
        Some(format!("{child}_composite_fk").as_str())
    );

    // A table with no outbound references reports none.
    let parent_edges = adapter
        .foreign_keys(&TableInfo::qualified("public", &parent))
        .await
        .expect("foreign_keys on parent");
    assert!(
        parent_edges.is_empty(),
        "parent has no FKs: {parent_edges:?}"
    );

    adapter.query(&drop_all).await.expect("cleanup drop");
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

/// `query_read_only` caps by *truncating*, not erroring (ADR-0046): a
/// 100-row series with `max_rows = 10` comes back with exactly 10 rows.
/// This is the opposite of `query`'s hard `MAX_RESULT_ROWS` error, since
/// the MCP surface must degrade gracefully for an agent.
#[tokio::test]
async fn read_only_query_truncates_to_max_rows() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };
    let adapter = PostgresAdapter::connect(config).await.expect("connect");
    let sql = "SELECT n FROM generate_series(1, 100) AS s(n) ORDER BY n";
    let result = adapter.query_read_only(sql, 10).await.expect("read-only");
    assert_eq!(result.rows.len(), 10);
    assert_eq!(result.rows[0].get(0), Some(&Value::Text("1".to_string())));
}

/// The engine backstop, not the classifier: `nextval()` is a *write*
/// (it advances a sequence) wrapped in a `SELECT`, so the AST classifier
/// waves it through as read-only — but `BEGIN READ ONLY` makes Postgres
/// itself reject it. This proves the read-only guarantee does not rest on
/// string/AST matching alone (ADR-0046 Decision 6).
#[tokio::test]
async fn read_only_rejects_nextval_at_the_engine() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };
    let adapter = PostgresAdapter::connect(config).await.expect("connect");

    let seq = format!("dbboard_pg_ro_seq_{}", std::process::id());
    adapter
        .query(&format!("DROP SEQUENCE IF EXISTS {seq}"))
        .await
        .expect("pre-drop");
    adapter
        .query(&format!("CREATE SEQUENCE {seq}"))
        .await
        .expect("create seq");

    // Classifier sees a plain SELECT and allows it; the read-only txn
    // must still refuse the sequence advance.
    let err = adapter
        .query_read_only(&format!("SELECT nextval('{seq}')"), 100)
        .await
        .expect_err("nextval must be rejected by the read-only transaction");
    assert!(
        matches!(err, dbboard_core::DbError::Query(_)),
        "expected DbError::Query from the engine, got {err:?}"
    );

    adapter
        .query(&format!("DROP SEQUENCE IF EXISTS {seq}"))
        .await
        .expect("cleanup drop");
}

/// EXPLAIN is a utility statement that cannot back a cursor, so
/// `query_read_only` runs it on the direct (non-cursor) branch. It must
/// still succeed and return the plan text rows.
#[tokio::test]
async fn read_only_explain_returns_a_plan() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_PG_URL not set");
        return;
    };
    let adapter = PostgresAdapter::connect(config).await.expect("connect");
    let result = adapter
        .query_read_only("EXPLAIN SELECT 1", 100)
        .await
        .expect("explain");
    assert!(!result.rows.is_empty(), "EXPLAIN should return plan rows");
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
