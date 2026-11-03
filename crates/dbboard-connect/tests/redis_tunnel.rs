//! A Redis-compatible store reached through an SSH bastion, end to end
//! (ADR-0166).
//!
//! The pieces are unit-tested on their own: the route that maps every node
//! to a forward, and the adapter that hands the route to the client. This
//! proves they meet: an entry with a `[connections.ssh]` block goes through
//! [`connect_adapter`], opens one SSH session, and reads keys from every
//! primary of a cluster — whose other nodes the client only learns of from
//! the server.
//!
//! Ignored by default: it needs a bastion and a server, and CI has neither.
//! Any `sshd` the current user can log in to will do, a throwaway one on
//! loopback included. Seed some keys under `tunnel_it:` first (the adapter
//! cannot write), then:
//!
//! ```sh
//! DBBOARD_TEST_REDIS_TUNNEL_URL=redis://127.0.0.1:17001 \
//! DBBOARD_TEST_REDIS_TUNNEL_CLUSTER=1 \
//! DBBOARD_TEST_SSH_HOST=127.0.0.1 DBBOARD_TEST_SSH_PORT=22022 \
//! DBBOARD_TEST_SSH_USER="$USER" DBBOARD_TEST_SSH_KEY=/path/to/id_ed25519 \
//! DBBOARD_TEST_SSH_FINGERPRINT=SHA256:… \
//!   cargo test -p dbboard-connect --test redis_tunnel -- --ignored
//! ```
//!
//! On loopback the forwards and a direct connection reach the same server,
//! so the bastion's own log is what shows the traffic went through it: one
//! `direct-tcpip` channel per node.

use dbboard_config::{ConnectionFile, InMemorySecretStore, SecretStore};
use dbboard_connect::{backend_config_for_entry, connect_adapter};
use dbboard_core::Value;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this test"))
}

#[tokio::test]
#[ignore = "needs a bastion and a Redis-compatible server"]
async fn every_key_is_read_through_the_bastion() {
    let cluster = std::env::var("DBBOARD_TEST_REDIS_TUNNEL_CLUSTER").is_ok_and(|v| v == "1");
    let toml_src = format!(
        r#"
version = 1
[[connections]]
id   = "cache"
name = "Cache"
kind = "redis"
keyring_url_ref = "dbboard.cache.url"
cluster = {cluster}

[connections.ssh]
host = "{host}"
port = {port}
user = "{user}"
key_path = "{key}"
fingerprint = "{fingerprint}"
"#,
        host = env("DBBOARD_TEST_SSH_HOST"),
        port = env("DBBOARD_TEST_SSH_PORT"),
        user = env("DBBOARD_TEST_SSH_USER"),
        key = env("DBBOARD_TEST_SSH_KEY"),
        fingerprint = env("DBBOARD_TEST_SSH_FINGERPRINT"),
    );
    let file = ConnectionFile::parse(&toml_src).expect("entry parses");
    let secrets = InMemorySecretStore::new();
    secrets
        .set("dbboard.cache.url", &env("DBBOARD_TEST_REDIS_TUNNEL_URL"))
        .expect("seed secret");
    let backend = backend_config_for_entry(&file.connections[0], &secrets).expect("resolve");
    let adapter = connect_adapter(backend)
        .await
        .expect("connect through the bastion");

    let result = adapter
        .query_read_only("SCAN 0 MATCH tunnel_it:* COUNT 1000", 1000)
        .await
        .expect("scan");
    let mut keys: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| match row.get(0) {
            Some(Value::Text(k)) => Some(k.clone()),
            _ => None,
        })
        .collect();
    keys.sort();
    let expected: Vec<String> = ('a'..='f').map(|c| format!("tunnel_it:{c}")).collect();
    assert_eq!(keys, expected);

    let value = adapter
        .query_read_only("GET tunnel_it:c", 1)
        .await
        .expect("get");
    assert_eq!(
        value.rows[0].get(0),
        Some(&Value::Text("value-c".to_owned()))
    );
}
