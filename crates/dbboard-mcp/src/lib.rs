//! `dbboard-mcp` — a headless MCP server over stdio (ADR-0046).
//!
//! Exposes the databases dbboard is already configured with
//! (`connections.toml` + the OS keychain) to an external AI agent —
//! Claude Desktop, Claude Code — as a small tool surface. The agent can
//! list connections, browse schemas, read rows, take a dump, and see
//! dbboard's local annotations. It never sees a secret, and it cannot
//! write until a human sets `mcp_write` on a connection — and not even
//! then for privilege changes, `TRUNCATE` or `DROP`, which no setting
//! opens (ADR-0087).
//!
//! It can also see the app: `capture_window` (ADR-0108) returns a PNG of
//! the running dbboard window, so a claim about what the interface renders
//! can be checked rather than asserted.
//!
//! And it says which build it is. This binary is copied into place by hand
//! and never replaces itself, so a running instance can be several releases
//! behind the behaviour anyone expects of it. The version opens the
//! handshake instructions and is repeated by `get_server_info`
//! (ADR-0116) — two channels, because some clients drop the instructions
//! and some agents never call a tool unprompted.
//!
//! Two layers:
//!
//! - [`service`] — [`McpService`], the transport-independent tool logic.
//!   Resolves a connection + keyring secret into an adapter, runs the
//!   operations, enforces the row cap, the write policy, and secret
//!   redaction. Testable without any MCP wiring.
//! - [`server`] — [`DbboardMcp`], the `rmcp` `ServerHandler` that wraps
//!   each service method as a `#[tool]` and translates errors onto the
//!   MCP envelope.
//! - [`capture`] — the window screenshot, which belongs to neither: it
//!   holds no service state and talks to the windowing system, not a
//!   database.
//!
//! The binary ([`main`](../main.rs)) resolves the config paths, builds a
//! [`McpService`] over the OS keychain, and serves a [`DbboardMcp`] on
//! stdio. stdout carries the JSON-RPC frames, so all logging goes to
//! stderr.

pub mod capture;
pub mod server;
pub mod service;

pub use capture::{CaptureError, CaptureShot};
pub use server::DbboardMcp;
pub use service::{
    AnnotationsView, ConnectionView, DumpFileOutcome, McpService, QueryOutput, ServiceError,
    UiLocaleView, WriteOutput, DEFAULT_MAX_ROWS, MAX_MAX_ROWS,
};
