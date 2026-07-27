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
use dbboard_core::{TableInfo, TableSchema};
use dbboard_mcp::service::{
    AnnotationsView, ConnectionView, QueryOutput, RelationshipView, SchemaSearchView,
};
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

/// Tables for one connection. Returns the full [`TableInfo`] (schema +
/// name), not just the name: the Structure/relationship views need the
/// schema to address a table unambiguously on engines that have schemas
/// (Postgres `public.orders`), while SQLite/libSQL tables stay unqualified.
#[tauri::command]
async fn list_tables(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<TableInfo>, String> {
    state
        .service
        .list_tables(&connection_id)
        .await
        .map_err(|e| e.to_string())
}

/// Column-level structure for one table (ordinal, type, nullable, PK,
/// default) plus the composite primary key. `schema` is optional — pass
/// the value from [`list_tables`] verbatim.
#[tauri::command]
async fn describe_table(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    schema: Option<String>,
    table: String,
) -> Result<TableSchema, String> {
    state
        .service
        .describe_table(&connection_id, schema.as_deref(), &table)
        .await
        .map_err(|e| e.to_string())
}

/// Local table/column notes (ADR-0045) for one connection, optionally
/// filtered to one table and/or column. Read-only here — the notes live
/// in `annotations.toml`, never in the database.
#[tauri::command]
async fn get_annotations(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    table: Option<String>,
    column: Option<String>,
) -> Result<AnnotationsView, String> {
    state
        .service
        .get_annotations(&connection_id, table.as_deref(), column.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Find tables (and columns) whose name contains `pattern`. Blank
/// patterns are rejected by the service, not matched to everything.
#[tauri::command]
async fn search_schema(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    pattern: String,
) -> Result<SchemaSearchView, String> {
    state
        .service
        .search_schema(&connection_id, &pattern)
        .await
        .map_err(|e| e.to_string())
}

/// Foreign-key edges for one connection, optionally filtered to those
/// touching `table` (either endpoint). Empty on engines/tables with no
/// declared foreign keys.
#[tauri::command]
async fn list_relationships(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    table: Option<String>,
) -> Result<RelationshipView, String> {
    state
        .service
        .list_relationships(&connection_id, table.as_deref())
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
            describe_table,
            get_annotations,
            search_schema,
            list_relationships,
            run_read_query,
            config_path
        ])
        .run(tauri::generate_context!())
        .expect("start the dbboard-desktop Tauri app");
}

/// Absolute path of the `connections.toml` this app reads. Pure lookup —
/// resolves the platform config path without touching the file — so the
/// frontend can tell a first-run user *where* to register a connection
/// while the app itself still has no write surface (ADR-0046/0059).
#[tauri::command]
fn config_path() -> Result<String, String> {
    dbboard_config::default_path()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    //! The commands themselves are one-line delegations to `McpService`
    //! (covered by `dbboard-mcp`'s own suite), so these tests pin the two
    //! things *this* crate owns: that the service builds against our
    //! config files, and that the read-only DTOs the frontend parses keep
    //! their JSON shape. We drive the service directly — the Tauri `State`
    //! wrapper adds no logic to test.
    use std::sync::Arc;

    use dbboard_config::InMemorySecretStore;
    use dbboard_mcp::McpService;

    /// A service over a temp `connections.toml` holding one in-memory
    /// libSQL connection. `:memory:` starts empty, so every read below is
    /// deterministic without needing a write path.
    fn service_with_memory_connection() -> (tempfile::TempDir, McpService) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = dir.path().join("connections.toml");
        let annotations = dir.path().join("annotations.toml");
        std::fs::write(
            &config,
            "version = 1\n\n[[connections]]\nid = \"mem\"\nname = \"Mem\"\nkind = \"turso\"\npath = \":memory:\"\n",
        )
        .expect("write config");
        let secrets = Arc::new(InMemorySecretStore::default());
        (dir, McpService::new(config, annotations, secrets))
    }

    #[tokio::test]
    async fn list_tables_is_empty_on_a_fresh_memory_db() {
        let (_dir, service) = service_with_memory_connection();
        let tables = service.list_tables("mem").await.expect("list_tables");
        assert!(tables.is_empty(), "fresh :memory: db has no tables");
    }

    #[tokio::test]
    async fn search_schema_rejects_a_blank_pattern() {
        let (_dir, service) = service_with_memory_connection();
        // A blank pattern must be an error, never "match everything" — the
        // command relays this straight to the frontend.
        assert!(service.search_schema("mem", "   ").await.is_err());
    }

    #[tokio::test]
    async fn relationships_view_keeps_its_frontend_json_shape() {
        let (_dir, service) = service_with_memory_connection();
        let view = service
            .list_relationships("mem", None)
            .await
            .expect("list_relationships");
        let json = serde_json::to_value(&view).expect("serialize");
        // The Svelte `RelationshipView` type keys off exactly these fields.
        assert!(json.get("connection_id").is_some());
        assert!(json.get("relationships").is_some());
        assert!(json.get("truncated").is_some());
    }

    #[tokio::test]
    async fn annotations_view_is_empty_and_serializable_for_an_unannotated_db() {
        let (_dir, service) = service_with_memory_connection();
        let view = service
            .get_annotations("mem", None, None)
            .await
            .expect("get_annotations");
        assert_eq!(view.connection_id, "mem");
        assert!(view.tables.is_empty());
        // Field names the frontend depends on.
        let json = serde_json::to_value(&view).expect("serialize");
        assert!(json.get("connection_id").is_some());
        assert!(json.get("tables").is_some());
    }
}
