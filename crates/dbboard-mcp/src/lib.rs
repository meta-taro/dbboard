//! `dbboard-mcp` — a headless MCP server over stdio (ADR-0046).
//!
//! Exposes the databases dbboard is already configured with
//! (`connections.toml` + the OS keychain) to an external AI agent —
//! Claude Desktop, Claude Code — as a small, **read-only** tool surface.
//! The agent can list connections, browse schemas, read rows, and see
//! dbboard's local annotations; it cannot write, and it never sees a
//! secret.
//!
//! Two layers:
//!
//! - [`service`] — [`McpService`], the transport-independent tool logic.
//!   Resolves a connection + keyring secret into an adapter, runs the
//!   five read-only operations, enforces the row cap and secret
//!   redaction. Testable without any MCP wiring.
//! - [`server`] — [`DbboardMcp`], the `rmcp` `ServerHandler` that wraps
//!   each service method as a `#[tool]` and translates errors onto the
//!   MCP envelope.
//!
//! The binary ([`main`](../main.rs)) resolves the config paths, builds a
//! [`McpService`] over the OS keychain, and serves a [`DbboardMcp`] on
//! stdio. stdout carries the JSON-RPC frames, so all logging goes to
//! stderr.

pub mod server;
pub mod service;

pub use server::DbboardMcp;
pub use service::{
    AnnotationsView, ConnectionView, McpService, QueryOutput, ServiceError, DEFAULT_MAX_ROWS,
    MAX_MAX_ROWS,
};
