//! Tunnel configuration value types.
//!
//! These are plain data — no I/O, no serde. The on-disk (`connections.toml`)
//! representation and keyring resolution live in `dbboard-config`; this crate
//! receives an already-resolved [`SshTunnelConfig`] with secrets in hand.

use std::path::PathBuf;

/// A fully-resolved SSH local-forward tunnel: where the bastion is, how to
/// authenticate to it, how to verify its host key, and which remote address to
/// forward to once inside.
#[derive(Clone)]
pub struct SshTunnelConfig {
    /// Bastion hostname or IP (the SSH server we connect to).
    pub host: String,
    /// Bastion SSH port (usually 22).
    pub port: u16,
    /// SSH username on the bastion.
    pub user: String,
    /// How to authenticate to the bastion.
    pub auth: SshAuth,
    /// How to verify the bastion's host key. There is no accept-any variant.
    pub host_key: HostKeyPolicy,
    /// Address the bastion should connect to on our behalf — from the
    /// bastion's point of view. For a localhost-bound database this is
    /// typically `127.0.0.1`.
    pub forward_host: String,
    /// Port on `forward_host` to reach (e.g. 3306 for `MySQL`, 5432 for
    /// Postgres).
    pub forward_port: u16,
}

impl std::fmt::Debug for SshTunnelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `auth` redacts itself; the rest is non-secret.
        f.debug_struct("SshTunnelConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("auth", &self.auth)
            .field("host_key", &self.host_key)
            .field("forward_host", &self.forward_host)
            .field("forward_port", &self.forward_port)
            .finish()
    }
}

/// How to authenticate to the bastion.
#[derive(Clone)]
pub enum SshAuth {
    /// Public-key auth with a private key file. The passphrase, when present,
    /// decrypts the key and must be treated as a secret.
    PrivateKey {
        /// Path to the OpenSSH/PEM private key.
        path: PathBuf,
        /// Passphrase protecting the key, if it is encrypted.
        passphrase: Option<String>,
    },
    /// Password auth.
    Password(String),
}

impl std::fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the passphrase or password — Debug output lands in
        // logs and panic messages.
        match self {
            SshAuth::PrivateKey { path, passphrase } => f
                .debug_struct("PrivateKey")
                .field("path", path)
                .field("passphrase", &passphrase.as_ref().map(|_| "<redacted>"))
                .finish(),
            SshAuth::Password(_) => f.debug_tuple("Password").field(&"<redacted>").finish(),
        }
    }
}

/// Host-key verification policy. Both variants *verify*; neither blindly
/// accepts. This is the security core of ADR-0069 — there is deliberately no
/// `AcceptAny`/TOFU-by-default option in the type.
#[derive(Debug, Clone)]
pub enum HostKeyPolicy {
    /// Pin the server key by its SHA-256 fingerprint (`SHA256:...`, prefix
    /// optional). Deterministic and filesystem-free.
    Fingerprint(String),
    /// Verify against an OpenSSH `known_hosts` file. `None` uses the user's
    /// default (`~/.ssh/known_hosts`). A *mismatch* is a hard failure distinct
    /// from an *unknown* host.
    KnownHosts(Option<PathBuf>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_debug_redacts_passphrase() {
        let auth = SshAuth::PrivateKey {
            path: PathBuf::from("/home/user/.ssh/id_ed25519"),
            passphrase: Some("hunter2".to_string()),
        };
        let rendered = format!("{auth:?}");
        assert!(rendered.contains("id_ed25519"));
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("hunter2"));
    }

    #[test]
    fn auth_debug_redacts_password() {
        let auth = SshAuth::Password("s3cr3t".to_string());
        let rendered = format!("{auth:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("s3cr3t"));
    }

    #[test]
    fn auth_debug_shows_none_passphrase_without_leaking() {
        let auth = SshAuth::PrivateKey {
            path: PathBuf::from("/k"),
            passphrase: None,
        };
        let rendered = format!("{auth:?}");
        assert!(rendered.contains("None"));
    }

    #[test]
    fn config_debug_does_not_leak_secrets() {
        let cfg = SshTunnelConfig {
            host: "bastion.example".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth: SshAuth::Password("topsecret".to_string()),
            host_key: HostKeyPolicy::Fingerprint("SHA256:abc".to_string()),
            forward_host: "127.0.0.1".to_string(),
            forward_port: 3306,
        };
        let rendered = format!("{cfg:?}");
        assert!(rendered.contains("bastion.example"));
        assert!(!rendered.contains("topsecret"));
    }
}
