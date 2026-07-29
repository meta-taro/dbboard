//! Pure-Rust SSH local-forward tunnel for bastion-gated databases (ADR-0069).
//!
//! Some databases — a work `MariaDB` bound to the server's `localhost`,
//! reachable only by first opening an SSH connection to the box — cannot be
//! connected to directly. This crate
//! opens that SSH connection itself (over [`russh`], no external `ssh`/`plink`
//! binary) and forwards a loopback port to the far-side database, so the
//! ordinary TCP adapters connect through `127.0.0.1:<port>` unchanged.
//!
//! Host-key verification is **mandatory**: [`HostKeyPolicy`] offers only
//! verifying modes (a pinned SHA-256 fingerprint or an OpenSSH `known_hosts`
//! match). There is deliberately no blind-accept path — a bad or unknown host
//! key fails the connection. Use [`probe_host_key`] to obtain a fingerprint to
//! pin on first setup.
//!
//! This is a leaf crate: it depends on `russh` + `tokio` only and knows nothing
//! about `dbboard-core` adapters. Binding the returned [`SshTunnel`] to an
//! adapter's lifetime (the `TunneledAdapter` decorator) lives in
//! `dbboard-connect`.

mod config;
mod error;
mod fingerprint;
mod tunnel;

pub use config::{HostKeyPolicy, SshAuth, SshTunnelConfig};
pub use error::{TunnelError, TunnelResult};
pub use fingerprint::{fingerprint_matches, normalize_fingerprint};
pub use tunnel::{open, probe_host_key, SshTunnel};
