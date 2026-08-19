//! Tauri command surface for the desktop client.
//!
//! Each read command is a thin async wrapper over [`McpService`] — the same
//! read-only service the `dbboard-mcp` stdio server exposes to external
//! agents (ADR-0046). Sharing it is deliberate: the engine-enforced
//! read-only guarantee has one implementation, and this crate adds a
//! transport (Tauri IPC in place of JSON-RPC over stdio), not DB logic.
//!
//! Errors are flattened to `String` because Tauri serialises a command's
//! `Err` to the frontend as JSON; the frontend only needs the message,
//! not the typed variant.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

mod ai;
mod dump;
mod restore;

use dbboard_config::secrets::{KeyringStore, SecretStore};
use dbboard_config::{
    AnnotationsAdmin, ConnectionAdmin, ConnectionDraft, ConnectionEditDraft, ConnectionKind,
    ConnectionKindDraft, ConnectionKindEditDraft, FirestoreCredentialField, ImportMode,
    SecretField, SshAuthDraft, SshAuthEditDraft, SshEditField, SshHostKeyDraft, SshPassphraseField,
    SshTunnelDraft, SshTunnelEditDraft, SshTunnelToml,
};
use dbboard_core::{CellValue, RowKey, TableInfo, TableSchema, UpdatePlan, Value};
use dbboard_mcp::service::{
    AnnotationsView, ConnectionView, QueryOutput, RelationshipView, SchemaSearchView, UiLocaleView,
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
///
/// `dump_cancel` is the one shared cancellation flag for a logical backup
/// (ADR-0049): [`dump::run_dump`] polls it between tables, [`dump::cancel_dump`]
/// flips it. Only one dump runs at a time, so a single flag suffices; a new
/// run clears it first (see `dump::run_dump`). `restore_cancel` is the
/// symmetric flag for a logical restore (ADR-0051), owned by the `restore`
/// submodule — a restore and a dump are never in flight together, but keeping
/// the flags separate avoids one cancelling the other by accident.
///
/// Fields are `pub(crate)` so the `dump` and `restore` submodules' commands
/// can reach the service and the cancel flags.
pub(crate) struct AppState {
    pub(crate) service: McpService,
    pub(crate) admin: Mutex<ConnectionAdmin>,
    pub(crate) annotations: Mutex<AnnotationsAdmin>,
    pub(crate) dump_cancel: Arc<AtomicBool>,
    pub(crate) restore_cancel: Arc<AtomicBool>,
    /// The AI assistant layer (ADR-0052): the live provider slot, the optional
    /// `ai-providers.toml` admin, the shared keyring handle, and the in-flight
    /// cancel flag. Owned by the `ai` submodule; see [`ai::AiState`].
    pub(crate) ai: ai::AiState,
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

/// The UI language dbboard is set to, and the codes it accepts (ADR-0041).
/// `locale` is `None` when nothing has been chosen — the frontend then falls
/// back to the OS language, so `None` is a state and not a missing value.
#[tauri::command]
async fn get_ui_locale(state: tauri::State<'_, AppState>) -> Result<UiLocaleView, String> {
    Ok(state.service.ui_locale())
}

/// Persist the UI language to `ui-settings.toml`.
///
/// The frontend already applied the change before calling this — writing is
/// what makes it survive a restart, and what lets an MCP client see which
/// language the window is in. Unsupported codes are refused rather than
/// silently ignored, so a typo surfaces instead of leaving the app in a
/// language nobody asked for.
#[tauri::command]
async fn set_ui_locale(state: tauri::State<'_, AppState>, locale: String) -> Result<(), String> {
    state
        .service
        .set_ui_locale(&locale)
        .map_err(|e| e.to_string())
}

/// Event carrying the locale after someone outside this window changed it.
/// Payload is `Option<String>`: `null` means the choice was cleared and the
/// frontend should resolve the OS language again.
const UI_LOCALE_EVENT: &str = "ui:locale";

/// How long the watcher sleeps between reads of `ui-settings.toml`.
///
/// A second is below the threshold where a person switching the language from
/// an agent would call it broken, and the work per tick is one small TOML
/// read. Polling is deliberate: a filesystem-notify dependency would buy
/// sub-second latency that nothing here needs, on a file that is written by
/// hand a few times a day.
const UI_LOCALE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// The locale to announce, or `None` when nothing changed.
///
/// Split out from the watcher loop because the comparison is the part that
/// matters: it is on the *value*, never the file's mtime. A theme write
/// touches the same file, and an mtime comparison would then announce a
/// locale change that never happened.
fn locale_change(previous: &Option<String>, current: &Option<String>) -> Option<Option<String>> {
    (previous != current).then(|| current.clone())
}

/// Watch `ui-settings.toml` and emit [`UI_LOCALE_EVENT`] when the language
/// changes underneath the running window — an MCP client setting it, or the
/// user editing the file.
///
/// Runs on its own thread rather than the async runtime: the body is a sleep
/// and a blocking read, and this crate carries no async timer dependency.
/// A failed emit is ignored, as everywhere else here — it only means no
/// window is listening.
fn watch_ui_locale(app: tauri::AppHandle, path: std::path::PathBuf, initial: Option<String>) {
    use tauri::Emitter;

    let mut current = initial;
    loop {
        std::thread::sleep(UI_LOCALE_POLL_INTERVAL);
        let next = dbboard_config::ui_settings::load_or_default(&path).locale;
        if let Some(changed) = locale_change(&current, &next) {
            current = next;
            let _ = app.emit(UI_LOCALE_EVENT, changed);
        }
    }
}

/// Event carrying an instruction for this window from an MCP client
/// (ADR-0109). The frontend must answer every one it receives through
/// [`report_ui_command_result`], including the ones it cannot carry out —
/// the caller is blocked until it does.
const UI_COMMAND_EVENT: &str = "ui:command";

/// How long the watcher sleeps between reads of `ui-command.toml`.
///
/// Ten times faster than the locale poll, because the two files are not the
/// same kind of thing: a locale write is a preference nobody waits for, while
/// a command has a caller blocked on the answer, and every wait here is
/// charged twice — once before the window sees the instruction, once before
/// the client sees the answer.
const UI_COMMAND_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// What the watcher must treat as already handled the moment the window opens.
///
/// A command file survives the process that wrote it. Obeying whatever is in
/// it at startup would mean launching dbboard replays the last instruction of
/// a session that ended hours ago — at a window whose caller is long gone, so
/// nothing would report it. Adopting the number instead leaves the file
/// intact for anyone reading it, and answers nothing.
fn already_handled_at_startup(file: &dbboard_config::UiCommandFile) -> u64 {
    file.seq
}

/// The shape the frontend receives: `{ seq, command: { kind, ... } }`.
///
/// `seq` travels with the command because the frontend hands it straight
/// back — the answer is matched to its question by number, and a window that
/// invented its own would answer a question nobody asked.
#[derive(Clone, serde::Serialize)]
struct UiCommandEvent {
    seq: u64,
    command: dbboard_config::UiCommand,
}

/// Watch `ui-command.toml` and hand each new instruction to the frontend.
///
/// Own thread for the same reason as [`watch_ui_locale`]: the body is a sleep
/// and a blocking read. The number is advanced *before* the emit, so a
/// command that somehow fails to reach the frontend is still not retried on
/// the next tick — a duplicate `run_query` is a second query against a real
/// database, and a failure that repeats forever is worse than one that is
/// reported once.
fn watch_ui_command(
    app: tauri::AppHandle,
    command_path: std::path::PathBuf,
    result_path: std::path::PathBuf,
) {
    use tauri::Emitter;

    let mut last_acted =
        already_handled_at_startup(&dbboard_config::load_command_or_default(&command_path));
    loop {
        std::thread::sleep(UI_COMMAND_POLL_INTERVAL);
        let file = dbboard_config::load_command_or_default(&command_path);
        let Some(command) = dbboard_config::pending_command(&file, last_acted) else {
            continue;
        };
        last_acted = file.seq;
        let event = UiCommandEvent {
            seq: file.seq,
            command: command.clone(),
        };
        if app.emit(UI_COMMAND_EVENT, event).is_err() {
            // Nothing will answer, so say so here rather than leave the
            // caller to work it out from a thirty-second silence.
            let _ = dbboard_config::save_result_atomic(
                &result_path,
                &dbboard_config::UiResultFile::failed(
                    file.seq,
                    "the dbboard window could not be reached",
                ),
            );
        }
    }
}

/// Answer the instruction the window has just carried out (ADR-0109).
///
/// Called by the frontend when the work is *finished*, not when it starts:
/// an agent that asked for a query to run and got an answer before the rows
/// arrived would read the previous result as this one's.
#[tauri::command]
async fn report_ui_command_result(
    state: tauri::State<'_, AppState>,
    seq: u64,
    ok: bool,
    error: Option<String>,
    detail: Option<String>,
) -> Result<(), String> {
    let answer = dbboard_config::UiResultFile {
        version: dbboard_config::UI_COMMAND_VERSION,
        seq,
        ok,
        error,
        detail,
    };
    dbboard_config::save_result_atomic(&state.service.ui_result_path(), &answer)
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
    // Stand up the optional AI layer before the service consumes `secrets` —
    // both need the same keyring handle (the `ai.` keyring infix keeps their
    // namespaces apart). A misconfigured assistant degrades to "no provider",
    // never a launch failure (ADR-0052).
    let ai = ai::AiState::bootstrap(&secrets);
    let service = McpService::with_default_paths(secrets)
        .expect("resolve platform config paths for connections.toml");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Auto-update (ADR-0067). `updater` verifies + installs a signed
        // release; `process` gives the frontend `relaunch()` after install.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            service,
            admin: Mutex::new(admin),
            annotations: Mutex::new(annotations),
            dump_cancel: Arc::new(AtomicBool::new(false)),
            restore_cancel: Arc::new(AtomicBool::new(false)),
            ai,
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
            probe_ssh_host_key,
            add_connection,
            update_connection,
            delete_connection,
            reconnect_connection,
            export_connections,
            import_connections,
            save_text_file,
            dump::plan_dump,
            dump::run_dump,
            dump::cancel_dump,
            restore::plan_restore,
            restore::run_restore,
            restore::cancel_restore,
            ai::ai_status,
            ai::ai_explain,
            ai::ai_suggest,
            ai::cancel_ai,
            ai::list_ai_providers,
            ai::add_ai_provider,
            ai::update_ai_provider,
            ai::delete_ai_provider,
            ai::set_active_ai_provider,
            get_ui_locale,
            set_ui_locale,
            report_ui_command_result,
            update_opt_out
        ])
        // Start watching `ui-settings.toml` and `ui-command.toml` once state
        // is managed: every path comes from the service, so a watcher and the
        // writer it listens to can never end up on two different files.
        .setup(|app| {
            use tauri::Manager;

            let state = app.state::<AppState>();
            let path = state.service.ui_settings_path().to_path_buf();
            let initial = state.service.ui_locale().locale;
            let handle = app.handle().clone();
            std::thread::spawn(move || watch_ui_locale(handle, path, initial));

            let command_path = state.service.ui_command_path();
            let result_path = state.service.ui_result_path();
            let handle = app.handle().clone();
            std::thread::spawn(move || watch_ui_command(handle, command_path, result_path));
            Ok(())
        })
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

/// Env var that disables the startup auto-update check. Any non-empty value
/// opts out — parity with the egui client (ADR-0040) so the same knob silences
/// both binaries.
const UPDATE_OPT_OUT_ENV: &str = "DBBOARD_NO_UPDATE_CHECK";

/// Whether the auto-update check is disabled via [`UPDATE_OPT_OUT_ENV`]. The
/// frontend calls this before `check()` so the network request is skipped
/// entirely when the user opted out.
#[tauri::command]
fn update_opt_out() -> bool {
    opt_out(std::env::var(UPDATE_OPT_OUT_ENV).ok().as_deref())
}

/// True when the opt-out env var is present and non-empty. Split out so the
/// policy is unit-testable without mutating process env (mirrors the egui
/// client's `opt_out`).
fn opt_out(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty())
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
    // snake_case would emit `my_sql`; pin the tag to `mysql` to match the
    // frontend draft and `ConnectionKind::MySql`'s discriminator (ADR-0068).
    #[serde(rename = "mysql")]
    MySql {
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
    /// Aurora DSQL with IAM auth (ADR-0036, ADR-0103). No URL: the five plain
    /// fields are what a SigV4 token is minted from at connect time, and only
    /// the AWS secret access key is a secret.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_access_key: String,
    },
    /// Firestore (ADR-0093). A blank `service_account` means the local
    /// emulator, which has no credential — not an empty secret.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        service_account: Option<String>,
    },
    // snake_case would emit `mongo_db`; pin the tag to `mongodb` to match
    // `ConnectionKind::MongoDb`'s discriminator (ADR-0096).
    #[serde(rename = "mongodb")]
    MongoDb {
        /// The whole URI is the secret — the password rides in its authority —
        /// so it is submitted as one field rather than host/user/password parts.
        uri: String,
        database: Option<String>,
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
    #[serde(rename = "mysql")]
    MySql {
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
    /// Aurora DSQL with IAM auth (ADR-0103). Two states for the secret access
    /// key, as for D1's token: a blank box keeps the stored one, which is what
    /// makes rotating the *access key id* alone possible.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_access_key: Option<String>,
    },
    /// Firestore (ADR-0093). Three states, like the SSH passphrase:
    /// `use_emulator` drops the credential outright, otherwise a blank
    /// `service_account` keeps the stored one and a non-blank one replaces it.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        use_emulator: bool,
        service_account: Option<String>,
    },
    /// `MongoDB` (ADR-0096). Two states, not Firestore's three: a MongoDB
    /// connection always has a URI, so there is no "drop the credential" mode.
    #[serde(rename = "mongodb")]
    MongoDb {
        uri: Option<String>,
        database: Option<String>,
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

// ---- SSH tunnel DTOs (ADR-0069) ----
//
// The tunnel fronts a URL-bearing connection; the forward target (the DB
// `host:port`) is parsed from the connection URL, never stored here, so these
// DTOs only carry the bastion coordinates, auth, and host-key policy. Auth and
// host-key are tagged unions so exactly one variant's fields ever arrive —
// mirroring the config layer's "exactly one auth / exactly one host-key policy"
// invariant. Host-key verification is mandatory: there is no "accept any".

fn default_ssh_port() -> u16 {
    22
}

/// Add-time SSH auth. Secrets arrive inline (seeded into the keyring by the
/// admin layer); an absent/blank `passphrase` means the key is unencrypted.
#[derive(serde::Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum SshAuthInput {
    Key {
        key_path: String,
        passphrase: Option<String>,
    },
    Password {
        password: String,
    },
}

/// Host-key verification policy. Exactly one variant; both the add and edit
/// paths reuse it because a fingerprint / known_hosts path is not a secret.
#[derive(serde::Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
enum SshHostKeyInput {
    Fingerprint { fingerprint: String },
    KnownHosts { known_hosts: String },
}

/// Add-time SSH tunnel, as the connection form submits it.
#[derive(serde::Deserialize)]
struct SshInput {
    host: String,
    #[serde(default = "default_ssh_port")]
    port: u16,
    user: String,
    auth: SshAuthInput,
    host_key: SshHostKeyInput,
}

/// Edit-time SSH auth. Secrets are keep-or-overwrite (a blank keeps the stored
/// one); `encrypted: false` on a key means "unencrypted", distinct from "keep".
#[derive(serde::Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum SshAuthEditInput {
    Key {
        key_path: String,
        encrypted: bool,
        passphrase: Option<String>,
    },
    Password {
        password: Option<String>,
    },
}

/// Edit-time SSH intent. `keep` leaves the stored tunnel untouched, `disable`
/// removes it (secrets purged), `set` replaces it. The desktop form always
/// knows the toggle state, so it sends `disable`/`set` explicitly; `keep`
/// exists for callers with no tunnel UI.
#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SshEditInput {
    Keep,
    Disable,
    Set {
        host: String,
        #[serde(default = "default_ssh_port")]
        port: u16,
        user: String,
        auth: SshAuthEditInput,
        host_key: SshHostKeyInput,
    },
}

fn to_host_key(host_key: SshHostKeyInput) -> SshHostKeyDraft {
    match host_key {
        SshHostKeyInput::Fingerprint { fingerprint } => SshHostKeyDraft::Fingerprint(fingerprint),
        SshHostKeyInput::KnownHosts { known_hosts } => SshHostKeyDraft::KnownHosts(known_hosts),
    }
}

fn to_ssh_draft(ssh: SshInput) -> SshTunnelDraft {
    let auth = match ssh.auth {
        SshAuthInput::Key {
            key_path,
            passphrase,
        } => SshAuthDraft::Key {
            key_path,
            // An unencrypted key seeds no passphrase secret.
            passphrase: none_if_blank(passphrase),
        },
        SshAuthInput::Password { password } => SshAuthDraft::Password(password),
    };
    SshTunnelDraft {
        host: ssh.host,
        port: ssh.port,
        user: ssh.user,
        auth,
        host_key: to_host_key(ssh.host_key),
    }
}

fn to_ssh_edit_field(ssh: SshEditInput) -> SshEditField {
    let (host, port, user, auth, host_key) = match ssh {
        SshEditInput::Keep => return SshEditField::Keep,
        SshEditInput::Disable => return SshEditField::Disable,
        SshEditInput::Set {
            host,
            port,
            user,
            auth,
            host_key,
        } => (host, port, user, auth, host_key),
    };
    let auth = match auth {
        SshAuthEditInput::Key {
            key_path,
            encrypted,
            passphrase,
        } => SshAuthEditDraft::Key {
            key_path,
            passphrase: if encrypted {
                // Encrypted key: blank input keeps the stored passphrase.
                match passphrase {
                    Some(s) if !s.trim().is_empty() => SshPassphraseField::Set(s),
                    _ => SshPassphraseField::Keep,
                }
            } else {
                SshPassphraseField::Unencrypted
            },
        },
        SshAuthEditInput::Password { password } => {
            SshAuthEditDraft::Password(secret_field(password))
        }
    };
    SshEditField::Set(SshTunnelEditDraft {
        host,
        port,
        user,
        auth,
        host_key: to_host_key(host_key),
    })
}

fn to_add_draft(
    id: String,
    name: String,
    kind: KindInput,
    ssh: Option<SshInput>,
    mcp_write: bool,
    mcp_alias: Option<String>,
) -> ConnectionDraft {
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
        KindInput::MySql { url } => ConnectionKindDraft::MySql { url },
        KindInput::Neon { url } => ConnectionKindDraft::Neon { url },
        KindInput::Supabase { url } => ConnectionKindDraft::Supabase { url },
        KindInput::AuroraDsql { url } => ConnectionKindDraft::AuroraDsql { url },
        KindInput::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        } => ConnectionKindDraft::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        },
        KindInput::Firestore {
            project_id,
            database_id,
            base_url,
            service_account,
        } => ConnectionKindDraft::Firestore {
            project_id,
            database_id: none_if_blank(database_id),
            base_url: none_if_blank(base_url),
            // Blank is the emulator, so it must collapse to None rather than
            // seeding an empty-string secret that later reads as a real one.
            service_account: none_if_blank(service_account),
        },
        KindInput::MongoDb { uri, database } => ConnectionKindDraft::MongoDb {
            uri,
            // Blank means "the URI's path names it"; an empty-string database
            // would instead be written to the TOML as a real, unusable name.
            database: none_if_blank(database),
        },
    };
    ConnectionDraft {
        mcp_write,
        mcp_alias,
        id,
        name,
        kind,
        ssh: ssh.map(to_ssh_draft),
    }
}

/// Rewrite a URL-bearing kind's DSN through `graft` (ADR-0080).
///
/// The edit form composes its DSN from the parts it was shown, and those never
/// included the password — so when the user leaves the password box blank,
/// meaning "keep the stored one", the composed URL is missing a credential the
/// connection needs. `graft` puts it back, inside the process that already
/// holds it.
///
/// Kinds with no DSN, and a blank URL (the URL-mode "keep the whole secret"
/// signal), pass through untouched. MongoDB is among them despite carrying a
/// URI: its edit form shows the URI whole rather than in parts, so there is no
/// password to put back — grafting one would rewrite what the user just typed.
fn graft_url<F>(kind: KindEditInput, graft: F) -> Result<KindEditInput, String>
where
    F: Fn(&str) -> Result<String, String>,
{
    let apply = |url: Option<String>| -> Result<Option<String>, String> {
        match url {
            Some(u) if !u.trim().is_empty() => graft(&u).map(Some),
            other => Ok(other),
        }
    };
    Ok(match kind {
        KindEditInput::Postgres { url } => KindEditInput::Postgres { url: apply(url)? },
        KindEditInput::MySql { url } => KindEditInput::MySql { url: apply(url)? },
        KindEditInput::Neon { url } => KindEditInput::Neon { url: apply(url)? },
        KindEditInput::Supabase { url } => KindEditInput::Supabase { url: apply(url)? },
        KindEditInput::AuroraDsql { url } => KindEditInput::AuroraDsql { url: apply(url)? },
        other => other,
    })
}

fn to_edit_draft(
    name: String,
    kind: KindEditInput,
    ssh: SshEditInput,
    mcp_write: Option<bool>,
    mcp_alias: Option<String>,
) -> ConnectionEditDraft {
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
        KindEditInput::MySql { url } => ConnectionKindEditDraft::MySql {
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
        KindEditInput::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        } => ConnectionKindEditDraft::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key: secret_field(secret_access_key),
        },
        KindEditInput::Firestore {
            project_id,
            database_id,
            base_url,
            use_emulator,
            service_account,
        } => ConnectionKindEditDraft::Firestore {
            project_id,
            database_id: none_if_blank(database_id),
            base_url: none_if_blank(base_url),
            service_account: if use_emulator {
                // The emulator wins over anything left in the credential box,
                // matching how an unencrypted SSH key discards a typed
                // passphrase rather than half-applying the user's choice.
                FirestoreCredentialField::Emulator
            } else {
                match service_account {
                    Some(s) if !s.trim().is_empty() => FirestoreCredentialField::Set(s),
                    _ => FirestoreCredentialField::Keep,
                }
            },
        },
        KindEditInput::MongoDb { uri, database } => ConnectionKindEditDraft::MongoDb {
            uri: secret_field(uri),
            database: none_if_blank(database),
        },
    };
    ConnectionEditDraft {
        mcp_write,
        mcp_alias,
        name,
        kind,
        ssh: to_ssh_edit_field(ssh),
    }
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
    #[serde(rename = "mysql")]
    MySql {},
    Neon {},
    Supabase {},
    AuroraDsql {},
    /// Aurora DSQL with IAM auth (ADR-0103). All five plain fields come back:
    /// the AWS *access key id* is an identifier, not a credential — it is
    /// already in `connections.toml` in the clear — and the form cannot let the
    /// operator rotate it without showing which one is stored.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
    },
    /// `use_emulator` is the read-back of "no stored credential" (ADR-0093).
    /// It is not a secret — it is which mode the connection is in — so unlike
    /// the service-account JSON it can be sent to the form, which needs it to
    /// open with the right box shown.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        use_emulator: bool,
    },
    /// The URI is absent for the usual reason — it is the secret (ADR-0096).
    /// Only the explicit database name, which the TOML stores in the clear,
    /// comes back.
    #[serde(rename = "mongodb")]
    MongoDb {
        database: Option<String>,
    },
}

/// Non-secret SSH auth prefill. The passphrase/password secrets are never sent
/// back (ADR-0016); `encrypted` tells the form whether a stored passphrase
/// exists so it can render "encrypted key, leave blank to keep" vs "unencrypted".
#[derive(serde::Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum SshAuthFieldsDto {
    Key { key_path: String, encrypted: bool },
    Password {},
}

/// Non-secret host-key policy prefill.
#[derive(serde::Serialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
enum SshHostKeyFieldsDto {
    Fingerprint { fingerprint: String },
    KnownHosts { known_hosts: String },
}

/// Non-secret SSH tunnel prefill for the edit form (ADR-0069).
#[derive(serde::Serialize)]
struct SshEditFieldsDto {
    host: String,
    port: u16,
    user: String,
    auth: SshAuthFieldsDto,
    host_key: SshHostKeyFieldsDto,
}

/// The non-secret parts of a stored DSN, so the edit form can offer the same
/// host/port/user/database inputs the add form does (ADR-0080).
///
/// There is deliberately no password field: the whole point of this DTO is
/// that the edit form can be structured *without* the credential ever
/// reaching the webview.
#[derive(serde::Serialize)]
struct DsnPartsDto {
    host: String,
    port: Option<u16>,
    user: String,
    database: String,
    /// The stored query string minus its `?`, so a `ssl-mode` the user chose
    /// earlier is still what the TLS select shows when they reopen the form.
    query: String,
}

/// The edit-form prefill payload: the kind's non-secret fields (flattened so
/// the `kind` discriminator sits at the top level, unchanged) plus the tunnel
/// block when one is configured, plus the DSN parts for URL-bearing kinds.
///
/// `dsn` is `None` both for kinds that store no DSN and when the stored one
/// could not be read or parsed; the form then opens its parts empty rather
/// than refusing to open.
#[derive(serde::Serialize)]
struct EditFieldsResponse {
    #[serde(flatten)]
    kind: EditFieldsDto,
    ssh: Option<SshEditFieldsDto>,
    dsn: Option<DsnPartsDto>,
    /// Whether the MCP server may write to this connection (ADR-0087). Not a
    /// secret — it is a permission the operator granted — so unlike the DSN
    /// password it can be read back and shown as the toggle's current state.
    mcp_write: bool,
    /// The agent-facing alias, or `None` when this connection has none
    /// (ADR-0088). Sent back so the form opens with the stored alias in the
    /// box: an alias input that always opened blank would send `Some("")` on
    /// the next save and silently drop the alias the operator set.
    mcp_alias: Option<String>,
}

/// Project a stored [`dbboard_config::SshTunnelToml`] into its non-secret
/// prefill DTO. Auth method is inferred from which slot is populated (the
/// config layer guarantees exactly one), matching its own `validate()`.
fn ssh_edit_fields(ssh: &SshTunnelToml) -> SshEditFieldsDto {
    let auth = if let Some(key_path) = &ssh.key_path {
        SshAuthFieldsDto::Key {
            key_path: key_path.clone(),
            encrypted: ssh.keyring_passphrase_ref.is_some(),
        }
    } else {
        SshAuthFieldsDto::Password {}
    };
    let host_key = if let Some(fingerprint) = &ssh.fingerprint {
        SshHostKeyFieldsDto::Fingerprint {
            fingerprint: fingerprint.clone(),
        }
    } else {
        SshHostKeyFieldsDto::KnownHosts {
            // `validate()` guarantees a policy is set, so the else-arm implies
            // known_hosts; default to empty only to avoid an unwrap.
            known_hosts: ssh.known_hosts.clone().unwrap_or_default(),
        }
    };
    SshEditFieldsDto {
        host: ssh.host.clone(),
        port: ssh.port,
        user: ssh.user.clone(),
        auth,
        host_key,
    }
}

/// Read the non-secret editable fields for `id` so the edit form can prefill.
#[tauri::command]
fn connection_edit_fields(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<EditFieldsResponse, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let entry = admin
        .entries()
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("no connection with id \"{id}\""))?;
    let ssh = entry.ssh.as_ref().map(ssh_edit_fields);
    let mcp_write = entry.mcp_write;
    let mcp_alias = entry.mcp_alias.clone();
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
        ConnectionKind::MySql { .. } => EditFieldsDto::MySql {},
        ConnectionKind::Neon { .. } => EditFieldsDto::Neon {},
        ConnectionKind::Supabase { .. } => EditFieldsDto::Supabase {},
        ConnectionKind::AuroraDsql { .. } => EditFieldsDto::AuroraDsql {},
        ConnectionKind::Firestore {
            project_id,
            database_id,
            base_url,
            keyring_service_account_ref,
        } => EditFieldsDto::Firestore {
            project_id: project_id.clone(),
            database_id: database_id.clone(),
            base_url: base_url.clone(),
            use_emulator: keyring_service_account_ref.is_none(),
        },
        ConnectionKind::MongoDb { database, .. } => EditFieldsDto::MongoDb {
            database: database.clone(),
        },
        ConnectionKind::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            ..
        } => EditFieldsDto::AuroraDsqlIam {
            endpoint: endpoint.clone(),
            region: region.clone(),
            database: database.clone(),
            username: username.clone(),
            access_key_id: access_key_id.clone(),
        },
    };
    let dsn = admin
        .dsn_prefill(&id)
        .map_err(|e| e.to_string())?
        .map(|p| DsnPartsDto {
            host: p.host,
            port: p.port,
            user: p.user,
            database: p.database,
            query: p.query,
        });
    Ok(EditFieldsResponse {
        kind: dto,
        ssh,
        dsn,
        mcp_write,
        mcp_alias,
    })
}

/// Read the SSH server's host-key fingerprint so the connection form can offer
/// it for pinning. This is the SSH client's first-connection prompt, moved into
/// the form: without it the fingerprint field is a required box with no way to
/// discover its value short of running `ssh-keyscan` by hand.
///
/// Two properties make this safe to expose. It never authenticates — the probe
/// handler captures the key and then rejects it, so no credential is sent to a
/// server whose identity is still unverified. And it never writes: the returned
/// string is filled into the form for the user to confirm and save, so pinning
/// stays a deliberate act rather than trust-on-first-use behind their back.
#[tauri::command]
async fn probe_ssh_host_key(host: String, port: u16) -> Result<String, String> {
    dbboard_tunnel::probe_host_key(&host, port)
        .await
        .map_err(|e| e.to_string())
}

/// Add a connection: writes the non-secret entry to `connections.toml`
/// and the secret to the OS keyring atomically (rolled back together on
/// failure). Fails with `DuplicateId` if the id is taken.
///
/// `mcp_write` defaults to closed when the caller omits it, so a form that
/// never rendered the toggle cannot grant the MCP write permission by
/// accident (ADR-0087).
///
/// `mcp_alias` is optional for the mirror-image reason (ADR-0088): omitting it
/// leaves the connection's real id and name visible to agents, which is what a
/// caller with no alias input meant.
#[tauri::command]
fn add_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: KindInput,
    ssh: Option<SshInput>,
    mcp_write: Option<bool>,
    mcp_alias: Option<String>,
) -> Result<(), String> {
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    admin
        .add(to_add_draft(
            id,
            name,
            kind,
            ssh,
            mcp_write.unwrap_or(false),
            mcp_alias,
        ))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Edit an existing connection. The id and kind are immutable here (a
/// kind change is a delete + re-add); a blank secret keeps the stored
/// one. Evicts the read path's cached adapter so the next query rebuilds
/// with the new credentials.
///
/// `keep_password` is the structured-input counterpart of that blank-secret
/// rule (ADR-0080): the form rebuilt the DSN from host/port/user/database but
/// the user did not retype the password, so the stored one is grafted back on
/// here rather than being sent to the webview and back.
///
/// `mcp_write` is `Option` for the same reason (ADR-0087): omitting it keeps
/// whatever is stored, so a caller with no toggle cannot revoke a permission
/// it never showed. `mcp_alias` follows the same rule with one extra state
/// (ADR-0088): omitted keeps, a filled string sets, and an empty string — what
/// an emptied text input sends — clears the alias.
// The parameter list *is* the wire contract: each name is a key the webview
// sends. Folding them into one payload struct would rename every key for a
// lint, so the arity is allowed to grow with the form instead.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn update_connection(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: KindEditInput,
    ssh: SshEditInput,
    keep_password: Option<bool>,
    mcp_write: Option<bool>,
    mcp_alias: Option<String>,
) -> Result<(), String> {
    {
        let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
        let kind = if keep_password.unwrap_or(false) {
            graft_url(kind, |url| {
                admin
                    .dsn_with_stored_password(&id, url)
                    .map_err(|e| e.to_string())
            })?
        } else {
            kind
        };
        admin
            .update(&id, to_edit_draft(name, kind, ssh, mcp_write, mcp_alias))
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

/// Drop the cached adapter for `id` and open a fresh connection.
///
/// The read path already re-checks an idle adapter before handing it out, so
/// this is not needed to *recover* — it is needed to recover *now*. A user
/// looking at a pane that just failed should not have to guess whether
/// clicking again will work; through an SSH bastion the dead thing is the
/// tunnel, and only dropping the adapter rebuilds the forward.
///
/// Connecting pings, so an `Ok` here means the database answered.
#[tauri::command]
async fn reconnect_connection(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .service
        .reconnect(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Export connections (entries + secrets) to a passphrase-encrypted `.dbbx`
/// bundle at `path` (ADR-0038, ADR-0105). The frontend picks `path` with the
/// native save dialog; the encrypted blob and passphrase never cross back
/// through the WebView — we write the file here. Refuses a passphrase weaker
/// than the bundle minimum before touching the keychain.
///
/// `ids` names which connections to include. An empty list is refused by the
/// config layer rather than treated as "all": the two readings of an empty
/// selection are opposites, and guessing wrong ships either an empty bundle
/// or every credential on the machine. `None` — the field absent from the
/// IPC payload — is the explicit whole-store export.
#[tauri::command]
fn export_connections(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
    ids: Option<Vec<String>>,
) -> Result<usize, String> {
    let admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let (blob, count) = match &ids {
        Some(ids) => (
            admin
                .export_bundle_of(ids, &passphrase)
                .map_err(|e| e.to_string())?,
            ids.len(),
        ),
        None => (
            admin
                .export_bundle(&passphrase)
                .map_err(|e| e.to_string())?,
            admin.entries().len(),
        ),
    };
    std::fs::write(&path, &blob).map_err(|e| e.to_string())?;
    Ok(count)
}

/// Import connections from a `.dbbx` bundle at `path` (ADR-0038, ADR-0105).
/// `overwrite` decides what an incoming id that already exists does: replace
/// the entry and its secrets, or be skipped and reported. It defaults to
/// skipping, because that is the choice that cannot lose a credential.
/// Returns the imported/overwritten/skipped id lists for the UI to report.
#[tauri::command]
fn import_connections(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
    overwrite: Option<bool>,
) -> Result<ImportReportDto, String> {
    let mode = if overwrite.unwrap_or(false) {
        ImportMode::Overwrite
    } else {
        ImportMode::Skip
    };
    let blob = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mut admin = state.admin.lock().map_err(|_| lock_poisoned())?;
    let report = admin
        .import_bundle(&blob, &passphrase, mode)
        .map_err(|e| e.to_string())?;
    Ok(ImportReportDto {
        imported: report.imported,
        overwritten: report.overwritten,
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
    overwritten: Vec<String>,
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

    use dbboard_config::{ImportMode, InMemorySecretStore};
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

    #[test]
    fn update_opt_out_only_triggers_on_a_non_empty_value() {
        // Parity with the egui client: any non-empty DBBOARD_NO_UPDATE_CHECK
        // silences the auto-update check; absent/blank leaves it on.
        assert!(super::opt_out(Some("1")));
        assert!(super::opt_out(Some("anything")));
        assert!(!super::opt_out(Some("")));
        assert!(!super::opt_out(None));
    }

    #[test]
    fn an_unchanged_locale_emits_nothing() {
        // The watcher wakes about once a second for the life of the window.
        // Emitting on every tick would re-initialise i18n in the WebView a
        // few thousand times an hour, so "no change" must stay silent.
        assert_eq!(super::locale_change(&None, &None), None);
        assert_eq!(
            super::locale_change(&Some("ja".to_owned()), &Some("ja".to_owned())),
            None
        );
    }

    #[test]
    fn a_new_locale_is_emitted_once_it_differs() {
        assert_eq!(
            super::locale_change(&None, &Some("ko".to_owned())),
            Some(Some("ko".to_owned()))
        );
        assert_eq!(
            super::locale_change(&Some("ja".to_owned()), &Some("ko".to_owned())),
            Some(Some("ko".to_owned()))
        );
    }

    #[test]
    fn clearing_the_locale_is_a_change_too() {
        // Back to "no explicit choice" is a real state, not a missing value:
        // the frontend has to fall back to the OS language again. Emitting
        // `Some(None)` is what tells it to.
        assert_eq!(
            super::locale_change(&Some("ja".to_owned()), &None),
            Some(None)
        );
    }

    #[test]
    fn a_command_left_over_from_a_previous_session_is_not_replayed() {
        // The file outlives the process that wrote it. If opening the window
        // obeyed whatever was in it, launching dbboard would re-run the last
        // instruction of a session that ended hours ago — against a live
        // database, with nobody waiting for the answer.
        let stale = dbboard_config::UiCommandFile {
            version: dbboard_config::UI_COMMAND_VERSION,
            seq: 7,
            command: Some(dbboard_config::UiCommand::RunQuery),
        };
        let acted = super::already_handled_at_startup(&stale);
        assert_eq!(acted, 7);
        assert!(dbboard_config::pending_command(&stale, acted).is_none());

        // The next command still arrives, because the number keeps climbing.
        let fresh = dbboard_config::UiCommandFile { seq: 8, ..stale };
        assert!(dbboard_config::pending_command(&fresh, acted).is_some());
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

    // --- Connection management (write path, ADR-0062) --------------------
    //
    // These pin the two things *this* crate owns on the write side: the
    // DTO→draft mapping (blank-handling for optional/secret fields) and the
    // add/update/delete flow driving a real `ConnectionAdmin` over a temp
    // store. The commit discipline (keyring/TOML rollback) is covered by
    // `dbboard-config`'s own suite; here we prove our wiring reaches it.
    use super::{
        graft_url, none_if_blank, secret_field, ssh_edit_fields, to_add_draft, to_edit_draft,
        to_ssh_draft, to_ssh_edit_field, EditFieldsDto, ImportReportDto, KindEditInput, KindInput,
        SshAuthEditInput, SshAuthFieldsDto, SshAuthInput, SshEditInput, SshHostKeyInput, SshInput,
    };
    use dbboard_config::{
        ConnectionAdmin, ConnectionKindDraft, ConnectionKindEditDraft, FirestoreCredentialField,
        SecretField, SshAuthDraft, SshAuthEditDraft, SshEditField, SshHostKeyDraft,
        SshPassphraseField, SshTunnelToml,
    };

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

    // --- keep-the-stored-password grafting (ADR-0080) ----------------------

    #[test]
    fn graft_url_rewrites_every_url_bearing_kind() {
        let graft = |url: &str| Ok(format!("{url}#grafted"));
        let cases = vec![
            KindEditInput::Postgres {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
            KindEditInput::MySql {
                url: Some("mysql://app@db:3306/x".to_string()),
            },
            KindEditInput::Neon {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
            KindEditInput::Supabase {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
            KindEditInput::AuroraDsql {
                url: Some("postgres://app@db:5432/x".to_string()),
            },
        ];
        for kind in cases {
            let out = graft_url(kind, graft).expect("graft");
            let url = match out {
                KindEditInput::Postgres { url }
                | KindEditInput::MySql { url }
                | KindEditInput::Neon { url }
                | KindEditInput::Supabase { url }
                | KindEditInput::AuroraDsql { url } => url,
                _ => panic!("kind changed under graft_url"),
            };
            assert!(url.expect("url").ends_with("#grafted"));
        }
    }

    // A blank URL is URL-mode's own "keep the whole stored secret" signal;
    // grafting a password into nothing would compose a bogus DSN.
    #[test]
    fn graft_url_leaves_a_blank_url_alone() {
        let boom = |_: &str| -> Result<String, String> { panic!("must not graft a blank url") };
        for url in [None, Some(String::new()), Some("  ".to_string())] {
            let out = graft_url(KindEditInput::MySql { url }, boom).expect("graft");
            match out {
                KindEditInput::MySql { url } => assert!(url.unwrap_or_default().trim().is_empty()),
                _ => panic!("kind changed"),
            }
        }
    }

    #[test]
    fn graft_url_ignores_kinds_that_store_no_dsn() {
        let boom = |_: &str| -> Result<String, String> { panic!("must not graft a non-dsn kind") };
        let out = graft_url(
            KindEditInput::Turso {
                path: "./a.db".to_string(),
            },
            boom,
        )
        .expect("graft");
        assert!(matches!(out, KindEditInput::Turso { .. }));
    }

    // The stored DSN being unreadable must surface, not be swallowed into a
    // save that drops the credential.
    #[test]
    fn graft_url_propagates_a_failure() {
        let fail = |_: &str| -> Result<String, String> { Err("keychain gone".to_string()) };
        // Matched rather than `expect_err`, which would need `Debug` on
        // `KindEditInput` — a type that carries a DSN, password and all.
        match graft_url(
            KindEditInput::MySql {
                url: Some("mysql://app@db:3306/x".to_string()),
            },
            fail,
        ) {
            Err(err) => assert_eq!(err, "keychain gone"),
            Ok(_) => panic!("a failed graft must not save"),
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
            None,
            false,
            None,
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

    #[test]
    fn to_add_draft_maps_a_firestore_service_account() {
        let draft = to_add_draft(
            "fs".to_string(),
            "FS".to_string(),
            KindInput::Firestore {
                project_id: "demo-project".to_string(),
                database_id: Some("  ".to_string()),
                base_url: None,
                service_account: Some(r#"{"type":"service_account"}"#.to_string()),
            },
            None,
            false,
            None,
        );
        match draft.kind {
            ConnectionKindDraft::Firestore {
                project_id,
                database_id,
                base_url,
                service_account,
            } => {
                assert_eq!(project_id, "demo-project");
                assert!(database_id.is_none(), "blank database_id collapses to None");
                assert!(base_url.is_none());
                assert_eq!(
                    service_account.as_deref(),
                    Some(r#"{"type":"service_account"}"#)
                );
            }
            _ => panic!("expected a Firestore draft"),
        }
    }

    #[test]
    fn to_add_draft_treats_a_blank_firestore_service_account_as_the_emulator() {
        // The form hides the credential box when "use the emulator" is on, so
        // what arrives is blank — and blank must mean "no credential", not an
        // empty-string secret written into the keychain.
        let draft = to_add_draft(
            "fs".to_string(),
            "FS".to_string(),
            KindInput::Firestore {
                project_id: "demo-project".to_string(),
                database_id: None,
                base_url: Some("http://127.0.0.1:8080/v1".to_string()),
                service_account: Some("   ".to_string()),
            },
            None,
            false,
            None,
        );
        match draft.kind {
            ConnectionKindDraft::Firestore {
                base_url,
                service_account,
                ..
            } => {
                assert_eq!(base_url.as_deref(), Some("http://127.0.0.1:8080/v1"));
                assert!(service_account.is_none(), "blank → emulator");
            }
            _ => panic!("expected a Firestore draft"),
        }
    }

    #[test]
    fn to_edit_draft_firestore_maps_all_three_credential_states() {
        let with = |use_emulator: bool, service_account: Option<&str>| {
            to_edit_draft(
                "FS".to_string(),
                KindEditInput::Firestore {
                    project_id: "demo-project".to_string(),
                    database_id: None,
                    base_url: None,
                    use_emulator,
                    service_account: service_account.map(str::to_string),
                },
                SshEditInput::Keep,
                None,
                None,
            )
            .kind
        };
        let keep = with(false, Some("  "));
        assert!(
            matches!(
                keep,
                ConnectionKindEditDraft::Firestore {
                    service_account: FirestoreCredentialField::Keep,
                    ..
                }
            ),
            "blank with the emulator off → keep the stored credential"
        );
        let set = with(false, Some("{\"type\":\"service_account\"}"));
        assert!(
            matches!(
                set,
                ConnectionKindEditDraft::Firestore {
                    service_account: FirestoreCredentialField::Set(v),
                    ..
                } if v == "{\"type\":\"service_account\"}"
            ),
            "a supplied value overwrites"
        );
        // Even a supplied value is ignored once the emulator is chosen —
        // the same precedence `to_ssh_edit_field` gives an unencrypted key.
        let emulator = with(true, Some("{\"type\":\"service_account\"}"));
        assert!(
            matches!(
                emulator,
                ConnectionKindEditDraft::Firestore {
                    service_account: FirestoreCredentialField::Emulator,
                    ..
                }
            ),
            "the emulator toggle wins over a typed credential"
        );
    }

    #[test]
    fn to_add_draft_maps_a_mongodb_uri() {
        let draft = to_add_draft(
            "mg".to_string(),
            "MG".to_string(),
            KindInput::MongoDb {
                uri: "mongodb://app:hunter2@127.0.0.1:27117".to_string(),
                database: Some("  ".to_string()),
            },
            None,
            false,
            None,
        );
        match draft.kind {
            ConnectionKindDraft::MongoDb { uri, database } => {
                assert_eq!(uri, "mongodb://app:hunter2@127.0.0.1:27117");
                // The URI may name the database in its path, so blank means
                // "let the URI decide" — not an empty database name.
                assert!(database.is_none(), "blank database collapses to None");
            }
            _ => panic!("expected a MongoDB draft"),
        }
    }

    #[test]
    fn to_edit_draft_mongodb_maps_both_uri_states() {
        let with = |uri: Option<&str>| {
            to_edit_draft(
                "MG".to_string(),
                KindEditInput::MongoDb {
                    uri: uri.map(str::to_string),
                    database: Some("shop".to_string()),
                },
                SshEditInput::Keep,
                None,
                None,
            )
            .kind
        };
        assert!(
            matches!(
                with(Some("   ")),
                ConnectionKindEditDraft::MongoDb {
                    uri: SecretField::Keep,
                    ..
                }
            ),
            "blank → keep the stored URI"
        );
        assert!(
            matches!(
                with(Some("mongodb://other:27017")),
                ConnectionKindEditDraft::MongoDb {
                    uri: SecretField::Set(v),
                    database,
                } if v == "mongodb://other:27017" && database.as_deref() == Some("shop")
            ),
            "a supplied URI overwrites"
        );
    }

    #[test]
    fn graft_url_leaves_a_mongodb_uri_alone() {
        // `graft` exists for the DSN-parts form, which is pg-wire shaped. A
        // Mongo URI is edited whole, so grafting a stored password into it
        // would rewrite a URI the user just typed in full.
        let out = graft_url(
            KindEditInput::MongoDb {
                uri: Some("mongodb://app@127.0.0.1:27117".to_string()),
                database: None,
            },
            |_| panic!("graft must not be called for MongoDB"),
        )
        .expect("graft_url");
        assert!(matches!(
            out,
            KindEditInput::MongoDb { uri: Some(u), .. } if u == "mongodb://app@127.0.0.1:27117"
        ));
    }

    #[test]
    fn to_add_draft_carries_the_mcp_alias() {
        let with = |alias: Option<&str>| {
            to_add_draft(
                "d".to_string(),
                "D".to_string(),
                KindInput::Turso {
                    path: ":memory:".to_string(),
                },
                None,
                false,
                alias.map(str::to_string),
            )
            .mcp_alias
        };
        assert_eq!(with(None), None, "no alias by default (ADR-0088)");
        assert_eq!(with(Some("store-a")), Some("store-a".to_string()));
    }

    #[test]
    fn to_edit_draft_passes_the_alias_through_unchanged() {
        // The three states of ADR-0088's edit semantics have to survive the
        // trip verbatim: the config layer, not this mapper, decides that a
        // blank string clears. Flattening `Some("")` to `None` here would turn
        // "clear the alias" into "keep it" and the alias could never be removed.
        let with = |alias: Option<&str>| {
            to_edit_draft(
                "D".to_string(),
                KindEditInput::Turso {
                    path: ":memory:".to_string(),
                },
                SshEditInput::Keep,
                None,
                alias.map(str::to_string),
            )
            .mcp_alias
        };
        assert_eq!(with(None), None, "omitted → keep the stored alias");
        assert_eq!(with(Some("store-a")), Some("store-a".to_string()));
        assert_eq!(with(Some("")), Some(String::new()), "emptied → clear");
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
                None,
                false,
                None,
            ))
            .expect("add turso");
        admin
            .add(to_add_draft(
                "p".to_string(),
                "PG".to_string(),
                KindInput::Postgres {
                    url: "postgres://u:pw@h/db".to_string(),
                },
                None,
                false,
                None,
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
                    SshEditInput::Keep,
                    None,
                    None,
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
                None,
                false,
                None,
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
            None,
            false,
            None,
        ))
        .expect("seed source");

        let passphrase = "correct horse battery staple";
        let blob = src.export_bundle(passphrase).expect("export");
        let bundle_path = src_dir.path().join("bundle.dbbx");
        std::fs::write(&bundle_path, &blob).expect("write bundle");

        let (_dst_dir, mut dst) = admin_over_temp();
        let disk = std::fs::read(&bundle_path).expect("read bundle");
        let report = dst
            .import_bundle(&disk, passphrase, ImportMode::Skip)
            .expect("import");
        assert_eq!(report.imported, vec!["t".to_string()]);
        assert!(report.overwritten.is_empty());
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
            overwritten: vec!["c".to_string()],
            skipped: vec!["b".to_string()],
        };
        let json = serde_json::to_value(&dto).expect("serialize");
        for key in ["imported", "overwritten", "skipped"] {
            assert_eq!(
                json.get(key).and_then(|v| v.as_array()).unwrap().len(),
                1,
                "{key} must reach the frontend as an array"
            );
        }
    }

    // ---- SSH tunnel DTO mapping (ADR-0069) ----

    #[test]
    fn ssh_input_deserializes_the_frontend_key_auth_contract() {
        // Locks the JSON the Svelte form sends: tagged `auth.method` and
        // `host_key.policy`, with `port` optional (defaults to 22).
        let json = serde_json::json!({
            "host": "bastion.example",
            "user": "deploy",
            "auth": { "method": "key", "key_path": "/home/deploy/.ssh/id_ed25519", "passphrase": "unlock" },
            "host_key": { "policy": "fingerprint", "fingerprint": "SHA256:abc" }
        });
        let input: SshInput = serde_json::from_value(json).expect("deserialize ssh input");
        assert_eq!(input.port, 22, "omitted port defaults to 22");
        let draft = to_ssh_draft(input);
        assert_eq!(draft.host, "bastion.example");
        match draft.auth {
            SshAuthDraft::Key {
                key_path,
                passphrase,
            } => {
                assert_eq!(key_path, "/home/deploy/.ssh/id_ed25519");
                assert_eq!(passphrase.as_deref(), Some("unlock"));
            }
            SshAuthDraft::Password(_) => panic!("expected key auth"),
        }
        assert!(matches!(draft.host_key, SshHostKeyDraft::Fingerprint(f) if f == "SHA256:abc"));
    }

    #[test]
    fn to_ssh_draft_treats_a_blank_passphrase_as_an_unencrypted_key() {
        let input = SshInput {
            host: "b".to_string(),
            port: 2222,
            user: "u".to_string(),
            auth: SshAuthInput::Key {
                key_path: "/k".to_string(),
                passphrase: Some("   ".to_string()),
            },
            host_key: SshHostKeyInput::KnownHosts {
                known_hosts: "/kh".to_string(),
            },
        };
        match to_ssh_draft(input).auth {
            SshAuthDraft::Key { passphrase, .. } => {
                assert!(passphrase.is_none(), "blank passphrase → unencrypted key");
            }
            SshAuthDraft::Password(_) => panic!("expected key auth"),
        }
    }

    #[test]
    fn to_ssh_edit_field_maps_the_three_intents() {
        assert!(matches!(
            to_ssh_edit_field(SshEditInput::Keep),
            SshEditField::Keep
        ));
        assert!(matches!(
            to_ssh_edit_field(SshEditInput::Disable),
            SshEditField::Disable
        ));
    }

    #[test]
    fn to_ssh_edit_field_encrypted_key_keeps_on_blank_and_sets_otherwise() {
        let mk = |passphrase: Option<&str>| SshEditInput::Set {
            host: "b".to_string(),
            port: 22,
            user: "u".to_string(),
            auth: SshAuthEditInput::Key {
                key_path: "/k".to_string(),
                encrypted: true,
                passphrase: passphrase.map(str::to_string),
            },
            host_key: SshHostKeyInput::Fingerprint {
                fingerprint: "SHA256:x".to_string(),
            },
        };
        let keep = to_ssh_edit_field(mk(None));
        match keep {
            SshEditField::Set(d) => match d.auth {
                SshAuthEditDraft::Key { passphrase, .. } => {
                    assert!(matches!(passphrase, SshPassphraseField::Keep));
                }
                SshAuthEditDraft::Password(_) => panic!("expected key auth"),
            },
            _ => panic!("expected Set"),
        }
        let set = to_ssh_edit_field(mk(Some("new-pass")));
        match set {
            SshEditField::Set(d) => match d.auth {
                SshAuthEditDraft::Key { passphrase, .. } => {
                    assert!(matches!(passphrase, SshPassphraseField::Set(v) if v == "new-pass"));
                }
                SshAuthEditDraft::Password(_) => panic!("expected key auth"),
            },
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn to_ssh_edit_field_unencrypted_key_drops_the_passphrase() {
        let input = SshEditInput::Set {
            host: "b".to_string(),
            port: 22,
            user: "u".to_string(),
            auth: SshAuthEditInput::Key {
                key_path: "/k".to_string(),
                encrypted: false,
                // Even a supplied value is ignored when the key is unencrypted.
                passphrase: Some("ignored".to_string()),
            },
            host_key: SshHostKeyInput::Fingerprint {
                fingerprint: "SHA256:x".to_string(),
            },
        };
        match to_ssh_edit_field(input) {
            SshEditField::Set(d) => match d.auth {
                SshAuthEditDraft::Key { passphrase, .. } => {
                    assert!(matches!(passphrase, SshPassphraseField::Unencrypted));
                }
                SshAuthEditDraft::Password(_) => panic!("expected key auth"),
            },
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn ssh_edit_fields_projects_a_stored_block_without_secrets() {
        // A password-auth tunnel with a known_hosts policy. The prefill DTO
        // must carry the coordinates but never a secret value.
        let toml = SshTunnelToml {
            host: "bastion.example".to_string(),
            port: 2222,
            user: "deploy".to_string(),
            key_path: None,
            keyring_passphrase_ref: None,
            keyring_password_ref: Some("dbboard.x.ssh_password".to_string()),
            fingerprint: None,
            known_hosts: Some("/home/deploy/.ssh/known_hosts".to_string()),
        };
        let dto = ssh_edit_fields(&toml);
        assert_eq!(dto.host, "bastion.example");
        assert_eq!(dto.port, 2222);
        assert!(matches!(dto.auth, SshAuthFieldsDto::Password {}));
        let json = serde_json::to_value(&dto).expect("serialize prefill");
        let s = json.to_string();
        assert!(
            !s.contains("ssh_password") && !s.contains("keyring"),
            "prefill must not leak secret refs: {s}"
        );
        assert_eq!(json["host_key"]["policy"], "known_hosts");
    }

    #[test]
    fn ssh_edit_fields_reports_an_encrypted_key() {
        let toml = SshTunnelToml {
            host: "b".to_string(),
            port: 22,
            user: "u".to_string(),
            key_path: Some("/home/u/.ssh/id".to_string()),
            keyring_passphrase_ref: Some("dbboard.x.ssh_passphrase".to_string()),
            keyring_password_ref: None,
            fingerprint: Some("SHA256:abc".to_string()),
            known_hosts: None,
        };
        match ssh_edit_fields(&toml).auth {
            SshAuthFieldsDto::Key {
                key_path,
                encrypted,
            } => {
                assert_eq!(key_path, "/home/u/.ssh/id");
                assert!(encrypted, "a passphrase ref means the key is encrypted");
            }
            SshAuthFieldsDto::Password {} => panic!("expected key auth"),
        }
    }
}
