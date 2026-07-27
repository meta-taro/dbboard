//! Tauri command surface for the spike.
//!
//! Each command is a thin async wrapper over [`McpService`] — the same
//! read-only service the `dbboard-mcp` stdio server exposes to external
//! agents (ADR-0046). Reusing it is the point of the spike: it proves a
//! WebView frontend can drive the egui-free core with no new DB logic,
//! only a new transport (Tauri IPC in place of JSON-RPC over stdio).
//!
//! Errors are flattened to `String` because Tauri serialises a command's
//! `Err` to the frontend as JSON; the frontend only needs the message,
//! not the typed variant.

use std::sync::Arc;

use dbboard_config::secrets::KeyringStore;
use dbboard_mcp::service::{ConnectionView, QueryOutput};
use dbboard_mcp::McpService;

/// The one managed instance backing every command. `McpService` reads
/// `connections.toml` fresh on each call and caches adapters internally,
/// so a single instance is correct for the whole app lifetime.
struct AppState {
    service: McpService,
}

/// List every configured connection (id / name / adapter kind). Never
/// includes secrets — same non-secret projection the MCP server uses.
#[tauri::command]
async fn list_connections(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ConnectionView>, String> {
    state
        .service
        .list_connections()
        .await
        .map_err(|e| e.to_string())
}

/// Table names for one connection.
#[tauri::command]
async fn list_tables(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let tables = state
        .service
        .list_tables(&connection_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(tables.into_iter().map(|t| t.name).collect())
}

/// Run a single read-only statement. Read-only is engine-enforced inside
/// the adapter (`query_read_only`), not by string matching here — a spike
/// cannot widen the write surface even by accident.
#[tauri::command]
async fn run_read_query(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    sql: String,
    max_rows: Option<usize>,
) -> Result<QueryOutput, String> {
    state
        .service
        .run_read_query(&connection_id, &sql, max_rows)
        .await
        .map_err(|e| e.to_string())
}

/// Build the service against the platform default `connections.toml` /
/// `annotations.toml` (the same files the egui app and MCP server read)
/// and run the Tauri app.
///
/// # Panics
///
/// Panics if no per-user config directory can be resolved or the Tauri
/// runtime fails to start — both are unrecoverable at launch and there is
/// no UI yet to surface them.
pub fn run() {
    let secrets = Arc::new(KeyringStore::new());
    let service = McpService::with_default_paths(secrets)
        .expect("resolve platform config paths for connections.toml");

    tauri::Builder::default()
        .manage(AppState { service })
        .invoke_handler(tauri::generate_handler![
            list_connections,
            list_tables,
            run_read_query
        ])
        .run(tauri::generate_context!())
        .expect("start the dbboard-desktop Tauri app");
}
