//! `dbboard-mcp` binary: serve the read-only tool surface on stdio.
//!
//! Startup wiring only (ADR-0046):
//!
//! 1. Initialise tracing to **stderr** — stdout is the MCP JSON-RPC
//!    channel and a single stray byte on it corrupts the stream.
//! 2. Resolve which `connections.toml` to read: `--config <path>` or
//!    `DBBOARD_CONFIG`, else the platform's per-user config dir (the
//!    same file the desktop GUI uses). `annotations.toml` sits beside it.
//! 3. Build a [`McpService`] backed by the OS keychain and serve a
//!    [`DbboardMcp`] over stdio until the peer disconnects (or Ctrl-C).

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use dbboard_config::secrets::KeyringStore;
use dbboard_mcp::{DbboardMcp, McpService};
use rmcp::transport::stdio;
use rmcp::ServiceExt;

const CONFIG_ENV: &str = "DBBOARD_CONFIG";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let config_override = resolve_config_override();
    let secrets = Arc::new(KeyringStore::new());
    let service = build_service(config_override, secrets)?;
    let server = DbboardMcp::new(Arc::new(service));

    tracing::info!("dbboard-mcp starting on stdio");

    // `serve` (from `ServiceExt`) drives the JSON-RPC loop on stdin/stdout;
    // `waiting()` blocks until the peer disconnects. A Ctrl-C races the
    // wait so an interactive run also exits cleanly.
    let running = server.serve(stdio()).await?;
    tokio::select! {
        result = running.waiting() => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("received Ctrl-C, shutting down");
        }
    }
    Ok(())
}

/// Build the service, honouring a `--config` / `DBBOARD_CONFIG` override.
///
/// When an override is given, `annotations.toml` is taken from the same
/// directory; otherwise both files come from the platform default dir.
fn build_service(
    config_override: Option<PathBuf>,
    secrets: Arc<KeyringStore>,
) -> Result<McpService, Box<dyn Error>> {
    match config_override {
        Some(config_path) => {
            let annotations_path = config_path.parent().map_or_else(
                || PathBuf::from("annotations.toml"),
                |dir| dir.join("annotations.toml"),
            );
            tracing::info!(config = %config_path.display(), "using config override");
            Ok(McpService::new(config_path, annotations_path, secrets))
        }
        None => Ok(McpService::with_default_paths(secrets)?),
    }
}

/// Resolve the config override from `--config <path>` (highest priority)
/// or the `DBBOARD_CONFIG` environment variable. Returns `None` to use
/// the platform default.
fn resolve_config_override() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--config=") {
            return Some(PathBuf::from(value));
        }
        if arg == "--config" {
            if let Some(value) = args.next() {
                return Some(PathBuf::from(value));
            }
        }
    }
    std::env::var_os(CONFIG_ENV).map(PathBuf::from)
}

/// Structured logging to stderr, level controlled by `RUST_LOG`
/// (default `info`). Never stdout — that is the MCP channel.
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
}
