//! Reading a database, and the one write that reaches it.
//!
//! Everything here is a thin async wrapper over `McpService` (ADR-0046): the
//! command exists so the frontend has something to `invoke`, and the logic it
//! delegates to is tested in `dbboard-mcp`. The local notes are the exception
//! — those are written here, to `annotations.toml`, and never to the database
//! (ADR-0045).

use dbboard_core::{CellValue, RowKey, TableInfo, TableSchema, UpdatePlan, Value};
use dbboard_mcp::service::{
    AnnotationsView, ConnectionView, QueryOutput, RelationshipView, SchemaSearchView,
};

use crate::{lock_poisoned, none_if_blank, AppState};

/// List every configured connection (id / name / adapter kind). Never
/// includes secrets — same non-secret projection the MCP server uses.
#[tauri::command]
pub(crate) async fn list_connections(
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
pub(crate) async fn list_tables(
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
pub(crate) async fn describe_table(
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
pub(crate) async fn get_annotations(
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

/// Set (or clear) the local note for a table (ADR-0045). `table` is the
/// schema-qualified key the frontend already builds (`tableKey()` →
/// "schema.name" / bare name), matching `dbboard-config`'s `table_key`. A
/// blank/whitespace `note` deletes the note — the admin trims and prunes,
/// so callers pass the raw editor text straight through. Written to
/// `annotations.toml`, never to the database.
#[tauri::command]
pub(crate) async fn set_table_note(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    table: String,
    note: String,
) -> Result<(), String> {
    let mut admin = state.annotations.lock().map_err(|_| lock_poisoned())?;
    admin
        .set_table_note(&connection_id, &table, &note)
        .map_err(|e| e.to_string())
}

/// Set (or clear) the local note for one column of a table (ADR-0045).
/// Same key/blank-deletes semantics as [`set_table_note`].
#[tauri::command]
pub(crate) async fn set_column_note(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    table: String,
    column: String,
    note: String,
) -> Result<(), String> {
    let mut admin = state.annotations.lock().map_err(|_| lock_poisoned())?;
    admin
        .set_column_note(&connection_id, &table, &column, &note)
        .map_err(|e| e.to_string())
}

/// Find tables (and columns) whose name contains `pattern`. Blank
/// patterns are rejected by the service, not matched to everything.
#[tauri::command]
pub(crate) async fn search_schema(
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
pub(crate) async fn list_relationships(
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

/// Run a single read-only statement. Read-only is engine-enforced inside
/// the adapter (`query_read_only`), not by string matching here — a spike
/// cannot widen the write surface even by accident.
#[tauri::command]
pub(crate) async fn run_read_query(
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

// --- Inline cell editing (write path, ADR-0042) --------------------------
//
// The grid stages edits and, on Save, sends the table, the row's
// primary-key values, and the changed cells. We map those to a core
// `UpdatePlan` and run it through `McpService::apply_row_update`, which
// builds one fully-escaped `UPDATE` and executes it. This is the app's
// first DB *write* surface; it is deliberately NOT wrapped as an MCP tool,
// so external agents stay read-only (ADR-0042 write-back, ADR-0062 parity).

/// One primary-key column paired with the row's *original* value, used to
/// build the `WHERE` key. `value` is a bare JSON scalar decoded by core's
/// [`Value`] (number → Integer/Real, string → Text, null → Null,
/// `{"$blob":…}` → Blob), so a key value taken straight from a query result
/// round-trips into the update unchanged.
#[derive(serde::Deserialize)]
pub(crate) struct KeyColumnInput {
    column: String,
    value: Value,
}

/// One staged cell edit. An absent/`null` value means SQL `NULL`; a string
/// (including `""`) is written as a coerced literal — the editor never
/// conflates empty text with `NULL` (matches core's [`CellValue`]).
#[derive(serde::Deserialize)]
pub(crate) struct CellEditInput {
    column: String,
    value: Option<String>,
}

/// Apply one row's staged edits as a single `UPDATE`. `schema` is the
/// optional table schema (pass the value from [`list_tables`] verbatim);
/// `key` carries the row's primary-key columns with their original values;
/// `edits` are the changed cells. Succeeds only when exactly one row
/// changed — `0` means the row was deleted/changed under us, `>1` means the
/// "key" was not unique. Both leave the caller free to reload and retry, and
/// are surfaced to the user (parity with the egui editor's affected-row
/// gate).
#[tauri::command]
pub(crate) async fn update_row(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    schema: Option<String>,
    table: String,
    key: Vec<KeyColumnInput>,
    edits: Vec<CellEditInput>,
) -> Result<(), String> {
    let plan = UpdatePlan {
        table: match none_if_blank(schema) {
            Some(s) => TableInfo::qualified(s, table),
            None => TableInfo::unqualified(table),
        },
        key: RowKey::Columns(key.into_iter().map(|k| (k.column, k.value)).collect()),
        edits: edits
            .into_iter()
            .map(|e| {
                let value = match e.value {
                    Some(text) => CellValue::Text(text),
                    None => CellValue::Null,
                };
                (e.column, value)
            })
            .collect(),
    };
    let affected = state
        .service
        .apply_row_update(&connection_id, &plan)
        .await
        .map_err(|e| e.to_string())?;
    match affected {
        1 => Ok(()),
        0 => Err(
            "no row matched — it may have been changed or deleted since it was loaded".to_string(),
        ),
        n => Err(format!(
            "expected to update exactly one row but {n} matched — the key columns are not unique"
        )),
    }
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
        let ui_settings = dir.path().join("ui-settings.toml");
        (
            dir,
            McpService::new(config, annotations, ui_settings, secrets),
        )
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

    // --- Local annotation editing (write path, ADR-0045) -----------------
    //
    // The `set_table_note`/`set_column_note` commands are one-line
    // delegations to `AnnotationsAdmin` (whose trim/prune/empty-delete
    // discipline is covered by `dbboard-config`'s own suite). What *this*
    // crate owns is the wiring: the write admin and the read service point
    // at ONE `annotations.toml`, so a note written through the admin is
    // visible to `get_annotations` on the next call, with no cache to evict.
    // These tests pin exactly that shared-file contract.
    use dbboard_config::AnnotationsAdmin;

    /// A read `service` and a write `admin` over a *shared* `annotations.toml`,
    /// plus a temp `connections.toml` with one in-memory connection.
    fn service_and_annotations_admin() -> (tempfile::TempDir, McpService, AnnotationsAdmin) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = dir.path().join("connections.toml");
        let annotations = dir.path().join("annotations.toml");
        std::fs::write(
            &config,
            "version = 1\n\n[[connections]]\nid = \"mem\"\nname = \"Mem\"\nkind = \"turso\"\npath = \":memory:\"\n",
        )
        .expect("write config");
        let secrets = Arc::new(InMemorySecretStore::default());
        let ui_settings = dir.path().join("ui-settings.toml");
        let service = McpService::new(config, annotations.clone(), ui_settings, secrets);
        let admin = AnnotationsAdmin::new_with_file(annotations).expect("open annotations admin");
        (dir, service, admin)
    }

    #[tokio::test]
    async fn a_table_note_written_by_the_admin_is_visible_to_the_read_service() {
        let (_dir, service, mut admin) = service_and_annotations_admin();
        admin
            .set_table_note("mem", "orders", "the live orders table")
            .expect("set table note");
        let view = service
            .get_annotations("mem", Some("orders"), None)
            .await
            .expect("get_annotations");
        let ta = view.tables.first().expect("one annotated table");
        assert_eq!(ta.note.as_deref(), Some("the live orders table"));
    }

    #[tokio::test]
    async fn an_emptied_table_note_is_deleted_not_stored_blank() {
        let (_dir, service, mut admin) = service_and_annotations_admin();
        admin.set_table_note("mem", "orders", "note").expect("set");
        // A whitespace-only note clears it — the admin trims, so the command
        // hands raw editor text through and an emptied field deletes.
        admin
            .set_table_note("mem", "orders", "   ")
            .expect("clear with blanks");
        let view = service
            .get_annotations("mem", None, None)
            .await
            .expect("get_annotations");
        assert!(
            view.tables.is_empty(),
            "clearing the last note prunes the whole stanza"
        );
    }

    #[tokio::test]
    async fn a_column_note_round_trips_and_empties_delete() {
        let (_dir, service, mut admin) = service_and_annotations_admin();
        admin
            .set_column_note("mem", "orders", "status", "enum-ish free text")
            .expect("set column note");
        let view = service
            .get_annotations("mem", Some("orders"), None)
            .await
            .expect("get");
        let note = view
            .tables
            .first()
            .and_then(|t| t.columns.iter().find(|c| c.name == "status"))
            .map(|c| c.note.clone());
        assert_eq!(note.as_deref(), Some("enum-ish free text"));

        admin
            .set_column_note("mem", "orders", "status", "")
            .expect("clear column note");
        let view = service
            .get_annotations("mem", None, None)
            .await
            .expect("get");
        assert!(
            view.tables.is_empty(),
            "clearing the only column prunes the table and connection stanzas"
        );
    }
}
