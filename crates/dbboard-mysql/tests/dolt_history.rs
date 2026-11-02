//! Live test of the history views against a real Dolt server (ADR-0165).
//!
//! Self-skips unless `DBBOARD_DOLT_URL` is set, like the other live tests.
//! Point it at a throwaway Dolt database: the test creates a table, commits
//! three times with `DOLT_COMMIT` (Dolt reserves table names that start with
//! `dolt_`, hence the `history_it_` prefix), then reads the history back through the
//! adapter's read-only path.
//!
//! ```sh
//! dolt sql-server --port 13306 &   # in an empty `dolt init` directory
//! DBBOARD_DOLT_URL='mysql://root@127.0.0.1:13306/<db>?ssl-mode=disabled' \
//!   cargo test -p dbboard-mysql --test dolt_history
//! ```

use dbboard_core::{ChangeKind, DatabaseAdapter, TableInfo, Value};
use dbboard_mysql::{MySqlAdapter, MySqlConfig};

fn config_from_env() -> Option<MySqlConfig> {
    Some(MySqlConfig {
        url: std::env::var("DBBOARD_DOLT_URL").ok()?,
    })
}

async fn run(adapter: &MySqlAdapter, sql: &str) {
    adapter
        .query(sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

#[tokio::test]
async fn dolt_history_reads_back_what_was_committed() {
    let Some(config) = config_from_env() else {
        eprintln!("skipping: DBBOARD_DOLT_URL not set");
        return;
    };
    let adapter = MySqlAdapter::connect(config).await.expect("connect");
    assert!(adapter.capabilities().has_history, "Dolt was not detected");
    let history = adapter.history().expect("a history reader");

    let name = format!("history_it_{}", std::process::id());
    run(
        &adapter,
        &format!("CREATE TABLE `{name}` (id INT PRIMARY KEY, qty INT, note VARCHAR(20))"),
    )
    .await;
    run(
        &adapter,
        &format!("INSERT INTO `{name}` VALUES (1, 3, 'a'), (2, 5, 'b')"),
    )
    .await;
    run(&adapter, "CALL DOLT_COMMIT('-Am', 'seed')").await;
    let seed = history.commit_log(1).await.expect("log")[0].hash.clone();
    run(
        &adapter,
        &format!("UPDATE `{name}` SET qty = 2 WHERE id = 1"),
    )
    .await;
    run(
        &adapter,
        &format!("INSERT INTO `{name}` VALUES (3, 1, 'c')"),
    )
    .await;
    run(&adapter, "CALL DOLT_COMMIT('-am', 'second')").await;
    run(&adapter, &format!("DELETE FROM `{name}` WHERE id = 2")).await;
    run(&adapter, "CALL DOLT_COMMIT('-am', 'third')").await;

    let log = history.commit_log(3).await.expect("log");
    let messages: Vec<&str> = log.iter().map(|c| c.message.as_str()).collect();
    assert_eq!(messages, ["third", "second", "seed"]);

    let table = TableInfo {
        schema: None,
        name: name.clone(),
    };

    // Row 1 changed in "second" only; "third" did not touch it.
    let row = history
        .row_history(&table, &[("id".to_string(), Value::Integer(1))], 10)
        .await
        .expect("row history");
    let changed_in: Vec<&str> = row
        .versions
        .iter()
        .map(|v| v.commit.message.as_str())
        .collect();
    assert_eq!(changed_in, ["second", "seed"]);

    let diff = history.diff(&table, &seed, "HEAD").await.expect("diff");
    let mut kinds: Vec<ChangeKind> = diff.changes.iter().map(|c| c.kind).collect();
    kinds.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(
        kinds,
        [ChangeKind::Added, ChangeKind::Modified, ChangeKind::Removed]
    );

    let blame = history.blame(&table).await.expect("blame");
    assert_eq!(blame.len(), 2, "rows 1 and 3 remain");

    // A ref that does not exist is an error, not an empty diff.
    assert!(history
        .diff(&table, "no-such-branch", "HEAD")
        .await
        .is_err());

    run(&adapter, &format!("DROP TABLE `{name}`")).await;
    run(&adapter, "CALL DOLT_COMMIT('-am', 'cleanup')").await;
}
