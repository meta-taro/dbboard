//! Live round-trip test against a real Cloudflare D1 database.
//!
//! Network-bound, so it is gated behind environment variables (see
//! `docs/architecture.md`): it self-skips unless `DBBOARD_D1_ACCOUNT_ID`,
//! `DBBOARD_D1_DATABASE_ID`, and `DBBOARD_D1_TOKEN` are all set. With
//! them set it exercises the full `connect → ping → query` path.

use dbboard_core::Value;
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
