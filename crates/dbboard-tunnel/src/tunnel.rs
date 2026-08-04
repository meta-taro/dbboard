//! Opening and probing the SSH local-forward tunnel.
//!
//! [`open`] connects to the bastion, verifies its host key against the
//! configured [`HostKeyPolicy`], authenticates, binds an ephemeral loopback
//! listener, and forwards every accepted connection over an SSH
//! `direct-tcpip` channel to the far-side database. The returned [`SshTunnel`]
//! owns the accept loop and tears it down on drop — bind its lifetime to
//! whatever holds the database connection so the forward outlives every query
//! but not the adapter.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use russh::keys::ssh_key::{HashAlg, PublicKey};

use crate::config::{HostKeyPolicy, SshAuth, SshTunnelConfig};
use crate::error::{TunnelError, TunnelResult};
use crate::fingerprint::fingerprint_matches;

/// A running SSH local-forward tunnel. Holds the accept loop; dropping it
/// aborts the loop and (once no forwarded connections remain) closes the SSH
/// session.
pub struct SshTunnel {
    local_addr: SocketAddr,
    accept_task: tokio::task::JoinHandle<()>,
}

impl SshTunnel {
    /// The loopback address the tunnel is listening on. Point the database
    /// client here instead of the real (bastion-gated) host.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The ephemeral local port the tunnel is listening on.
    #[must_use]
    pub fn local_port(&self) -> u16 {
        self.local_addr.port()
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        // Stop accepting new forwards. In-flight forwards hold their own clone
        // of the session and finish when their socket closes; the session then
        // drops with the last of them.
        self.accept_task.abort();
    }
}

/// Connect to the bastion, verify its host key, authenticate, and start
/// forwarding a fresh loopback port to `forward_host:forward_port` on the far
/// side.
///
/// # Errors
/// Returns [`TunnelError::HostKey`] if host-key verification fails (unknown
/// host or — worse — a mismatch), [`TunnelError::Auth`] on bad credentials,
/// [`TunnelError::KeyLoad`] if the private key cannot be read/decrypted, and
/// [`TunnelError::Connect`]/[`TunnelError::Listener`] for transport failures.
///
/// # Panics
/// Panics only if an internal mutex is poisoned — i.e. another task panicked
/// while holding it, which does not happen in normal operation.
pub async fn open(config: SshTunnelConfig) -> TunnelResult<SshTunnel> {
    let rejection = Arc::new(Mutex::new(None));
    let handler = VerifyHandler {
        policy: config.host_key.clone(),
        host: config.host.clone(),
        port: config.port,
        rejection: Arc::clone(&rejection),
    };

    let ssh_config = Arc::new(russh::client::Config::default());
    let mut session =
        russh::client::connect(ssh_config, (config.host.as_str(), config.port), handler)
            .await
            .map_err(
                |e| match rejection.lock().expect("rejection lock poisoned").take() {
                    // A captured rejection means check_server_key rejected the key —
                    // report it as a host-key failure, not a bland connect error.
                    Some(reason) => TunnelError::HostKey(reason),
                    None => TunnelError::Connect(format!("{}:{}: {e}", config.host, config.port)),
                },
            )?;

    if !authenticate(&mut session, &config).await? {
        return Err(TunnelError::Auth(config.user.clone()));
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0u16))
        .await
        .map_err(TunnelError::Listener)?;
    let local_addr = listener.local_addr().map_err(TunnelError::Listener)?;

    let session = Arc::new(session);
    let accept_task = tokio::spawn(accept_loop(
        listener,
        session,
        config.forward_host.clone(),
        config.forward_port,
    ));

    Ok(SshTunnel {
        local_addr,
        accept_task,
    })
}

/// Connect only far enough to read the bastion's host-key fingerprint, without
/// authenticating. Used to show the user a fingerprint to pin on first setup —
/// the SSH equivalent of a first-connection host-key prompt.
///
/// # Errors
/// Returns [`TunnelError::Connect`] if the server key could not be obtained.
///
/// # Panics
/// Panics only if an internal mutex is poisoned (a bug), which does not happen
/// in normal operation.
pub async fn probe_host_key(host: &str, port: u16) -> TunnelResult<String> {
    let captured = Arc::new(Mutex::new(None));
    let handler = ProbeHandler {
        captured: Arc::clone(&captured),
    };
    let ssh_config = Arc::new(russh::client::Config::default());
    // The probe handler rejects the key after capturing it, so this connect is
    // expected to return an error — we only care about the captured value.
    let _ = russh::client::connect(ssh_config, (host, port), handler).await;
    let fingerprint = captured.lock().expect("probe lock poisoned").take();
    fingerprint
        .ok_or_else(|| TunnelError::Connect(format!("{host}:{port}: server key was not received")))
}

async fn authenticate(
    session: &mut russh::client::Handle<VerifyHandler>,
    config: &SshTunnelConfig,
) -> TunnelResult<bool> {
    match &config.auth {
        SshAuth::PrivateKey { path, passphrase } => {
            let key = russh::keys::load_secret_key(path, passphrase.as_deref()).map_err(|e| {
                TunnelError::KeyLoad {
                    path: path.display().to_string(),
                    detail: e.to_string(),
                }
            })?;
            let with_alg = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
            let result = session
                .authenticate_publickey(&config.user, with_alg)
                .await
                .map_err(|e| TunnelError::Connect(format!("publickey auth error: {e}")))?;
            Ok(result.success())
        }
        SshAuth::Password(password) => {
            let result = session
                .authenticate_password(&config.user, password)
                .await
                .map_err(|e| TunnelError::Connect(format!("password auth error: {e}")))?;
            Ok(result.success())
        }
    }
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    session: Arc<russh::client::Handle<VerifyHandler>>,
    forward_host: String,
    forward_port: u16,
) {
    loop {
        let Ok((mut local, peer)) = listener.accept().await else {
            break;
        };
        let session = Arc::clone(&session);
        let forward_host = forward_host.clone();
        let peer_port = u32::from(peer.port());
        tokio::spawn(async move {
            let Ok(channel) = session
                .channel_open_direct_tcpip(
                    forward_host,
                    u32::from(forward_port),
                    "127.0.0.1",
                    peer_port,
                )
                .await
            else {
                return;
            };
            let mut remote = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
        });
    }
}

/// Client handler that enforces the host-key policy and captures the reason a
/// key was rejected so [`open`] can report it distinctly.
struct VerifyHandler {
    policy: HostKeyPolicy,
    host: String,
    port: u16,
    rejection: Arc<Mutex<Option<String>>>,
}

impl VerifyHandler {
    fn reject(&self, reason: String) {
        *self.rejection.lock().expect("rejection lock poisoned") = Some(reason);
    }
}

impl russh::client::Handler for VerifyHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match &self.policy {
            HostKeyPolicy::Fingerprint(expected) => {
                let actual = server_public_key.fingerprint(HashAlg::Sha256).to_string();
                if fingerprint_matches(expected, &actual) {
                    Ok(true)
                } else {
                    self.reject(format!(
                        "fingerprint mismatch: expected {expected}, server presented {actual}"
                    ));
                    Ok(false)
                }
            }
            HostKeyPolicy::KnownHosts(path) => {
                let checked = match path {
                    Some(p) => russh::keys::check_known_hosts_path(
                        &self.host,
                        self.port,
                        server_public_key,
                        p,
                    ),
                    None => {
                        russh::keys::check_known_hosts(&self.host, self.port, server_public_key)
                    }
                };
                match checked {
                    Ok(true) => Ok(true),
                    Ok(false) => {
                        let fp = server_public_key.fingerprint(HashAlg::Sha256);
                        self.reject(format!(
                            "unknown host {}:{} (server key {fp}); pin it before connecting",
                            self.host, self.port
                        ));
                        Ok(false)
                    }
                    Err(e) => {
                        // A known_hosts *error* is the MITM signal: the host is
                        // known but the key changed. Fail hard and loud.
                        self.reject(format!(
                            "KEY MISMATCH for {}:{} — the host key changed, possible man-in-the-middle ({e})",
                            self.host, self.port
                        ));
                        Ok(false)
                    }
                }
            }
        }
    }
}

/// Client handler used only to read the host-key fingerprint. It captures the
/// fingerprint then rejects, so the connection never proceeds to auth.
struct ProbeHandler {
    captured: Arc<Mutex<Option<String>>>,
}

impl russh::client::Handler for ProbeHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        *self.captured.lock().expect("probe lock poisoned") =
            Some(server_public_key.fingerprint(HashAlg::Sha256).to_string());
        Ok(false)
    }
}
