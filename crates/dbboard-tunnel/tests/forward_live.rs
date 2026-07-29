//! Live SSH-forward integration test, gated on `DBBOARD_SSH_*` env vars.
//!
//! CI stays offline: with no `DBBOARD_SSH_HOST` set the test returns early. To
//! run it against a real bastion (the maintainer's VPS), set:
//!
//! ```text
//! DBBOARD_SSH_HOST=bastion.example        # required — enables the test
//! DBBOARD_SSH_PORT=22                      # optional (default 22)
//! DBBOARD_SSH_USER=deploy                  # required
//! DBBOARD_SSH_KEY=/path/to/id_ed25519      # key auth (or DBBOARD_SSH_PASSWORD)
//! DBBOARD_SSH_KEY_PASSPHRASE=...           # optional, if the key is encrypted
//! DBBOARD_SSH_PASSWORD=...                 # password auth (if no key)
//! DBBOARD_SSH_FINGERPRINT=SHA256:...       # host-key pin (or DBBOARD_SSH_KNOWN_HOSTS)
//! DBBOARD_SSH_KNOWN_HOSTS=/path/known_hosts # host-key file (default: pin required)
//! DBBOARD_SSH_FORWARD_HOST=127.0.0.1       # optional (default 127.0.0.1)
//! DBBOARD_SSH_FORWARD_PORT=3306            # optional (default 3306 — MySQL/MariaDB)
//! ```
//!
//! The far-side service is assumed to greet on connect (MySQL/MariaDB sends a
//! handshake packet), so the test asserts that bytes flow back through the
//! tunnel — proving the full connect → verify → auth → direct-tcpip →
//! copy path.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use dbboard_tunnel::{HostKeyPolicy, SshAuth, SshTunnelConfig};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

fn env_var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::test]
async fn forwards_to_far_side_service() {
    let Some(host) = env_var("DBBOARD_SSH_HOST") else {
        eprintln!("DBBOARD_SSH_HOST unset — skipping live SSH forward test");
        return;
    };

    let port: u16 = env_var("DBBOARD_SSH_PORT")
        .and_then(|v| v.parse().ok())
        .unwrap_or(22);
    let user = env_var("DBBOARD_SSH_USER").expect("DBBOARD_SSH_USER required for live test");

    let auth = if let Some(key) = env_var("DBBOARD_SSH_KEY") {
        SshAuth::PrivateKey {
            path: PathBuf::from(key),
            passphrase: env_var("DBBOARD_SSH_KEY_PASSPHRASE"),
        }
    } else if let Some(password) = env_var("DBBOARD_SSH_PASSWORD") {
        SshAuth::Password(password)
    } else {
        panic!("set DBBOARD_SSH_KEY or DBBOARD_SSH_PASSWORD for the live test");
    };

    let host_key = if let Some(fp) = env_var("DBBOARD_SSH_FINGERPRINT") {
        HostKeyPolicy::Fingerprint(fp)
    } else if let Some(kh) = env_var("DBBOARD_SSH_KNOWN_HOSTS") {
        HostKeyPolicy::KnownHosts(Some(PathBuf::from(kh)))
    } else {
        panic!("set DBBOARD_SSH_FINGERPRINT or DBBOARD_SSH_KNOWN_HOSTS to pin the host key");
    };

    let forward_host =
        env_var("DBBOARD_SSH_FORWARD_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let forward_port: u16 = env_var("DBBOARD_SSH_FORWARD_PORT")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3306);

    let config = SshTunnelConfig {
        host,
        port,
        user,
        auth,
        host_key,
        forward_host,
        forward_port,
    };

    let tunnel = dbboard_tunnel::open(config)
        .await
        .expect("tunnel should open against the configured bastion");

    let mut stream = TcpStream::connect(tunnel.local_addr())
        .await
        .expect("connect to the local forward port");

    // MySQL/MariaDB greets on connect; read the first bytes of the handshake.
    let mut buf = [0u8; 16];
    let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
        .await
        .expect("far-side service should respond within 10s")
        .expect("read handshake bytes");
    assert!(n > 0, "expected a handshake greeting through the tunnel");
}
