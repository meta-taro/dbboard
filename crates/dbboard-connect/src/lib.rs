//! Resolve a dbboard connection into a live [`DatabaseAdapter`].
//!
//! This is the connection factory: it turns environment variables plus
//! `connections.toml` entries (with their keyring secret refs) into a
//! [`BackendConfig`], and [`connect_adapter`] turns that config into a
//! connected, `ping()`-validated `Arc<dyn DatabaseAdapter>`.
//!
//! Extracted from `dbboard-server` in ADR-0046 so headless consumers —
//! the `dbboard-mcp` server — reuse the exact same, security-sensitive
//! connection construction the desktop GUI uses, **without** pulling in
//! an HTTP server (`axum`). `dbboard-server` re-exports from here, so its
//! public surface and the loopback HTTP contract are unchanged.
//!
//! [`DatabaseAdapter`]: dbboard_core::DatabaseAdapter

mod backend;
mod config;

pub use backend::connect_adapter;
pub use config::{
    backend_config_for_entry, backend_config_from_env, backend_config_from_env_and_store,
    resolved_connection_label, BackendConfig,
};
