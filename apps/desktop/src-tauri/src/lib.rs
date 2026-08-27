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
mod browse;
mod connections;
mod dump;
mod restore;
mod ui_state;

use dbboard_config::secrets::{KeyringStore, SecretStore};
use dbboard_config::update_attempt;
use dbboard_config::{AnnotationsAdmin, ConnectionAdmin};
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
            browse::list_connections,
            browse::list_tables,
            browse::describe_table,
            browse::get_annotations,
            browse::set_table_note,
            browse::set_column_note,
            browse::search_schema,
            browse::list_relationships,
            browse::run_read_query,
            browse::update_row,
            config_path,
            connections::fields::connection_edit_fields,
            connections::connection_marks,
            connections::probe_ssh_host_key,
            connections::add_connection,
            connections::update_connection,
            connections::delete_connection,
            connections::move_connection,
            connections::set_connection_mark,
            connections::duplicate_connection,
            connections::repair_connection_ref,
            connections::foreign_connection_refs,
            connections::reconnect_connection,
            connections::transfer::export_connections,
            connections::transfer::import_connections,
            connections::transfer::save_text_file,
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
            ai::providers::list_ai_providers,
            ai::providers::add_ai_provider,
            ai::providers::update_ai_provider,
            ai::providers::delete_ai_provider,
            ai::providers::set_active_ai_provider,
            ui_state::get_ui_locale,
            ui_state::set_ui_locale,
            ui_state::report_ui_command_result,
            update_opt_out,
            record_update_attempt,
            take_stalled_update
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
            std::thread::spawn(move || ui_state::watch_ui_locale(handle, path, initial));

            let command_path = state.service.ui_command_path();
            let result_path = state.service.ui_result_path();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                ui_state::watch_ui_command(handle, command_path, result_path)
            });
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

/// The running build, as compiled in. The updater plugin reports the same
/// string as `currentVersion` (both come from the workspace version, which
/// `scripts/release-cut.mjs` moves in one step), so a breadcrumb written by
/// one build is always comparable with the build that reads it back.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Note that an update to `to` is about to be installed.
///
/// Called immediately before the installer takes over, because from that
/// point on this process may never run another line: ADR-0067 hands control
/// to an installer that replaces the binary and relaunches it. When the
/// relaunch does not happen, this file is the only trace left.
#[tauri::command]
fn record_update_attempt(to: String) -> Result<(), String> {
    let path = update_attempt::default_update_attempt_path().map_err(|e| e.to_string())?;
    update_attempt::record(&path, APP_VERSION, &to).map_err(|e| e.to_string())
}

/// Report an update that was started and did not land, once.
///
/// Returns `None` when there is nothing to say — no attempt, or one that
/// finished. Consumes the record either way, so the notice appears on the
/// launch after the failure and not on every launch after that.
#[tauri::command]
fn take_stalled_update() -> Result<Option<update_attempt::StalledUpdate>, String> {
    let path = update_attempt::default_update_attempt_path().map_err(|e| e.to_string())?;
    Ok(update_attempt::take(&path, APP_VERSION))
}

/// Treat a blank/whitespace optional field as absent (matches the egui
/// form's `optional()` helper for D1's `base_url`).
pub(crate) fn none_if_blank(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

pub(crate) fn lock_poisoned() -> String {
    "connection store lock was poisoned by a previous panic".to_string()
}

#[cfg(test)]
mod tests {
    //! What is left at the crate root is wiring: the commands delegate to
    //! `McpService` or to a submodule, and both are covered where they live.
    //! These two pin the only decisions made here — when the auto-update
    //! check is silenced, and what counts as an absent optional field.
    use super::none_if_blank;

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
}
