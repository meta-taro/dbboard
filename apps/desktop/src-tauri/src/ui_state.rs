//! Window state that lives in files rather than in the window (ADR-0041,
//! ADR-0109).
//!
//! Two of the things this app shows are not owned by the running window: the
//! UI language, which an MCP client may change while the window is open, and
//! the instruction file an agent writes to drive the window from outside.
//! Both are polled from their own thread and announced to the frontend as
//! events, so the window follows the file rather than the other way round.

use crate::AppState;
use dbboard_mcp::service::UiLocaleView;

/// The UI language dbboard is set to, and the codes it accepts (ADR-0041).
/// `locale` is `None` when nothing has been chosen — the frontend then falls
/// back to the OS language, so `None` is a state and not a missing value.
#[tauri::command]
pub(crate) async fn get_ui_locale(
    state: tauri::State<'_, AppState>,
) -> Result<UiLocaleView, String> {
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
pub(crate) async fn set_ui_locale(
    state: tauri::State<'_, AppState>,
    locale: String,
) -> Result<(), String> {
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
pub(crate) fn watch_ui_locale(
    app: tauri::AppHandle,
    path: std::path::PathBuf,
    initial: Option<String>,
) {
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
pub(crate) fn watch_ui_command(
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
pub(crate) async fn report_ui_command_result(
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

#[cfg(test)]
mod tests {
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
}
