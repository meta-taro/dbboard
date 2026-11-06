//! Live test of the history views against a real Umbrachyx server
//! (ADR-0170).
//!
//! Self-skips unless `DBBOARD_UMBRACHYX_URL` is set, like the other live
//! tests. Point it at a throwaway server serving a branch other than main:
//! the test writes with `SET`, through a plain client, because the adapter
//! itself cannot write, then reads the history back through the adapter.
//!
//! ```sh
//! umbrachyx branch agent --from main
//! umbrachyx serve agent --port 16399 &
//! DBBOARD_UMBRACHYX_URL=redis://127.0.0.1:16399 cargo test -p dbboard-redis --test live_umbrachyx
//! ```

use dbboard_core::{ChangeKind, DatabaseAdapter, TableInfo, Value};
use dbboard_redis::{RedisAdapter, RedisConfig};
use redis::AsyncCommands;

#[tokio::test]
async fn umbrachyx_history_reads_back_what_was_written() {
    let Some(url) = std::env::var("DBBOARD_UMBRACHYX_URL")
        .ok()
        .filter(|v| !v.is_empty())
    else {
        eprintln!("skipping: DBBOARD_UMBRACHYX_URL not set");
        return;
    };
    let prefix = format!("dbboard_it/{}", std::process::id());
    let key = format!("{prefix}/gems");
    let client = redis::Client::open(url.as_str()).expect("a redis url");
    let mut writer = client
        .get_multiplexed_async_connection()
        .await
        .expect("connects");
    for value in ["1", "2"] {
        let () = writer.set(&key, value).await.expect("SET");
    }

    let adapter = RedisAdapter::connect(RedisConfig {
        url,
        cluster: false,
        route: None,
    })
    .await
    .expect("connect");
    assert!(
        adapter.capabilities().has_history,
        "Umbrachyx was not detected"
    );
    let history = adapter.history().expect("a history reader");
    let table = TableInfo::unqualified("dbboard_it/*");

    let log = history.commit_log(2).await.expect("commit log");
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].message, "SET");

    let rows = history
        .row_history(&table, &[("key".into(), Value::Text(key.clone()))], 10)
        .await
        .expect("row history");
    let values: Vec<_> = rows
        .versions
        .iter()
        .map(|v| v.values.get(1).cloned())
        .collect();
    assert_eq!(
        values,
        [Some(Value::Text("2".into())), Some(Value::Text("1".into()))]
    );

    let diff = history
        .diff(&table, &log[1].hash, &log[0].hash)
        .await
        .expect("diff");
    assert_eq!(diff.changes.len(), 1);
    assert_eq!(diff.changes[0].kind, ChangeKind::Modified);

    let blame = history.blame(&table).await.expect("blame");
    let blamed = blame
        .iter()
        .find(|entry| entry.key.get(0) == Some(&Value::Text(key.clone())))
        .expect("the key is blamed");
    assert_eq!(blamed.commit.hash, log[0].hash);

    let tables = adapter.list_tables().await.expect("prefixes");
    assert!(
        tables.iter().any(|t| t.name == "dbboard_it/*"),
        "keys group by / on Umbrachyx"
    );
}
