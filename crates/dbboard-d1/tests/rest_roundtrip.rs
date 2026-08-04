//! Live round-trip test against a real Cloudflare D1 database.
//!
//! Network-bound, so it is gated behind environment variables (see
//! `docs/architecture.md`): it self-skips unless `DBBOARD_D1_ACCOUNT_ID`,
//! `DBBOARD_D1_DATABASE_ID`, and `DBBOARD_D1_TOKEN` are all set. With
//! them set it exercises the full `connect → ping → query` path.

use dbboard_core::{DatabaseAdapter, Value};
use dbboard_d1::{D1Adapter, D1Config};

fn config_from_env() -> Option<D1Config> {
    Some(D1Config {
        account_id: std::env::var("DBBOARD_D1_ACCOUNT_ID").ok()?,
        database_id: std::env::var("DBBOARD_D1_DATABASE_ID").ok()?,
        api_token: std::env::var("DBBOARD_D1_TOKEN").ok()?,
        base_url: std::env::var("DBBOARD_D1_BASE_URL").ok(),
    })
}

#[tokio::test]
async fn select_one_round_trips() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_D1_* env vars not set");
        return;
    };

    let adapter = D1Adapter::connect(config).expect("build adapter");
    adapter.ping().await.expect("ping");

    let result = adapter.query("SELECT 1 AS one").await.expect("query");
    assert_eq!(result.columns.len(), 1);
    assert_eq!(result.columns[0].name, "one");
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get(0), Some(&Value::Integer(1)));
}

#[tokio::test]
async fn list_tables_round_trips() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_D1_* env vars not set");
        return;
    };

    let adapter = D1Adapter::connect(config).expect("build adapter");
    // Should succeed even on an empty database (sqlite_master is always
    // present); we only assert it does not error.
    adapter.list_tables().await.expect("list tables");
}

/// `foreign_keys` round-trip (ADR-0054): a child table with an explicit
/// single-column reference and an implicit composite reference (parent
/// column list omitted) reports both edges, the implicit one resolved to
/// the parent's primary key in key order.
#[tokio::test]
async fn foreign_keys_round_trip_reports_edges() {
    use dbboard_core::TableInfo;
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_D1_* env vars not set");
        return;
    };

    let adapter = D1Adapter::connect(config).expect("build adapter");

    // Unique-per-process names so repeated runs don't collide. D1 has no
    // multi-statement batch over /raw, so each statement is its own call.
    let pid = std::process::id();
    let parent = format!("dbboard_d1_fk_parent_{pid}");
    let composite = format!("dbboard_d1_fk_composite_{pid}");
    let child = format!("dbboard_d1_fk_child_{pid}");
    for stmt in [
        format!("DROP TABLE IF EXISTS {child}"),
        format!("DROP TABLE IF EXISTS {composite}"),
        format!("DROP TABLE IF EXISTS {parent}"),
        format!("CREATE TABLE {parent} (id INTEGER PRIMARY KEY)"),
        format!("CREATE TABLE {composite} (a INTEGER, b INTEGER, PRIMARY KEY (a, b))"),
        // `parent_id` names the parent column; `(ca, cb)` omits it, so the
        // reference is implicitly to the composite parent's primary key.
        format!(
            "CREATE TABLE {child} (\
             id INTEGER PRIMARY KEY, \
             parent_id INTEGER REFERENCES {parent}(id), \
             ca INTEGER, cb INTEGER, \
             FOREIGN KEY (ca, cb) REFERENCES {composite})"
        ),
    ] {
        adapter.query(&stmt).await.expect("setup ddl");
    }

    let edges = adapter
        .foreign_keys(&TableInfo::unqualified(&child))
        .await
        .expect("foreign_keys");
    assert_eq!(edges.len(), 2, "expected two edges, got {edges:?}");

    let single = edges
        .iter()
        .find(|e| e.columns == vec!["parent_id".to_string()])
        .expect("single-column edge");
    assert_eq!(single.referenced_table, TableInfo::unqualified(&parent));
    assert_eq!(single.referenced_columns, vec!["id".to_string()]);

    let comp = edges
        .iter()
        .find(|e| e.columns == vec!["ca".to_string(), "cb".to_string()])
        .expect("composite edge");
    assert_eq!(comp.referenced_table, TableInfo::unqualified(&composite));
    // Implicit reference resolved against the parent's composite PK.
    assert_eq!(
        comp.referenced_columns,
        vec!["a".to_string(), "b".to_string()]
    );

    for stmt in [
        format!("DROP TABLE IF EXISTS {child}"),
        format!("DROP TABLE IF EXISTS {composite}"),
        format!("DROP TABLE IF EXISTS {parent}"),
    ] {
        adapter.query(&stmt).await.expect("cleanup drop");
    }
}

/// `https_only(true)` rejects an `http://` base URL at request time. The
/// reqwest error embeds the URL — which would expose the account and
/// database IDs — so this verifies `transport_error` scrubs it.
#[tokio::test]
async fn http_base_url_failure_does_not_leak_the_url() {
    let secret_account = "claude-d1-acc-leak-marker";
    let secret_database = "claude-d1-db-leak-marker";
    let config = D1Config {
        account_id: secret_account.to_string(),
        database_id: secret_database.to_string(),
        api_token: "irrelevant-but-non-empty".to_string(),
        // Plain http:// is rejected by `https_only(true)` set in
        // `D1Adapter::connect`, so this fires the transport error path.
        base_url: Some("http://leak-marker.invalid".to_string()),
    };
    // `D1Adapter` is not `Debug`, so unwrap_err / expect_err are off
    // the table — drive the match by hand.
    let adapter = D1Adapter::connect(config).expect("build adapter");
    let err = match adapter.ping().await {
        Ok(()) => panic!("http:// base URL must be rejected"),
        Err(e) => e,
    };
    let dbboard_core::DbError::Connection(msg) = err else {
        panic!("expected DbError::Connection, got {err:?}");
    };
    assert!(
        !msg.contains("leak-marker.invalid"),
        "URL host leaked into error: {msg}"
    );
    assert!(
        !msg.contains(secret_account),
        "account id leaked into error: {msg}"
    );
    assert!(
        !msg.contains(secret_database),
        "database id leaked into error: {msg}"
    );
}
