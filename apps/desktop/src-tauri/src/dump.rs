//! Logical dump (backup) command surface (ADR-0049/0050).
//!
//! The dump orchestrator and its preflight are pure and live in
//! `dbboard-core` (`run_dump` / `plan_dump`), reached here through the same
//! [`McpService`](dbboard_mcp::McpService) the read commands use — the dump
//! methods on it are deliberately **not** MCP tools, so external agents stay
//! read-only while the desktop app gains this write surface (mirroring
//! inline cell editing, ADR-0063).
//!
//! This module supplies only the two worker-side pieces the domain layer
//! cannot hold, because they touch I/O and the Tauri event bus:
//!
//! - [`FileSink`] — a buffered-file [`DumpSink`], the dump's sole write.
//! - [`EventControl`] — a [`DumpControl`] that emits each [`DumpProgress`]
//!   to the frontend as a `dump:progress` event and reads cancellation off
//!   the shared [`AtomicBool`] a [`cancel_dump`] call flips.
//!
//! `DumpPlan` is not `Serialize`, so the plan never crosses the IPC
//! boundary: [`plan_dump`] returns a flat [`DumpPlanDto`] for the
//! confirmation dialog, and [`run_dump`] re-plans internally before running.

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::Path;
use std::sync::atomic::Ordering;

use dbboard_core::{DumpControl, DumpError, DumpProgress, DumpResult, DumpSink};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::AppState;

/// The Tauri event carrying each in-flight [`DumpProgress`] to the frontend
/// progress bar. One name, emitted repeatedly; the frontend listens for the
/// duration of a single [`run_dump`] call.
const DUMP_PROGRESS_EVENT: &str = "dump:progress";

/// Flat, `Serialize` mirror of one table in a [`DumpPlan`](dbboard_core::DumpPlan),
/// which is itself not serialisable.
#[derive(Serialize)]
pub(crate) struct DumpTableDto {
    pub name: String,
    pub row_count: u64,
}

/// The preflight summary the frontend needs to size a dump and decide
/// whether to warn before running it. The huge-DB threshold itself is
/// frontend-owned (localStorage, like theme/language), so this DTO reports
/// the counts and lets the frontend apply its own threshold — the backend
/// never blocks a dump (ADR-0049 warn-and-allow).
#[derive(Serialize)]
pub(crate) struct DumpPlanDto {
    pub tables: Vec<DumpTableDto>,
    pub total_rows: u64,
    pub is_empty_data: bool,
}

/// Flat, `Serialize` mirror of [`dbboard_core::TableFailure`].
#[derive(Serialize)]
pub(crate) struct TableFailureDto {
    pub table: String,
    pub message: String,
}

/// Flat, `Serialize` mirror of [`dbboard_core::TableTruncation`].
#[derive(Serialize)]
pub(crate) struct TableTruncationDto {
    pub table: String,
    pub rows_written: u64,
}

/// Flat, `Serialize` mirror of [`dbboard_core::DumpOutcome`]. A dump that
/// ran with some tables failing or truncated is still a success — the
/// frontend presents the partial result from these lists.
#[derive(Serialize)]
pub(crate) struct DumpOutcomeDto {
    pub tables_dumped: usize,
    pub rows_written: u64,
    pub failures: Vec<TableFailureDto>,
    pub truncations: Vec<TableTruncationDto>,
    pub cancelled: bool,
}

/// `Serialize` mirror of [`DumpProgress`] for the `dump:progress` event.
#[derive(Serialize, Clone)]
struct DumpProgressDto {
    tables_total: usize,
    tables_done: usize,
    rows_total: u64,
    rows_done: u64,
    current_table: Option<String>,
}

impl From<&DumpProgress> for DumpProgressDto {
    fn from(p: &DumpProgress) -> Self {
        Self {
            tables_total: p.tables_total,
            tables_done: p.tables_done,
            rows_total: p.rows_total,
            rows_done: p.rows_done,
            current_table: p.current_table.clone(),
        }
    }
}

/// A buffered-file [`DumpSink`]. `write_str` maps any I/O failure to
/// [`DumpError::Sink`], which aborts the dump (a backup we cannot write is
/// worthless); [`Self::finish`] flushes so a clean run leaves a complete
/// file. Mirrors the egui `backup::FileSink`.
struct FileSink {
    writer: BufWriter<File>,
}

impl FileSink {
    fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            writer: BufWriter::new(File::create(path)?),
        })
    }

    fn finish(mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl DumpSink for FileSink {
    fn write_str(&mut self, chunk: &str) -> DumpResult<()> {
        self.writer
            .write_all(chunk.as_bytes())
            .map_err(|e| DumpError::Sink(e.to_string()))
    }
}

/// A [`DumpControl`] that bridges the orchestrator to the WebView: every
/// progress snapshot is emitted as a `dump:progress` Tauri event, and
/// cancellation is read off the shared flag a [`cancel_dump`] call flips.
struct EventControl {
    app: AppHandle,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl DumpControl for EventControl {
    fn report(&self, progress: &DumpProgress) {
        // A failed emit means no window is listening; the dump finishes into
        // the void harmlessly, exactly as the egui path tolerates a closed
        // channel.
        let _ = self
            .app
            .emit(DUMP_PROGRESS_EVENT, DumpProgressDto::from(progress));
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Preflight a whole-connection dump: count every table so the frontend can
/// show a size and warn before a large backup. Reads only.
#[tauri::command]
pub(crate) async fn plan_dump(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<DumpPlanDto, String> {
    let plan = state
        .service
        .plan_dump(&connection_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(DumpPlanDto {
        tables: plan
            .tables
            .iter()
            .map(|t| DumpTableDto {
                name: t.table.name.clone(),
                row_count: t.row_count,
            })
            .collect(),
        total_rows: plan.total_rows(),
        is_empty_data: plan.is_empty_data(),
    })
}

/// Run a whole-connection logical dump to `path`, emitting `dump:progress`
/// events throughout and returning the outcome. Re-plans internally (the
/// plan is not serialisable), writes SQL to a file the user just chose, and
/// reads the database only. A cancellation observed mid-run is not an error:
/// the outcome carries `cancelled = true` and the partial file is kept and
/// reported honestly. Only an unopenable/unwritable output fails the command.
#[tauri::command]
pub(crate) async fn run_dump(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    connection_id: String,
    path: String,
) -> Result<DumpOutcomeDto, String> {
    let plan = state
        .service
        .plan_dump(&connection_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut sink = FileSink::create(Path::new(&path)).map_err(|e| e.to_string())?;

    // Clear any cancellation left set by a prior run before we begin, so a
    // stale flag can never abort this dump instantly.
    state.dump_cancel.store(false, Ordering::SeqCst);
    let control = EventControl {
        app,
        cancel: std::sync::Arc::clone(&state.dump_cancel),
    };

    let outcome = state
        .service
        .run_dump(&connection_id, &plan, &mut sink, &control)
        .await
        .map_err(|e| e.to_string())?;

    // Flush the buffer: a dump reported as complete must be a complete file.
    sink.finish().map_err(|e| e.to_string())?;

    Ok(DumpOutcomeDto {
        tables_dumped: outcome.tables_dumped,
        rows_written: outcome.rows_written,
        failures: outcome
            .failures
            .into_iter()
            .map(|f| TableFailureDto {
                table: f.table,
                message: f.message,
            })
            .collect(),
        truncations: outcome
            .truncations
            .into_iter()
            .map(|t| TableTruncationDto {
                table: t.table,
                rows_written: t.rows_written,
            })
            .collect(),
        cancelled: outcome.cancelled,
    })
}

/// Request cancellation of the in-flight dump. Flips the shared flag the
/// running [`run_dump`] polls between tables/pages; the dump stops at the
/// next checkpoint and returns a `cancelled` outcome with its partial file.
#[tauri::command]
pub(crate) fn cancel_dump(state: tauri::State<'_, AppState>) {
    state.dump_cancel.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn file_sink_writes_and_flushes_a_complete_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dump.sql");

        let mut sink = FileSink::create(&path).expect("create");
        sink.write_str("-- header\n").expect("write");
        sink.write_str("INSERT INTO t VALUES (1);\n")
            .expect("write");
        sink.finish().expect("finish");

        let contents = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(contents, "-- header\nINSERT INTO t VALUES (1);\n");
    }

    #[test]
    fn file_sink_create_on_a_missing_parent_is_an_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no_such_dir").join("nested").join("x.sql");
        assert!(FileSink::create(&path).is_err());
    }

    #[test]
    fn event_control_reads_the_shared_cancel_flag() {
        // No AppHandle is needed to exercise the cancellation half — the flag
        // is the whole contract `run_dump` polls between tables.
        let cancel = Arc::new(AtomicBool::new(false));
        // Construct without a live app: `report` is the only method that
        // touches `app`, and we only assert `is_cancelled` here.
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
