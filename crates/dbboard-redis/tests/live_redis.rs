//! Live tests against a real Redis-compatible server (ADR-0166).
//!
//! Self-skip unless the variable is set, like the other live tests. Point
//! them at a throwaway server: they write a few keys under a prefix of their
//! own, through a plain client, because the adapter itself cannot write.
//!
//! ```sh
//! valkey-server --port 16379 &
//! DBBOARD_REDIS_URL=redis://127.0.0.1:16379 cargo test -p dbboard-redis --test live_redis
//! # A cluster-mode server (three primaries, as MemoryDB runs):
//! DBBOARD_REDIS_CLUSTER_URL=redis://127.0.0.1:17001 cargo test -p dbboard-redis --test live_redis
//! ```

use dbboard_core::{DatabaseAdapter, DbError, QueryResult, Value};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use dbboard_redis::{AddressMap, MapFuture, RedisAdapter, RedisConfig};
use redis::AsyncCommands;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Unique per test as well as per run: tests against the same server run
/// in parallel, and the fixtures include a list that seeding appends to.
fn prefix(test: &str) -> String {
    format!("dbboard_it_{}_{test}", std::process::id())
}

fn texts(result: &QueryResult, column: usize) -> Vec<String> {
    result
        .rows
        .iter()
        .filter_map(|r| match r.get(column) {
            Some(Value::Text(s)) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Write the fixtures with a plain client. Hash tags keep a test's keys on
/// one slot where a command needs them together.
async fn seed(url: &str, cluster: bool, p: &str) -> redis::RedisResult<()> {
    if cluster {
        let client = redis::cluster::ClusterClient::new(vec![url])?;
        let mut c = client.get_async_connection().await?;
        seed_with(&mut c, p).await
    } else {
        let client = redis::Client::open(url)?;
        let mut c = client.get_multiplexed_async_connection().await?;
        seed_with(&mut c, p).await
    }
}

async fn seed_with<C: AsyncCommands>(c: &mut C, p: &str) -> redis::RedisResult<()> {
    let _: () = c
        .hset_multiple(
            format!("{p}:user:1"),
            &[("name", "Aiko Demo"), ("plan", "pro")],
        )
        .await?;
    let _: () = c.rpush(format!("{p}:queue"), &["a", "b", "c"]).await?;
    let _: () = c
        .zadd_multiple(format!("{p}:board"), &[(30, "ben"), (50, "aiko")])
        .await?;
    for i in 0..120 {
        let _: () = c.set(format!("{p}:cache:{i}"), i).await?;
    }
    let _: () = c.set(format!("{p}:guarded"), "unchanged").await?;
    Ok(())
}

async fn exercise(test: &str, url: String, cluster: bool, route: Option<Arc<dyn AddressMap>>) {
    let p = prefix(test);
    seed(&url, cluster, &p).await.expect("seed fixtures");
    let adapter = RedisAdapter::connect(RedisConfig {
        url: url.clone(),
        cluster,
        route,
    })
    .await
    .expect("connect");
    adapter.ping().await.expect("ping");

    // A hash reads as fields and values.
    let hash = adapter
        .query(&format!("HGETALL {p}:user:1"))
        .await
        .expect("HGETALL");
    assert_eq!(texts(&hash, 0), ["name", "plan"]);
    assert_eq!(texts(&hash, 1), ["Aiko Demo", "pro"]);

    // A whole-list read is checked for size first, then sent.
    let list = adapter
        .query(&format!("LRANGE {p}:queue 0 -1"))
        .await
        .expect("LRANGE");
    assert_eq!(texts(&list, 0), ["a", "b", "c"]);
    let board = adapter
        .query(&format!("ZRANGE {p}:board 0 -1 WITHSCORES"))
        .await
        .expect("ZRANGE");
    assert_eq!(texts(&board, 0), ["ben", "aiko"]);

    // The size check refuses what would not fit.
    let too_big = adapter
        .query_read_only(&format!("LRANGE {p}:queue 0 -1"), 2)
        .await
        .unwrap_err();
    assert!(matches!(too_big, DbError::Query(_)), "{too_big}");

    // A scan follows its cursor until it has what it was asked for, across
    // every node of a cluster.
    let page = adapter
        .query(&format!("SCAN 0 MATCH {p}:cache:* COUNT 50"))
        .await
        .expect("SCAN");
    assert_eq!(page.rows.len(), 50);
    let all = adapter
        .query_read_only(&format!("SCAN 0 MATCH {p}:cache:* COUNT 1000"), 1000)
        .await
        .expect("SCAN all");
    assert_eq!(texts(&all, 0).len(), 120);
    assert_eq!(texts(&all, 1)[0], "0", "the walk is complete");

    // The sidebar's prefixes include this test's.
    let tables = adapter.list_tables().await.expect("list_tables");
    assert!(
        tables.iter().any(|t| t.name == format!("{p}:*")),
        "{tables:?}"
    );

    // A write is refused before it is sent, and the value stays as it was.
    let refused = adapter
        .query(&format!("SET {p}:guarded changed"))
        .await
        .unwrap_err();
    assert!(matches!(refused, DbError::Query(_)), "{refused}");
    let value = adapter
        .query(&format!("GET {p}:guarded"))
        .await
        .expect("GET");
    assert_eq!(texts(&value, 0), ["unchanged"]);

    // KEYS is refused even though it reads.
    assert!(adapter.query("KEYS *").await.is_err());

    cleanup(&url, cluster, &p).await;
}

async fn cleanup(url: &str, cluster: bool, p: &str) {
    let mut keys = vec![
        format!("{p}:user:1"),
        format!("{p}:queue"),
        format!("{p}:board"),
        format!("{p}:guarded"),
    ];
    keys.extend((0..120).map(|i| format!("{p}:cache:{i}")));
    if cluster {
        let client = redis::cluster::ClusterClient::new(vec![url]).expect("client");
        let mut c = client.get_async_connection().await.expect("connect");
        for k in keys {
            let _: redis::RedisResult<()> = c.del(k).await;
        }
    } else {
        let client = redis::Client::open(url).expect("client");
        let mut c = client
            .get_multiplexed_async_connection()
            .await
            .expect("connect");
        let _: redis::RedisResult<()> = c.del(keys).await;
    }
}

#[tokio::test]
async fn a_single_server_reads_and_refuses_writes() {
    let Some(url) = env("DBBOARD_REDIS_URL") else {
        eprintln!("skipping: DBBOARD_REDIS_URL not set");
        return;
    };
    exercise("single", url, false, None).await;
}

#[tokio::test]
async fn a_cluster_is_read_across_every_node() {
    let Some(url) = env("DBBOARD_REDIS_CLUSTER_URL") else {
        eprintln!("skipping: DBBOARD_REDIS_CLUSTER_URL not set");
        return;
    };
    exercise("cluster", url, true, None).await;

    // A cursor belongs to one node, so continuing one is refused rather than
    // read from the wrong node.
    let adapter = RedisAdapter::connect(RedisConfig {
        url: env("DBBOARD_REDIS_CLUSTER_URL").unwrap(),
        cluster: true,
        route: None,
    })
    .await
    .expect("connect");
    assert!(adapter.query("SCAN 17").await.is_err());
}

/// Resolves names the ordinary way, and records every one it was asked for:
/// a stand-in for an SSH forward that shows the client goes through the map
/// for every connection, not only the first.
#[derive(Default)]
struct Recording {
    asked: Mutex<BTreeSet<(String, u16)>>,
}

impl AddressMap for Recording {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> MapFuture<'a> {
        self.asked.lock().unwrap().insert((host.to_owned(), port));
        Box::pin(async move {
            tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| e.to_string())?
                .next()
                .ok_or_else(|| format!("{host}:{port} has no address"))
        })
    }
}

#[tokio::test]
async fn a_routed_server_is_reached_through_the_route() {
    let Some(url) = env("DBBOARD_REDIS_URL") else {
        eprintln!("skipping: DBBOARD_REDIS_URL not set");
        return;
    };
    let route = Arc::new(Recording::default());
    exercise(
        "routed",
        url,
        false,
        Some(route.clone() as Arc<dyn AddressMap>),
    )
    .await;
    assert_eq!(route.asked.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn every_node_of_a_routed_cluster_is_reached_through_the_route() {
    let Some(url) = env("DBBOARD_REDIS_CLUSTER_URL") else {
        eprintln!("skipping: DBBOARD_REDIS_CLUSTER_URL not set");
        return;
    };
    let route = Arc::new(Recording::default());
    exercise(
        "routed_cluster",
        url,
        true,
        Some(route.clone() as Arc<dyn AddressMap>),
    )
    .await;
    let asked = route.asked.lock().unwrap();
    // The URL names one node; the others come from the server, and the
    // client must reach them through the route too.
    let ports: BTreeSet<u16> = asked.iter().map(|(_, port)| *port).collect();
    assert!(ports.len() >= 3, "only asked for {asked:?}");
}
