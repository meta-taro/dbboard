//! Logical restore (import) command surface (ADR-0051).
//!
//! The restore orchestrator and its preflight are pure and live in
//! `dbboard-core` (`run_restore` / `plan_restore`), reached here through the
//! same [`McpService`](dbboard_mcp::McpService) the read commands use — the
//! restore methods on it are deliberately **not** MCP tools, so external
//! agents stay read-only while the desktop app gains this write surface
//! (mirroring the dump path, ADR-0064, and inline cell editing, ADR-0063).
//!
//! Unlike the dump side there is no sink: a restore writes into the target
//! database through the adapter, not to a file. This module supplies the two
//! worker-side pieces the domain layer cannot hold:
//!
//! - reading the chosen `.sql` file (I/O) at both preflight and run time.
//! - [`EventControl`] — a [`RestoreControl`] that emits each
//!   [`RestoreProgress`] to the frontend as a `restore:progress` event and
//!   reads cancellation off the shared [`AtomicBool`] a [`cancel_restore`]
//!   call flips.
//!
//! `RestorePlan` is not `Serialize`, so the plan never crosses the IPC
//! boundary: [`plan_restore`] returns a flat [`RestorePlanDto`] for the
//! confirmation dialog, and [`run_restore`] re-reads and re-plans the file
//! internally before applying it — the same re-plan-on-run shape as the dump
//! path.

use std::path::Path;
use std::sync::atomic::Ordering;

use dbboard_core::{OnError, RestoreControl, RestoreOptions, RestoreProgress, StatementKind};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::AppState;

/// The Tauri event carrying each in-flight [`RestoreProgress`] to the
/// frontend progress bar. One name, emitted repeatedly; the frontend listens
/// for the duration of a single [`run_restore`] call.
const RESTORE_PROGRESS_EVENT: &str = "restore:progress";

/// The preflight summary the frontend needs to size a restore and decide
/// whether the empty-target safety gate needs a typed confirmation. The
/// counts are of the *runnable* statements only — transaction-control
/// statements (a dump's own `BEGIN`/`COMMIT`) are stripped by the runner and
/// excluded here so the numbers match what actually executes.
#[derive(Serialize)]
pub(crate) struct RestorePlanDto {
    /// Total statements that will run (everything except stripped
    /// transaction-control).
    pub statements_total: usize,
    pub ddl_count: usize,
    pub data_count: usize,
    /// Statements the classifier could not parse under the dialect. They
    /// still run verbatim (best-effort); surfaced so the UI can warn.
    pub unparsed_count: usize,
    /// The target's existing user tables. Non-empty ⇒ the run needs
    /// `confirmed = true`.
    pub existing_tables: Vec<String>,
    pub is_target_empty: bool,
}

/// Flat, `Serialize` mirror of [`dbboard_core::StatementFailure`].
#[derive(Serialize)]
pub(crate) struct StatementFailureDto {
    pub index: usize,
    pub message: String,
}

/// Flat, `Serialize` mirror of [`dbboard_core::RestoreOutcome`]. A restore
/// that ran with some statements failing on the per-statement path is still
/// a completed run — the frontend presents the partial result from
/// `failures`.
#[derive(Serialize)]
pub(crate) struct RestoreOutcomeDto {
    pub statements_run: usize,
    pub ddl_run: usize,
    pub data_run: usize,
    pub failures: Vec<StatementFailureDto>,
    pub cancelled: bool,
    /// True if the script ran as one atomic batch (all-or-nothing).
    pub atomic: bool,
}

/// `Serialize` mirror of [`RestoreProgress`] for the `restore:progress` event.
#[derive(Serialize, Clone)]
struct RestoreProgressDto {
    statements_total: usize,
    statements_done: usize,
    current_index: Option<usize>,
}

impl From<&RestoreProgress> for RestoreProgressDto {
    fn from(p: &RestoreProgress) -> Self {
        Self {
            statements_total: p.statements_total,
            statements_done: p.statements_done,
            current_index: p.current_index,
        }
    }
}

/// A [`RestoreControl`] that bridges the orchestrator to the WebView: every
/// progress snapshot is emitted as a `restore:progress` Tauri event, and
/// cancellation is read off the shared flag a [`cancel_restore`] call flips.
struct EventControl {
    app: AppHandle,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl RestoreControl for EventControl {
    fn report(&self, progress: &RestoreProgress) {
        // A failed emit means no window is listening; the restore finishes
        // harmlessly, exactly as the egui path tolerates a closed channel.
        let _ = self
            .app
            .emit(RESTORE_PROGRESS_EVENT, RestoreProgressDto::from(progress));
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Map the frontend's `on_error` string to the core policy. Anything other
/// than the explicit `"continue"` is the safe default: stop at the first
/// failure so a later statement cannot run against half-applied schema.
fn on_error_from(raw: &str) -> OnError {
    match raw {
        "continue" => OnError::Continue,
        _ => OnError::Stop,
    }
}

/// Preflight a restore of the `.sql` file at `path` into `connection_id`:
/// read and classify the script and list the target's existing tables, so
/// the frontend can show a summary and decide whether to prompt for the
/// empty-target confirmation. Reads only.
#[tauri::command]
pub(crate) async fn plan_restore(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    path: String,
) -> Result<RestorePlanDto, String> {
    let script = std::fs::read_to_string(Path::new(&path)).map_err(|e| e.to_string())?;
    let plan = state
        .service
        .plan_restore(&connection_id, &script)
        .await
        .map_err(|e| e.to_string())?;

    let runnable = plan
        .statements
        .iter()
        .filter(|s| s.kind != StatementKind::TransactionControl);
    let mut statements_total = 0;
    let mut ddl_count = 0;
    let mut data_count = 0;
    let mut unparsed_count = 0;
    for s in runnable {
        statements_total += 1;
        match s.kind {
            StatementKind::Ddl => ddl_count += 1,
            StatementKind::Data => data_count += 1,
            StatementKind::Unparsed => unparsed_count += 1,
            _ => {}
        }
    }

    Ok(RestorePlanDto {
        statements_total,
        ddl_count,
        data_count,
        unparsed_count,
        is_target_empty: plan.is_target_empty(),
        existing_tables: plan.existing_tables,
    })
}

/// Apply the `.sql` file at `path` to `connection_id`, emitting
/// `restore:progress` events throughout and returning the outcome. Re-reads
/// and re-plans the file internally (the plan is not serialisable), then runs
/// it either atomically or statement-by-statement depending on the engine.
///
/// `confirmed` must be `true` to write into a non-empty target (the frontend
/// collects the typed confirmation from the plan's `existing_tables`).
/// `on_error` (`"stop"` | `"continue"`) only affects the per-statement path.
/// A cancellation observed mid-run is not an error: the outcome carries
/// `cancelled = true` and any already-applied statements are reported.
#[tauri::command]
pub(crate) async fn run_restore(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    connection_id: String,
    path: String,
    confirmed: bool,
    on_error: String,
) -> Result<RestoreOutcomeDto, String> {
    let script = std::fs::read_to_string(Path::new(&path)).map_err(|e| e.to_string())?;
    let plan = state
        .service
        .plan_restore(&connection_id, &script)
        .await
        .map_err(|e| e.to_string())?;

    // Clear any cancellation left set by a prior run before we begin, so a
    // stale flag can never abort this restore instantly.
    state.restore_cancel.store(false, Ordering::SeqCst);
    let control = EventControl {
        app,
        cancel: std::sync::Arc::clone(&state.restore_cancel),
    };
    let options = RestoreOptions {
        confirmed,
        on_error: on_error_from(&on_error),
    };

    let outcome = state
        .service
        .run_restore(&connection_id, &plan, options, &control)
        .await
        .map_err(|e| e.to_string())?;

    Ok(RestoreOutcomeDto {
        statements_run: outcome.statements_run,
        ddl_run: outcome.ddl_run,
        data_run: outcome.data_run,
        failures: outcome
            .failures
            .into_iter()
            .map(|f| StatementFailureDto {
                index: f.index,
                message: f.message,
            })
            .collect(),
        cancelled: outcome.cancelled,
        atomic: outcome.atomic,
    })
}

/// Request cancellation of the in-flight restore. Flips the shared flag the
/// running [`run_restore`] polls between statements; the restore stops at the
/// next boundary and returns a `cancelled` outcome. On the atomic path the
/// flag is only observed before the batch starts (the batch is indivisible).
#[tauri::command]
pub(crate) fn cancel_restore(state: tauri::State<'_, AppState>) {
    state.restore_cancel.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn on_error_defaults_to_stop_and_only_continue_opts_in() {
        assert!(matches!(on_error_from("continue"), OnError::Continue));
        assert!(matches!(on_error_from("stop"), OnError::Stop));
        // Any unrecognised value is the safe default, never Continue.
        assert!(matches!(on_error_from(""), OnError::Stop));
        assert!(matches!(on_error_from("garbage"), OnError::Stop));
    }

    #[test]
    fn restore_progress_dto_mirrors_the_core_progress() {
        let p = RestoreProgress {
            statements_total: 7,
            statements_done: 3,
            current_index: Some(3),
        };
        let dto = RestoreProgressDto::from(&p);
        assert_eq!(dto.statements_total, 7);
        assert_eq!(dto.statements_done, 3);
        assert_eq!(dto.current_index, Some(3));
    }

    #[test]
    fn event_control_reads_the_shared_cancel_flag() {
        // The cancellation half is the whole contract `run_restore` polls
        // between statements; exercise it without a live `AppHandle`.
        let cancel = Arc::new(AtomicBool::new(false));
        let control = CancelOnly {
            cancel: Arc::clone(&cancel),
        };
        assert!(!control.is_cancelled());
        cancel.store(true, Ordering::SeqCst);
        assert!(control.is_cancelled());
    }

    /// A stand-in exposing only [`EventControl`]'s cancellation half so the
    /// flag contract can be unit-tested without a live `AppHandle`.
    struct CancelOnly {
        cancel: Arc<AtomicBool>,
    }

    impl CancelOnly {
        fn is_cancelled(&self) -> bool {
            self.cancel.load(Ordering::SeqCst)
        }
    }
}
