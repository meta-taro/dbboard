//! The `DatabaseAdapter` implementation (ADR-0166).
//!
//! **The query text is a command line, not SQL.** `query_read_only`
//! overrides the trait default — which would hand the text to the SQL
//! classifier and refuse everything — with [`crate::read_only`], and every
//! path from caller text to the server runs through it. The adapter's own
//! commands (the ping, the key sample, the size checks, the node list) are
//! constants in this file, and none of them writes.
//!
//! **There are no tables.** `list_tables` samples the keyspace with `SCAN`
//! and offers one entry per key prefix (`user:*`), whose name is the
//! `MATCH` pattern that browses it.
//!
//! **A cluster is walked node by node.** `SCAN` only sees the keys of the
//! node it is sent to, so in cluster mode a scan from cursor `0` visits
//! every primary in turn. A cursor other than `0` belongs to one node and
//! cannot be continued on another, so it is refused there.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dbboard_core::{
    Capabilities, ChangeReview, DatabaseAdapter, DbError, DbResult, HistoryReader, QueryResult,
    TableInfo, TableSchema, MAX_RESULT_ROWS,
};
use redis::aio::{ConnectionLike, MultiplexedConnection};
use redis::cluster::ClusterClient;
use redis::cluster_async::ClusterConnection;
use redis::cluster_routing::{RoutingInfo, SingleNodeRoutingInfo};
use redis::{AsyncConnectionConfig, Client, Cmd, Value};

use crate::read_only::{classify, ReadPlan, Shape, SizeGuard};
use crate::reply;
use crate::route::{AddressMap, Resolver};
use crate::umbrachyx::{self, UmbrachyxHistory};

/// How long to wait for a connection, and for any one reply.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Keys read to find the prefixes the sidebar lists. Enough to find the
/// prefixes a keyspace is organised by, few enough that listing them is a
/// handful of `SCAN` calls on a keyspace of any size.
const PREFIX_SAMPLE: usize = 10_000;

/// Prefixes listed at most. Past this the sidebar stops being a list.
const MAX_PREFIXES: usize = 500;

/// Keys a scan collects when the command names no `COUNT`.
const DEFAULT_SCAN_ROWS: usize = 100;

/// The most `SCAN` round trips one call makes on one node. A sparse `MATCH`
/// over a large keyspace returns mostly empty pages; this keeps such a walk
/// from running for minutes. The cursor it stopped at is in the result.
const MAX_SCAN_CALLS: usize = 200;

/// The delimiter key prefixes are grouped by.
const DELIMITER: char = ':';

#[derive(Clone)]
pub struct RedisConfig {
    /// A `redis://` or `rediss://` (TLS) URL. User and password, when the
    /// server wants them, are in the URL.
    pub url: String,
    /// Connect as a cluster client. `MemoryDB` always runs in cluster mode;
    /// a cluster-mode server refuses keyed commands sent to the wrong node
    /// unless the client follows its redirects.
    pub cluster: bool,
    /// Where each server's `host:port` is actually reached, such as an SSH
    /// forward. `None` connects to the address itself.
    pub route: Option<Arc<dyn AddressMap>>,
}

/// Hand-written: the URL usually carries the password.
impl std::fmt::Debug for RedisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisConfig")
            .field("cluster", &self.cluster)
            .field("routed", &self.route.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) enum Connection {
    Single(MultiplexedConnection),
    Cluster(ClusterConnection),
}

pub struct RedisAdapter {
    connection: Connection,
    /// Present when the server answered `UMBRA.BRANCHES` at connect time:
    /// it is Umbrachyx, and keeps a history (ADR-0170).
    umbrachyx: Option<UmbrachyxHistory>,
    /// Held so whatever the route keeps open, such as a forward, lasts as
    /// long as the connection that goes through it.
    _route: Option<Arc<dyn AddressMap>>,
}

/// Hand-written so the URL, which usually carries the password, never
/// reaches a log line.
impl std::fmt::Debug for RedisAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = match self.connection {
            Connection::Single(_) => "single",
            Connection::Cluster(_) => "cluster",
        };
        f.debug_struct("RedisAdapter")
            .field("mode", &mode)
            .finish_non_exhaustive()
    }
}

impl RedisAdapter {
    /// Connect to the server or cluster `config` names.
    ///
    /// # Errors
    ///
    /// [`DbError::Connection`] when the URL does not parse or the server
    /// cannot be reached. The message never contains the URL.
    pub async fn connect(config: RedisConfig) -> DbResult<Self> {
        let connection = if config.cluster {
            let mut builder = ClusterClient::builder([config.url.as_str()])
                .connection_timeout(CONNECT_TIMEOUT)
                .response_timeout(RESPONSE_TIMEOUT);
            if let Some(route) = &config.route {
                builder = builder.async_dns_resolver(Resolver(Arc::clone(route)));
            }
            let client = builder.build().map_err(|e| connection_error(&e))?;
            Connection::Cluster(
                client
                    .get_async_connection()
                    .await
                    .map_err(|e| connection_error(&e))?,
            )
        } else {
            let client = Client::open(config.url.as_str()).map_err(|e| connection_error(&e))?;
            let mut settings = AsyncConnectionConfig::new()
                .set_connection_timeout(Some(CONNECT_TIMEOUT))
                .set_response_timeout(Some(RESPONSE_TIMEOUT));
            if let Some(route) = &config.route {
                settings = settings.set_dns_resolver(Resolver(Arc::clone(route)));
            }
            Connection::Single(
                client
                    .get_multiplexed_async_connection_with_config(&settings)
                    .await
                    .map_err(|e| connection_error(&e))?,
            )
        };
        // Umbrachyx serves one node, so a cluster is never asked.
        let umbrachyx = match &connection {
            Connection::Single(_) => UmbrachyxHistory::detect(&connection).await,
            Connection::Cluster(_) => None,
        };
        Ok(Self {
            connection,
            umbrachyx,
            _route: config.route,
        })
    }

    async fn send(&self, cmd: &Cmd) -> DbResult<Value> {
        send(&self.connection, cmd).await
    }

    /// Classify `line`, check its size guard, send it and shape the reply.
    async fn run(&self, line: &str, max_rows: usize) -> DbResult<QueryResult> {
        let plan = classify(line)?;
        if let Some(guard) = &plan.guard {
            self.check_size(guard, max_rows, plan.name()).await?;
        }
        match plan.shape {
            Shape::ScanKeys => self.scan_keys(&plan, max_rows).await,
            Shape::ScanMembers | Shape::ScanFieldValues | Shape::ScanMemberScores => {
                self.scan_collection(&plan, max_rows).await
            }
            shape => {
                let reply = self.send(&command(&plan.words)).await?;
                let result = reply::to_result(shape, reply)?;
                if result.rows.len() > max_rows {
                    return Err(dbboard_core::too_many_rows_error());
                }
                Ok(result)
            }
        }
    }

    /// Refuse a whole-collection read whose collection is over the cap,
    /// before the collection is sent.
    async fn check_size(&self, guard: &SizeGuard, max_rows: usize, name: &str) -> DbResult<()> {
        let size = reply::integer(&self.send(&command(&[guard.command, &guard.key])).await?)?;
        if usize::try_from(size).unwrap_or(usize::MAX) > max_rows {
            return Err(DbError::Query(format!(
                "{name} would return {size} items, more than the {max_rows} a result may hold: \
                 read it a page at a time with the SCAN form or a narrower range"
            )));
        }
        Ok(())
    }

    /// `SCAN`, followed until it has the rows asked for.
    async fn scan_keys(&self, plan: &ReadPlan, max_rows: usize) -> DbResult<QueryResult> {
        let want = wanted(plan, max_rows);
        let cursor = plan.words[1].clone();
        let (keys, next) = match &self.connection {
            Connection::Single(_) => self.follow(plan, 1, cursor, want, None).await?,
            Connection::Cluster(_) => {
                if cursor != "0" {
                    return Err(DbError::Query(
                        "in cluster mode a scan starts from cursor 0: a cursor belongs to one node \
                         and cannot be continued on another"
                            .into(),
                    ));
                }
                self.scan_cluster(plan, want).await?
            }
        };
        Ok(reply::scan_result(plan.shape, keys, &next))
    }

    /// `HSCAN`, `SSCAN` or `ZSCAN`. One key lives on one node, so these
    /// need no cluster walk.
    async fn scan_collection(&self, plan: &ReadPlan, max_rows: usize) -> DbResult<QueryResult> {
        let want = wanted(plan, max_rows);
        // A field-value or member-score scan returns two items per row.
        let per_row = if matches!(plan.shape, Shape::ScanMembers) {
            1
        } else {
            2
        };
        let (items, next) = self
            .follow(plan, 2, plan.words[2].clone(), want * per_row, None)
            .await?;
        Ok(reply::scan_result(plan.shape, items, &next))
    }

    /// Call the scan in `plan` from `cursor` until `want` items are in hand,
    /// the walk ends, or the call budget runs out. `at` is where the cursor
    /// sits among the words; `node` sends every call to one cluster node.
    async fn follow(
        &self,
        plan: &ReadPlan,
        at: usize,
        mut cursor: String,
        want: usize,
        node: Option<&(String, u16)>,
    ) -> DbResult<(Vec<Value>, String)> {
        let mut items = Vec::new();
        for _ in 0..MAX_SCAN_CALLS {
            let mut words = plan.words.clone();
            words[at] = cursor;
            let reply = match node {
                Some(address) => self.send_to(address, &command(&words)).await?,
                None => self.send(&command(&words)).await?,
            };
            let (next, page) = reply::scan_page(reply)?;
            items.extend(page);
            cursor = next;
            if cursor == "0" || items.len() >= want {
                break;
            }
        }
        items.truncate(want);
        Ok((items, cursor))
    }

    /// Scan every primary of the cluster in turn.
    async fn scan_cluster(&self, plan: &ReadPlan, want: usize) -> DbResult<(Vec<Value>, String)> {
        let mut keys = Vec::new();
        let mut complete = true;
        for node in self.primaries().await? {
            let (found, next) = self
                .follow(plan, 1, "0".into(), want - keys.len(), Some(&node))
                .await?;
            keys.extend(found);
            complete &= next == "0";
            if keys.len() >= want {
                complete = false;
                break;
            }
        }
        // There is no single cursor to continue a cluster walk from, so an
        // unfinished one says so by not reporting `0`.
        Ok((
            keys,
            if complete {
                "0".into()
            } else {
                "partial".into()
            },
        ))
    }

    async fn send_to(&self, (host, port): &(String, u16), cmd: &Cmd) -> DbResult<Value> {
        let Connection::Cluster(mut c) = self.connection.clone() else {
            return self.send(cmd).await;
        };
        let routing = RoutingInfo::SingleNode(SingleNodeRoutingInfo::ByAddress {
            host: host.clone(),
            port: *port,
        });
        c.route_command(cmd.clone(), routing)
            .await
            .map_err(|e| query_error(&e))
    }

    /// The cluster's primaries, from `CLUSTER NODES`.
    async fn primaries(&self) -> DbResult<Vec<(String, u16)>> {
        let Connection::Cluster(mut c) = self.connection.clone() else {
            return Ok(Vec::new());
        };
        let reply = c
            .route_command(
                command(&["CLUSTER", "NODES"]),
                RoutingInfo::SingleNode(SingleNodeRoutingInfo::Random),
            )
            .await
            .map_err(|e| query_error(&e))?;
        let dbboard_core::Value::Text(body) = reply::cell(reply) else {
            return Err(DbError::TypeConversion(
                "CLUSTER NODES did not answer text".into(),
            ));
        };
        Ok(primaries_of(&body))
    }

    /// Group a sample of keys by their first segment.
    async fn prefixes(&self) -> DbResult<Vec<TableInfo>> {
        let plan = classify(&format!(
            "SCAN 0 COUNT {}",
            crate::read_only::MAX_SCAN_COUNT
        ))?;
        let (keys, _) = match &self.connection {
            Connection::Single(_) => {
                self.follow(&plan, 1, "0".into(), PREFIX_SAMPLE, None)
                    .await?
            }
            Connection::Cluster(_) => self.scan_cluster(&plan, PREFIX_SAMPLE).await?,
        };
        let names: Vec<String> = keys
            .into_iter()
            .filter_map(|k| match reply::cell(k) {
                dbboard_core::Value::Text(s) => Some(s),
                _ => None,
            })
            .collect();
        let delimiter = if self.umbrachyx.is_some() {
            umbrachyx::DELIMITER
        } else {
            DELIMITER
        };
        Ok(group(&names, delimiter)
            .into_iter()
            .map(TableInfo::unqualified)
            .collect())
    }
}

/// How many rows a scan should collect: its `COUNT`, else a page, never
/// more than the caller allows.
fn wanted(plan: &ReadPlan, max_rows: usize) -> usize {
    let count = plan
        .words
        .iter()
        .position(|w| w.eq_ignore_ascii_case("COUNT"))
        .and_then(|i| plan.words.get(i + 1))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SCAN_ROWS);
    count.clamp(1, max_rows.max(1))
}

/// The `MATCH` pattern for each prefix in `keys`: `user:*` for every key that
/// starts `user:`, and the key itself, escaped, for one with no delimiter.
fn group(keys: &[String], delimiter: char) -> Vec<String> {
    let mut patterns = BTreeSet::new();
    for key in keys {
        let pattern = match key.split_once(delimiter) {
            Some((head, _)) => format!("{}{delimiter}*", escape_glob(head)),
            None => escape_glob(key),
        };
        patterns.insert(pattern);
        if patterns.len() >= MAX_PREFIXES {
            break;
        }
    }
    patterns.into_iter().collect()
}

/// Escape the characters `MATCH` treats as a pattern.
fn escape_glob(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The primaries in a `CLUSTER NODES` reply that are not marked failed.
fn primaries_of(body: &str) -> Vec<(String, u16)> {
    body.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _id = fields.next()?;
            let address = fields.next()?;
            let flags = fields.next()?;
            let flags: Vec<&str> = flags.split(',').collect();
            if !flags.contains(&"master") || flags.iter().any(|f| f.starts_with("fail")) {
                return None;
            }
            // `ip:port@cport[,hostname]`
            let host_port = address.split('@').next()?;
            let (host, port) = host_port.rsplit_once(':')?;
            Some((host.to_owned(), port.parse().ok()?))
        })
        .collect()
}

/// Send `cmd` on `connection` and read its reply.
pub(crate) async fn send(connection: &Connection, cmd: &Cmd) -> DbResult<Value> {
    match connection.clone() {
        Connection::Single(mut c) => c.req_packed_command(cmd).await,
        Connection::Cluster(mut c) => c.req_packed_command(cmd).await,
    }
    .map_err(|e| query_error(&e))
}

fn command<S: AsRef<str>>(words: &[S]) -> Cmd {
    let mut cmd = redis::cmd(words[0].as_ref());
    for word in &words[1..] {
        cmd.arg(word.as_ref());
    }
    cmd
}

/// A failure to connect. `redis` puts the URL in some of its messages, so
/// only the kind and the server's own detail are kept.
fn connection_error(error: &redis::RedisError) -> DbError {
    DbError::Connection(describe(error))
}

pub(crate) fn query_error(error: &redis::RedisError) -> DbError {
    if error.is_io_error() || error.is_connection_dropped() || error.is_timeout() {
        DbError::Connection(describe(error))
    } else {
        DbError::Query(describe(error))
    }
}

fn describe(error: &redis::RedisError) -> String {
    let detail = error.detail().unwrap_or_default();
    let kind = format!("{:?}", error.kind());
    if detail.is_empty() || detail.contains("://") {
        kind
    } else {
        format!("{kind}: {detail}")
    }
}

#[async_trait]
impl DatabaseAdapter for RedisAdapter {
    fn id(&self) -> &'static str {
        "redis"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_history: self.umbrachyx.is_some(),
            has_review: self.umbrachyx.is_some(),
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        self.send(&redis::cmd("PING")).await.map(|_| ())
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        self.prefixes().await
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        let result = self.run(sql, MAX_RESULT_ROWS + 1).await?;
        if result.rows.len() > MAX_RESULT_ROWS {
            return Err(dbboard_core::too_many_rows_error());
        }
        Ok(result)
    }

    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        self.run(sql, max_rows).await
    }

    /// Only an Umbrachyx connection describes a pattern, so that the history
    /// views can key a row by it (ADR-0170). `has_describe_table` stays off:
    /// it would also put the column views on a plain Redis connection's
    /// patterns, whose Structure tab is the key browser.
    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        match self.umbrachyx {
            Some(_) => Ok(crate::umbrachyx::schema(table)),
            None => Err(DbError::Capability(
                "describe_table not supported by this adapter".into(),
            )),
        }
    }

    fn history(&self) -> Option<&dyn HistoryReader> {
        self.umbrachyx.as_ref().map(|h| h as &dyn HistoryReader)
    }

    fn review(&self) -> Option<&dyn ChangeReview> {
        self.umbrachyx.as_ref().map(|h| h as &dyn ChangeReview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(line: &str) -> ReadPlan {
        classify(line).expect("allowed")
    }

    #[test]
    fn a_scan_collects_its_count_or_a_page() {
        assert_eq!(wanted(&plan("SCAN 0 COUNT 25"), 1000), 25);
        assert_eq!(wanted(&plan("SCAN 0"), 1000), DEFAULT_SCAN_ROWS);
        assert_eq!(wanted(&plan("SCAN 0 COUNT 500"), 50), 50);
    }

    #[test]
    fn keys_group_by_their_first_segment() {
        let keys = ["user:1", "user:2:cart", "order:9", "config", "a*b:1"].map(String::from);
        assert_eq!(
            group(&keys, DELIMITER),
            ["a\\*b:*", "config", "order:*", "user:*"]
        );
        let umbrachyx_keys = ["player/42/gems", "player/7/gems", "config"].map(String::from);
        assert_eq!(
            group(&umbrachyx_keys, umbrachyx::DELIMITER),
            ["config", "player/*"]
        );
    }

    #[test]
    fn a_key_with_glob_characters_matches_only_itself() {
        assert_eq!(escape_glob("odd[1]*?\\"), "odd\\[1\\]\\*\\?\\\\");
    }

    #[test]
    fn primaries_are_read_from_cluster_nodes() {
        let body = "\
07c3 10.0.0.1:6379@16379,node-a myself,master - 0 0 1 connected 0-5460
67ed 10.0.0.2:6379@16379 slave 07c3 0 0 1 connected
292f 10.0.0.3:6380@16380 master,fail - 0 0 2 connected 5461-10922
e7d1 10.0.0.4:6379@16379 master - 0 0 3 connected 10923-16383
";
        assert_eq!(
            primaries_of(body),
            [("10.0.0.1".to_owned(), 6379), ("10.0.0.4".to_owned(), 6379)]
        );
    }

    #[tokio::test]
    async fn a_url_that_is_not_redis_is_refused_without_echoing_it() {
        let error = RedisAdapter::connect(RedisConfig {
            url: "postgres://user:hunter2@localhost/shop".into(),
            cluster: false,
            route: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(error, DbError::Connection(_)), "{error:?}");
        assert!(!error.to_string().contains("hunter2"), "{error}");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_connection_error() {
        let error = RedisAdapter::connect(RedisConfig {
            url: "redis://:hunter2@127.0.0.1:1".into(),
            cluster: false,
            route: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(error, DbError::Connection(_)), "{error:?}");
        assert!(!error.to_string().contains("hunter2"), "{error}");
    }
}
