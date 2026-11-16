//! Tunnel error type.
//!
//! Failure modes are kept distinct on purpose (ADR-0069): a host-key
//! *mismatch* (possible man-in-the-middle) must never be reported as a bland
//! connection failure, and an authentication failure must never be confused
//! with an unreachable host.

/// Everything that can go wrong opening or probing an SSH tunnel.
#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    /// The TCP/SSH connection to the bastion could not be established.
    #[error("ssh connect to {0} failed")]
    Connect(String),

    /// The bastion rejected our credentials.
    #[error("ssh authentication failed for user '{0}'")]
    Auth(String),

    /// The server's host key did not satisfy the configured policy. This is
    /// the security-critical variant: an unknown host or — worse — a key that
    /// does not match a pinned fingerprint / `known_hosts` entry.
    #[error("host key verification failed: {0}")]
    HostKey(String),

    /// The private key file could not be read or decoded (wrong passphrase,
    /// unsupported format, missing file).
    #[error("failed to load ssh private key '{path}': {detail}")]
    KeyLoad {
        /// The key path that failed to load.
        path: String,
        /// The underlying russh/ssh-key error, rendered to a string so this
        /// crate does not leak russh's error type across its public surface.
        /// (Named `detail`, not `source`, so thiserror treats it as a plain
        /// message field rather than an `Error` source.)
        detail: String,
    },

    /// The loopback forward listener could not be bound.
    #[error("failed to open local forward listener: {0}")]
    Listener(#[source] std::io::Error),
}

/// Convenience alias for tunnel operations.
pub type TunnelResult<T> = Result<T, TunnelError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop app reads this message to offer "Edit connection" instead
    /// of a reconnect that can never work (#331, `lib/connections/failure.ts`).
    /// Changing the wording means changing that parser in the same commit.
    #[test]
    fn a_key_load_failure_names_the_path_in_quotes_then_the_reason() {
        let e = TunnelError::KeyLoad {
            path: "/k/id_ed25519".into(),
            detail: "No such file or directory (os error 2)".into(),
        };
        assert_eq!(
            e.to_string(),
            "failed to load ssh private key '/k/id_ed25519': No such file or directory (os error 2)"
        );
    }
}
