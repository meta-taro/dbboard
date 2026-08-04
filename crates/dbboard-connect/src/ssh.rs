//! Resolve a connection's SSH tunnel block into a live forward (ADR-0069).
//!
//! The on-disk [`SshTunnelToml`] carries only the bastion coordinates and
//! *references* to its secrets; this module pulls those secrets from the
//! keyring (or, for the env-var path, from the process environment) into a
//! [`ResolvedSsh`], and turns a database URL into the two halves the tunnel
//! needs: the far-side `host:port` to forward to, and — once the tunnel is
//! up — the loopback URL the ordinary TCP adapter actually dials.
//!
//! The far-side address is *not* stored in the tunnel block: it is the
//! database's own URL host:port, exactly like a GUI client forwards to the DB
//! host named in its main connection settings. So resolution is a two-step
//! dance owned by [`crate::backend`]: parse the target out of the URL, open
//! the tunnel, then rewrite the URL to the loopback the tunnel bound.

use std::path::PathBuf;

use dbboard_config::{ConfigError, SecretStore, SshTunnelToml};
use dbboard_core::{DbError, DbResult};
use dbboard_tunnel::{HostKeyPolicy, SshAuth, SshTunnelConfig};
use url::Url;

/// Default database ports, used as the SSH forward target when a connection
/// URL omits an explicit port. Keyed by adapter, so a `mysql://host/db` with
/// no port forwards to 3306 and a `postgres://host/db` to 5432.
pub(crate) const DEFAULT_POSTGRES_PORT: u16 = 5432;
pub(crate) const DEFAULT_MYSQL_PORT: u16 = 3306;

/// A fully-resolved SSH tunnel minus its forward target: bastion host/port/user
/// plus the *secret-bearing* auth and the host-key policy. The forward
/// `host:port` is supplied later, from the database URL, via
/// [`ResolvedSsh::into_tunnel_config`].
///
/// Constructed from an [`SshTunnelToml`] (keyring path) or the `DBBOARD_SSH_*`
/// environment (env path). Held inside a [`crate::BackendConfig`] URL variant,
/// so its fields are private and its `Debug` is redacted — the enclosing
/// `BackendConfig::Debug` already redacts the whole variant, and this keeps
/// the type safe if that ever changes.
#[derive(Clone)]
pub struct ResolvedSsh {
    host: String,
    port: u16,
    user: String,
    auth: SshAuth,
    host_key: HostKeyPolicy,
}

impl std::fmt::Debug for ResolvedSsh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `auth` redacts itself; host/port/user/host_key are non-secret.
        f.debug_struct("ResolvedSsh")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("auth", &self.auth)
            .field("host_key", &self.host_key)
            .finish()
    }
}

impl ResolvedSsh {
    /// Resolve a stored `[connections.ssh]` block, pulling the passphrase or
    /// password out of `secrets`. The block was already shape-validated at
    /// load time ([`SshTunnelToml::validate`]); the defensive branches here
    /// keep the mapping total without trusting that.
    ///
    /// # Errors
    /// - [`ConfigError::Secret`] when a `keyring_*_ref` cannot be resolved.
    /// - [`ConfigError::SshInvalid`] if the block names no (or an ambiguous)
    ///   auth method or host-key policy — a state the loader should have
    ///   already rejected.
    pub(crate) fn from_toml(
        ssh: &SshTunnelToml,
        id: &str,
        secrets: &dyn SecretStore,
    ) -> Result<Self, ConfigError> {
        let auth = match (&ssh.key_path, &ssh.keyring_password_ref) {
            (Some(key_path), None) => {
                let passphrase = match &ssh.keyring_passphrase_ref {
                    Some(reference) => Some(secrets.get(reference)?),
                    None => None,
                };
                SshAuth::PrivateKey {
                    path: PathBuf::from(key_path),
                    passphrase,
                }
            }
            (None, Some(password_ref)) => SshAuth::Password(secrets.get(password_ref)?),
            _ => {
                return Err(ConfigError::SshInvalid {
                    id: id.to_string(),
                    reason: "needs exactly one auth method (key_path or keyring_password_ref)"
                        .to_string(),
                })
            }
        };
        let host_key = resolve_host_key(ssh.fingerprint.as_deref(), ssh.known_hosts.as_deref())
            .ok_or_else(|| ConfigError::SshInvalid {
                id: id.to_string(),
                reason: "needs exactly one host-key policy (fingerprint or known_hosts)"
                    .to_string(),
            })?;
        Ok(Self {
            host: ssh.host.clone(),
            port: ssh.port,
            user: ssh.user.clone(),
            auth,
            host_key,
        })
    }

    /// Pair this bastion config with a forward target (the database's own
    /// host:port) to get the [`SshTunnelConfig`] the tunnel crate opens.
    pub(crate) fn into_tunnel_config(
        self,
        forward_host: String,
        forward_port: u16,
    ) -> SshTunnelConfig {
        SshTunnelConfig {
            host: self.host,
            port: self.port,
            user: self.user,
            auth: self.auth,
            host_key: self.host_key,
            forward_host,
            forward_port,
        }
    }
}

/// Map the `(fingerprint, known_hosts)` pair to a [`HostKeyPolicy`], enforcing
/// "exactly one". `None` when both or neither is set. Shared by the keyring and
/// env resolution paths so both reject an ambiguous policy identically.
fn resolve_host_key(fingerprint: Option<&str>, known_hosts: Option<&str>) -> Option<HostKeyPolicy> {
    match (fingerprint, known_hosts) {
        (Some(fp), None) => Some(HostKeyPolicy::Fingerprint(fp.to_string())),
        (None, Some(path)) => Some(HostKeyPolicy::KnownHosts(Some(PathBuf::from(path)))),
        // A tunnel must verify the host key; neither-set is not a silent
        // known_hosts default here — the loader requires one explicitly.
        _ => None,
    }
}

/// Extract the SSH forward target — the database's own `host:port` — from a
/// connection URL, filling in `default_port` when the URL omits the port.
///
/// # Errors
/// [`DbError::Connection`] if the URL cannot be parsed or names no host.
pub(crate) fn forward_target(url: &str, default_port: u16) -> DbResult<(String, u16)> {
    let parsed = Url::parse(url)
        .map_err(|e| DbError::Connection(format!("could not parse connection URL: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| DbError::Connection("connection URL has no host to tunnel to".to_string()))?
        .to_string();
    let port = parsed.port().unwrap_or(default_port);
    Ok((host, port))
}

/// Rewrite `url` so its host becomes `127.0.0.1` and its port the tunnel's
/// local forward port, preserving the userinfo, path, query, and everything
/// else. The adapter then dials the loopback forward instead of the real
/// (bastion-gated) database host.
///
/// # Errors
/// [`DbError::Connection`] if the URL cannot be parsed or the loopback host/
/// port cannot be set on it.
pub(crate) fn rewrite_to_loopback(url: &str, local_port: u16) -> DbResult<String> {
    let mut parsed = Url::parse(url)
        .map_err(|e| DbError::Connection(format!("could not parse connection URL: {e}")))?;
    parsed
        .set_host(Some("127.0.0.1"))
        .map_err(|e| DbError::Connection(format!("could not rewrite URL host: {e}")))?;
    parsed
        .set_port(Some(local_port))
        .map_err(|()| DbError::Connection("could not rewrite URL port".to_string()))?;
    Ok(parsed.to_string())
}

/// Build a [`ResolvedSsh`] straight from the process environment for the
/// env-var connection path (`DBBOARD_MYSQL_URL` + `DBBOARD_SSH_*`). Returns
/// `None` when no bastion host is configured (the common no-tunnel case).
///
/// Unlike the stored path, the passphrase/password come from the environment
/// directly (`DBBOARD_SSH_KEY_PASSPHRASE` / `DBBOARD_SSH_PASSWORD`) rather than
/// the keyring — the env path is credentials-in-env by construction.
///
/// # Errors
/// [`ConfigError::SshInvalid`] when `DBBOARD_SSH_HOST` is set but the rest of
/// the env does not name exactly one auth method and one host-key policy.
pub(crate) fn resolved_ssh_from_env(env: &SshEnv) -> Result<Option<ResolvedSsh>, ConfigError> {
    let Some(host) = env.host.clone() else {
        return Ok(None);
    };
    let bad = |reason: &str| ConfigError::SshInvalid {
        id: "env".to_string(),
        reason: reason.to_string(),
    };
    let user = env
        .user
        .clone()
        .ok_or_else(|| bad("DBBOARD_SSH_HOST is set but DBBOARD_SSH_USER is missing"))?;
    let auth = match (&env.key_path, &env.password) {
        (Some(key_path), None) => SshAuth::PrivateKey {
            path: PathBuf::from(key_path),
            passphrase: env.key_passphrase.clone(),
        },
        (None, Some(password)) => SshAuth::Password(password.clone()),
        (Some(_), Some(_)) => {
            return Err(bad(
                "set either DBBOARD_SSH_KEY_PATH or DBBOARD_SSH_PASSWORD, not both",
            ))
        }
        (None, None) => {
            return Err(bad(
                "needs an auth method: DBBOARD_SSH_KEY_PATH or DBBOARD_SSH_PASSWORD",
            ))
        }
    };
    let host_key = resolve_host_key(env.fingerprint.as_deref(), env.known_hosts.as_deref())
        .ok_or_else(|| {
            bad("needs a host-key policy: DBBOARD_SSH_FINGERPRINT or DBBOARD_SSH_KNOWN_HOSTS")
        })?;
    Ok(Some(ResolvedSsh {
        host,
        port: env.port.unwrap_or(22),
        user,
        auth,
        host_key,
    }))
}

/// Snapshot of the `DBBOARD_SSH_*` environment, captured once alongside the
/// rest of the resolver's env so the logic stays pure and testable.
#[derive(Debug, Default, Clone)]
pub(crate) struct SshEnv {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub key_path: Option<String>,
    pub key_passphrase: Option<String>,
    pub password: Option<String>,
    pub fingerprint: Option<String>,
    pub known_hosts: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_config::InMemorySecretStore;

    fn key_toml() -> SshTunnelToml {
        SshTunnelToml {
            host: "bastion.example".into(),
            port: 22,
            user: "deploy".into(),
            key_path: Some("/home/deploy/.ssh/id_ed25519".into()),
            keyring_passphrase_ref: None,
            keyring_password_ref: None,
            fingerprint: Some("SHA256:abc".into()),
            known_hosts: None,
        }
    }

    #[test]
    fn forward_target_reads_explicit_host_and_port() {
        let (host, port) = forward_target("mysql://u:p@db.internal:3307/shop", 3306).unwrap();
        assert_eq!(host, "db.internal");
        assert_eq!(port, 3307);
    }

    #[test]
    fn forward_target_falls_back_to_the_default_port() {
        let (host, port) = forward_target("postgres://u:p@db.internal/app", 5432).unwrap();
        assert_eq!(host, "db.internal");
        assert_eq!(port, 5432);
    }

    #[test]
    fn forward_target_rejects_a_urlless_string() {
        let err = forward_target("not a url", 3306).unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
    }

    #[test]
    fn rewrite_points_at_loopback_and_keeps_credentials() {
        let rewritten =
            rewrite_to_loopback("mysql://user:p%40ss@db.internal:3306/shop?ssl=true", 55001)
                .unwrap();
        assert!(
            rewritten.starts_with("mysql://user:p%40ss@127.0.0.1:55001/shop"),
            "got {rewritten}"
        );
        // The query string survives the rewrite.
        assert!(rewritten.contains("ssl=true"), "got {rewritten}");
        // The real host is gone.
        assert!(!rewritten.contains("db.internal"), "got {rewritten}");
    }

    #[test]
    fn from_toml_maps_key_auth_and_fingerprint() {
        let secrets = InMemorySecretStore::new();
        let resolved = ResolvedSsh::from_toml(&key_toml(), "conn", &secrets).unwrap();
        let cfg = resolved.into_tunnel_config("127.0.0.1".into(), 3306);
        assert_eq!(cfg.host, "bastion.example");
        assert_eq!(cfg.user, "deploy");
        assert!(matches!(cfg.auth, SshAuth::PrivateKey { .. }));
        assert!(matches!(cfg.host_key, HostKeyPolicy::Fingerprint(_)));
        assert_eq!(cfg.forward_port, 3306);
    }

    #[test]
    fn from_toml_resolves_passphrase_from_the_secret_store() {
        let mut toml = key_toml();
        toml.keyring_passphrase_ref = Some("dbboard.conn.ssh_passphrase".into());
        let secrets = InMemorySecretStore::new();
        secrets
            .set("dbboard.conn.ssh_passphrase", "unlock-me")
            .unwrap();
        let resolved = ResolvedSsh::from_toml(&toml, "conn", &secrets).unwrap();
        let cfg = resolved.into_tunnel_config("127.0.0.1".into(), 3306);
        match cfg.auth {
            SshAuth::PrivateKey { passphrase, .. } => {
                assert_eq!(passphrase.as_deref(), Some("unlock-me"));
            }
            SshAuth::Password(_) => panic!("expected key auth"),
        }
    }

    #[test]
    fn from_toml_maps_password_auth_from_the_secret_store() {
        let toml = SshTunnelToml {
            host: "bastion.example".into(),
            port: 2222,
            user: "deploy".into(),
            key_path: None,
            keyring_passphrase_ref: None,
            keyring_password_ref: Some("dbboard.conn.ssh_password".into()),
            fingerprint: None,
            known_hosts: Some("/home/deploy/.ssh/known_hosts".into()),
        };
        let secrets = InMemorySecretStore::new();
        secrets.set("dbboard.conn.ssh_password", "s3cr3t").unwrap();
        let resolved = ResolvedSsh::from_toml(&toml, "conn", &secrets).unwrap();
        let cfg = resolved.into_tunnel_config("127.0.0.1".into(), 5432);
        assert_eq!(cfg.port, 2222);
        match cfg.auth {
            SshAuth::Password(pw) => assert_eq!(pw, "s3cr3t"),
            SshAuth::PrivateKey { .. } => panic!("expected password auth"),
        }
        assert!(matches!(cfg.host_key, HostKeyPolicy::KnownHosts(Some(_))));
    }

    #[test]
    fn from_toml_missing_secret_propagates_secret_error() {
        let mut toml = key_toml();
        toml.key_path = None;
        toml.keyring_password_ref = Some("dbboard.conn.ssh_password".into());
        let secrets = InMemorySecretStore::new();
        let err = ResolvedSsh::from_toml(&toml, "conn", &secrets).unwrap_err();
        assert!(matches!(err, ConfigError::Secret(_)));
    }

    #[test]
    fn resolved_ssh_from_env_is_none_without_a_host() {
        let env = SshEnv::default();
        assert!(resolved_ssh_from_env(&env).unwrap().is_none());
    }

    #[test]
    fn resolved_ssh_from_env_builds_key_auth() {
        let env = SshEnv {
            host: Some("bastion.example".into()),
            user: Some("deploy".into()),
            key_path: Some("/k/id_ed25519".into()),
            fingerprint: Some("SHA256:abc".into()),
            ..SshEnv::default()
        };
        let resolved = resolved_ssh_from_env(&env).unwrap().unwrap();
        let cfg = resolved.into_tunnel_config("127.0.0.1".into(), 3306);
        assert_eq!(cfg.host, "bastion.example");
        assert_eq!(cfg.port, 22);
        assert!(matches!(cfg.auth, SshAuth::PrivateKey { .. }));
    }

    #[test]
    fn resolved_ssh_from_env_rejects_two_auth_methods() {
        let env = SshEnv {
            host: Some("bastion.example".into()),
            user: Some("deploy".into()),
            key_path: Some("/k".into()),
            password: Some("pw".into()),
            fingerprint: Some("SHA256:abc".into()),
            ..SshEnv::default()
        };
        let err = resolved_ssh_from_env(&env).unwrap_err();
        assert!(matches!(err, ConfigError::SshInvalid { .. }));
    }

    #[test]
    fn resolved_ssh_from_env_requires_a_host_key_policy() {
        let env = SshEnv {
            host: Some("bastion.example".into()),
            user: Some("deploy".into()),
            password: Some("pw".into()),
            ..SshEnv::default()
        };
        let err = resolved_ssh_from_env(&env).unwrap_err();
        assert!(matches!(err, ConfigError::SshInvalid { .. }));
    }
}
