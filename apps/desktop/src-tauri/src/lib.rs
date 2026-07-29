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

use std::sync::{Arc, Mutex};

use dbboard_config::secrets::{KeyringStore, SecretStore};
use dbboard_config::{
    AnnotationsAdmin, ConnectionAdmin, ConnectionDraft, ConnectionEditDraft, ConnectionKind,
    ConnectionKindDraft, ConnectionKindEditDraft, SecretField,
};
use dbboard_core::{CellValue, RowKey, TableInfo, TableSchema, UpdatePlan, Value};
use dbboard_mcp::service::{
    AnnotationsView, ConnectionView, QueryOutput, RelationshipView, SchemaSearchView,
};
use dbboard_mcp::McpService;

/// The managed state backing every command.
///
/// `service` is the read path: [`McpService`] reads `connections.toml`
/// fresh on each call and caches adapters. `admin` is the write path:
/// [`ConnectionAdmin`] owns the same `connections.toml` plus the keyring
/// and is the *only* thing that mutates them (ADR-0062). The two share
/// one `connections.toml`, so after a write we evict the matching cached
/// adapter from `service` to keep the read path from serving stale
/// credentials.
///
/// `annotations` is the note write path: [`AnnotationsAdmin`] owns
/// `annotations.toml` (the same file `service` reads, and the egui app and
/// MCP server share). Notes never touch the database or the read adapters,
/// so — unlike a connection write — a note write needs no cache eviction;
/// `service` re-reads the file on the next `get_annotations` (ADR-0045).
struct AppState {
    service: McpService,
    admin: Mutex<ConnectionAdmin>,
    annotations: Mutex<AnnotationsAdmin>,
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

/// Set (or clear) the local note for a table (ADR-0045). `table` is the
/// schema-qualified key the frontend already builds (`tableKey()` →
/// "schema.name" / bare name), matching `dbboard-config`'s `table_key`. A
/// blank/whitespace `note` deletes the note — the admin trims and prunes,
/// so callers pass the raw editor text straight through. Written to
/// `annotations.toml`, never to the database.
#[tauri::command]
async fn set_table_note(
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
async fn set_column_note(
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
    let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());
    let path =
        dbboard_config::default_path().expect("resolve platform config paths for connections.toml");
    let admin = ConnectionAdmin::open(path, Arc::clone(&secrets))
        .expect("open connections.toml for connection management");
    let annotations =
        AnnotationsAdmin::open_default().expect("open annotations.toml for local note editing");
    let service = McpService::with_default_paths(secrets)
        .expect("resolve platform config paths for connections.toml");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            service,
            admin: Mutex::new(admin),
            annotations: Mutex::new(annotations),
        })
        .invoke_handler(tauri::generate_handler![
            list_connections,
            list_tables,
            describe_table,
            get_annotations,
            set_table_note,
            set_column_note,
            search_schema,
            list_relationships,
            run_read_query,
            update_row,
            config_path,
            connection_edit_fields,
            add_connection,
            update_connection,
            delete_connection,
            export_connections,
            import_connections,
            save_text_file
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
struct KeyColumnInput {
    column: String,
    value: Value,
}

/// One staged cell edit. An absent/`null` value means SQL `NULL`; a string
/// (including `""`) is written as a coerced literal — the editor never
/// conflates empty text with `NULL` (matches core's [`CellValue`]).
#[derive(serde::Deserialize)]
struct CellEditInput {
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
async fn update_row(
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

// --- Connection management (write path, ADR-0062) ------------------------
//
// The frontend speaks these Deserialize DTOs; we map them to the
// `dbboard-config` draft types so the Svelte contract stays decoupled from
// the crate's internal enums. `#[serde(tag = "kind")]` matches the same
// `kind` discriminator the read-side `ConnectionView` already carries.

/// Add-time kind + inline secret, as the connection form submits it.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum KindInput {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
        token: String,
    },
    Postgres {
        url: String,
    },
    Neon {
        url: String,
    },
    Supabase {
        url: String,
    },
    AuroraDsql {
        url: String,
    },
}

/// Edit-time kind. Secret fields are `Option`: absent or blank means
/// "keep the stored secret" (the existing value is never sent back to the
/// UI, ADR-0016); a non-blank value replaces it.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum KindEditInput {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
        token: Option<String>,
    },
    Postgres {
        url: Option<String>,
    },
    Neon {
        url: Option<String>,
    },
    Supabase {
        url: Option<String>,
    },
    AuroraDsql {
        url: Option<String>,
    },
}

/// Treat a blank/whitespace optional field as absent (matches the egui
/// form's `optional()` helper for D1's `base_url`).
fn none_if_blank(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

/// A blank secret input means "leave the keyring entry alone"; anything
/// else overwrites it. The value is stored verbatim (never trimmed) —
/// only the blank check trims.
fn secret_field(v: Option<String>) -> SecretField {
    match v {
        Some(s) if !s.trim().is_empty() => SecretField::Set(s),
        _ => SecretField::Keep,
    }
}

fn to_add_draft(id: String, name: String, kind: KindInput) -> ConnectionDraft {
    let kind = match kind {
        KindInput::Turso { path } => ConnectionKindDraft::Turso { path },
        KindInput::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } => ConnectionKindDraft::D1 {
            account_id,
            database_id,
            base_url: none_if_blank(base_url),
            token,
        },
        KindInput::Postgres { url } => ConnectionKindDraft::Postgres { url },
        KindInput::Neon { url } => ConnectionKindDraft::Neon { url },
        KindInput::Supabase { url } => ConnectionKindDraft::Supabase { url },
        KindInput::AuroraDsql { url } => ConnectionKindDraft::AuroraDsql { url },
    };
    ConnectionDraft { id, name, kind }
}

fn to_edit_draft(name: String, kind: KindEditInput) -> ConnectionEditDraft {
    let kind = match kind {
        KindEditInput::Turso { path } => ConnectionKindEditDraft::Turso { path },
        KindEditInput::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } => ConnectionKindEditDraft::D1 {
            account_id,
            database_id,
            base_url: none_if_blank(base_url),
            token: secret_field(token),
        },
        KindEditInput::Postgres { url } => ConnectionKindEditDraft::Postgres {
            url: secret_field(url),
        },
        KindEditInput::Neon { url } => ConnectionKindEditDraft::Neon {
            url: secret_field(url),
        },
        KindEditInput::Supabase { url } => ConnectionKindEditDraft::Supabase {
            url: secret_field(url),
        },
        KindEditInput::AuroraDsql { url } => ConnectionKindEditDraft::AuroraDsql {
            url: secret_field(url),
        },
    };
    ConnectionEditDraft { name, kind }
}

/// The non-secret editable fields of one connection, so the edit form can
/// prefill without ever reading a secret back out of the keyring (ADR-0016).
/// Secret fields (D1 token, the Postgres-family URL) are intentionally
/// absent — the form leaves them blank, meaning "keep the stored secret".
/// The `kind` discriminator is snake_case to match the frontend's draft
/// model (and `AuroraDsql` → `aurora_dsql`).
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EditFieldsDto {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
    },
    Postgres {},
    Neon {},
    Supabase {},
    AuroraDsql {},
}

/// Read the non-secret editable fields for `id` so the edit form can prefill.
/// Aurora DSQL (IAM) is config-file-only and has no in-app editor (parity
/// with egui), so it is rejected here rather than silently mis-rendered.
#[tauri::command]
fn connection_edit_fields(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<EditFieldsDto, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let entry = admin
        .entries()
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("no connection with id \"{id}\""))?;
    let dto = match &entry.kind {
        ConnectionKind::Turso { path } => EditFieldsDto::Turso { path: path.clone() },
        ConnectionKind::D1 {
            account_id,
            database_id,
            base_url,
            ..
        } => EditFieldsDto::D1 {
            account_id: account_id.clone(),
            database_id: database_id.clone(),
            base_url: base_url.clone(),
        },
        ConnectionKind::Postgres { .. } => EditFieldsDto::Postgres {},
        ConnectionKind::Neon { .. } => EditFieldsDto::Neon {},
        ConnectionKind::Supabase { .. } => EditFieldsDto::Supabase {},
        ConnectionKind::AuroraDsql { .. } => EditFieldsDto::AuroraDsql {},
        ConnectionKind::AuroraDsqlIam { .. } => {
            return Err(
                "Aurora DSQL (IAM) connections are configured in connections.toml \
                 and cannot be edited in-app"
                    .to_string(),
            )
        }
    };
    Ok(dto)
}

/// Add a connection: writes the non-secret entry to `connections.toml`
/// and the secret to the OS keyring atomically (rolled back together on
/// failure). Fails with `DuplicateId` if the id is taken.
#[tauri::command]
fn add_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: KindInput,
) -> Result<(), String> {
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    admin
        .add(to_add_draft(id, name, kind))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Edit an existing connection. The id and kind are immutable here (a
/// kind change is a delete + re-add); a blank secret keeps the stored
/// one. Evicts the read path's cached adapter so the next query rebuilds
/// with the new credentials.
#[tauri::command]
async fn update_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: KindEditInput,
) -> Result<(), String> {
    {
        let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
        admin
            .update(&id, to_edit_draft(name, kind))
            .map_err(|e| e.to_string())?;
    } // drop the guard before awaiting — keeps the command future Send.
    state.service.invalidate(&id).await;
    Ok(())
}

/// Delete a connection and purge its keyring secrets, then evict any
/// cached adapter for it.
#[tauri::command]
async fn delete_connection(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    {
        let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
        admin.delete(&id).map_err(|e| e.to_string())?;
    }
    state.service.invalidate(&id).await;
    Ok(())
}

/// Export every connection (entries + secrets) to a passphrase-encrypted
/// `.dbbx` bundle at `path` (ADR-0038). The frontend picks `path` with the
/// native save dialog; the encrypted blob and passphrase never cross back
/// through the WebView — we write the file here. Refuses a passphrase weaker
/// than the bundle minimum before touching the keychain.
#[tauri::command]
fn export_connections(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<usize, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let blob = admin
        .export_bundle(&passphrase)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, &blob).map_err(|e| e.to_string())?;
    Ok(admin.entries().len())
}

/// Import connections from a `.dbbx` bundle at `path` (ADR-0038). Additive
/// and non-destructive: an incoming id that already exists is skipped, never
/// overwritten. Returns the imported/skipped id lists for the UI to report.
#[tauri::command]
fn import_connections(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<ImportReportDto, String> {
    let blob = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let report = admin
        .import_bundle(&blob, &passphrase)
        .map_err(|e| e.to_string())?;
    Ok(ImportReportDto {
        imported: report.imported,
        skipped: report.skipped,
    })
}

/// Write a UTF-8 text file to `path` (ADR-0035 result-set export). The
/// frontend builds the delimited body (with its leading BOM for the `.csv`
/// form) and picks `path` with the native save dialog, so this is a thin,
/// deliberate writer — nothing here is fabricated and the path is always a
/// destination the user just chose. Kept in Rust (rather than a WebView blob
/// download) so the save lands at the chosen path with a real "Save As"
/// dialog, mirroring the connection-bundle export.
#[tauri::command]
fn save_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents.as_bytes()).map_err(|e| e.to_string())
}

/// Serialize-only mirror of `dbboard_config::ImportReport` (which is
/// Deserialize-oriented internally) so the frontend gets a stable JSON shape.
#[derive(serde::Serialize)]
struct ImportReportDto {
    imported: Vec<String>,
    skipped: Vec<String>,
}

fn lock_poisoned() -> String {
    "connection store lock was poisoned by a previous panic".to_string()
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
        let service = McpService::new(config, annotations.clone(), secrets);
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

    // --- Connection management (write path, ADR-0062) --------------------
    //
    // These pin the two things *this* crate owns on the write side: the
    // DTO→draft mapping (blank-handling for optional/secret fields) and the
    // add/update/delete flow driving a real `ConnectionAdmin` over a temp
    // store. The commit discipline (keyring/TOML rollback) is covered by
    // `dbboard-config`'s own suite; here we prove our wiring reaches it.
    use super::{
        none_if_blank, secret_field, to_add_draft, to_edit_draft, EditFieldsDto, ImportReportDto,
        KindEditInput, KindInput,
    };
    use dbboard_config::{ConnectionAdmin, ConnectionKindDraft, SecretField};

    #[test]
    fn none_if_blank_treats_whitespace_as_absent() {
        assert_eq!(none_if_blank(None), None);
        assert_eq!(none_if_blank(Some("   ".to_string())), None);
        assert_eq!(none_if_blank(Some("\t\n".to_string())), None);
        assert_eq!(
            none_if_blank(Some(" v ".to_string())),
            Some(" v ".to_string()),
            "a non-blank value is passed through verbatim, not trimmed"
        );
    }

    #[test]
    fn secret_field_keeps_on_blank_and_sets_verbatim_otherwise() {
        assert!(matches!(secret_field(None), SecretField::Keep));
        assert!(matches!(
            secret_field(Some("  ".to_string())),
            SecretField::Keep
        ));
        // A real secret is stored exactly as typed — surrounding spaces can
        // be significant in a URL/token, so only the blank check trims.
        match secret_field(Some(" tok ".to_string())) {
            SecretField::Set(v) => assert_eq!(v, " tok "),
            SecretField::Keep => panic!("a non-blank secret must Set, not Keep"),
        }
    }

    #[test]
    fn to_add_draft_maps_d1_and_drops_a_blank_base_url() {
        let draft = to_add_draft(
            "d".to_string(),
            "D".to_string(),
            KindInput::D1 {
                account_id: "acct".to_string(),
                database_id: "db".to_string(),
                base_url: Some("   ".to_string()),
                token: "t".to_string(),
            },
        );
        assert_eq!(draft.id, "d");
        assert_eq!(draft.name, "D");
        match draft.kind {
            ConnectionKindDraft::D1 {
                account_id,
                database_id,
                base_url,
                token,
            } => {
                assert_eq!(account_id, "acct");
                assert_eq!(database_id, "db");
                assert!(base_url.is_none(), "blank base_url collapses to None");
                assert_eq!(token, "t");
            }
            _ => panic!("expected a D1 draft"),
        }
    }

    /// A `ConnectionAdmin` over a throwaway `connections.toml` paired with an
    /// in-memory keyring, so add/update/delete never touch the real OS store.
    fn admin_over_temp() -> (tempfile::TempDir, ConnectionAdmin) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::default());
        let admin = ConnectionAdmin::open(path, secrets).expect("open admin");
        (dir, admin)
    }

    #[test]
    fn add_update_delete_flow_over_a_temp_store() {
        let (_dir, mut admin) = admin_over_temp();

        admin
            .add(to_add_draft(
                "t".to_string(),
                "Turso".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
            ))
            .expect("add turso");
        admin
            .add(to_add_draft(
                "p".to_string(),
                "PG".to_string(),
                KindInput::Postgres {
                    url: "postgres://u:pw@h/db".to_string(),
                },
            ))
            .expect("add postgres");
        assert_eq!(admin.entries().len(), 2);

        // Rename only (blank secret → Keep the stored URL). Must not error
        // for lack of a resupplied secret.
        admin
            .update(
                "p",
                to_edit_draft(
                    "PG-renamed".to_string(),
                    KindEditInput::Postgres { url: None },
                ),
            )
            .expect("rename postgres, keep secret");
        let pg = admin
            .entries()
            .iter()
            .find(|e| e.id == "p")
            .expect("postgres still present");
        assert_eq!(pg.name, "PG-renamed");

        admin.delete("t").expect("delete turso");
        let ids: Vec<&str> = admin.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["p"], "only the postgres entry survives");
    }

    #[test]
    fn add_rejects_a_duplicate_id() {
        let (_dir, mut admin) = admin_over_temp();
        let mk = || {
            to_add_draft(
                "dup".to_string(),
                "One".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
            )
        };
        admin.add(mk()).expect("first add");
        assert!(admin.add(mk()).is_err(), "a taken id must be rejected");
    }

    #[test]
    fn export_then_import_roundtrips_through_a_file() {
        // Mirrors the export_connections/import_connections command bodies:
        // encrypt one store to a `.dbbx` file, import it into a fresh store,
        // and confirm the entry crosses over intact.
        let (src_dir, mut src) = admin_over_temp();
        src.add(to_add_draft(
            "t".to_string(),
            "Turso".to_string(),
            KindInput::Turso {
                path: ":memory:".to_string(),
            },
        ))
        .expect("seed source");

        let passphrase = "correct horse battery staple";
        let blob = src.export_bundle(passphrase).expect("export");
        let bundle_path = src_dir.path().join("bundle.dbbx");
        std::fs::write(&bundle_path, &blob).expect("write bundle");

        let (_dst_dir, mut dst) = admin_over_temp();
        let disk = std::fs::read(&bundle_path).expect("read bundle");
        let report = dst.import_bundle(&disk, passphrase).expect("import");
        assert_eq!(report.imported, vec!["t".to_string()]);
        assert!(report.skipped.is_empty());
        assert_eq!(dst.entries().len(), 1);
    }

    #[test]
    fn edit_fields_dto_matches_the_frontend_draft_shape() {
        // The `kind` tag must be snake_case (draft.ts keys off it), and D1
        // must carry its non-secret fields but never the token.
        let turso = serde_json::to_value(EditFieldsDto::Turso {
            path: ":memory:".to_string(),
        })
        .expect("serialize turso");
        assert_eq!(turso.get("kind").unwrap(), "turso");
        assert_eq!(turso.get("path").unwrap(), ":memory:");

        let d1 = serde_json::to_value(EditFieldsDto::D1 {
            account_id: "a".to_string(),
            database_id: "b".to_string(),
            base_url: None,
        })
        .expect("serialize d1");
        assert_eq!(d1.get("kind").unwrap(), "d1");
        assert!(
            d1.get("token").is_none(),
            "a secret must never be serialized"
        );

        // AuroraDsql collapses to the snake_case discriminator the form uses.
        let aurora = serde_json::to_value(EditFieldsDto::AuroraDsql {}).expect("serialize aurora");
        assert_eq!(aurora.get("kind").unwrap(), "aurora_dsql");
    }

    #[test]
    fn import_report_dto_keeps_its_frontend_json_shape() {
        let dto = ImportReportDto {
            imported: vec!["a".to_string()],
            skipped: vec!["b".to_string()],
        };
        let json = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(
            json.get("imported")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            json.get("skipped")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            1
        );
    }
}
