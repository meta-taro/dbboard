//! Presentation layer for dbboard.
//!
//! The UI is an HTTP client of the in-process loopback server
//! (ADR-0009). It never links a database adapter; instead it sends
//! [`Command`]s and receives [`Reply`]s over a pair of
//! `std::sync::mpsc` channels. A background [`worker`] thread owns a
//! `reqwest` client and translates that channel traffic into HTTP
//! calls against the server, keeping the synchronous egui thread free
//! of blocking I/O. The crate still depends only on `dbboard-core`
//! among workspace crates.
//!
//! Use [`DbboardApp::connect`] to wire the app to a running server;
//! [`DbboardApp::new`] is the lower-level constructor over raw channels
//! (used by [`connect`](DbboardApp::connect) and by tests).

mod ai;
mod ai_settings;
mod backup;
mod client;
mod connections;
mod edit;
mod errors;
mod export;
mod history;
mod restore;
mod selection;
/// Central design system: branded palette, theme-aware semantic colours,
/// and the [`theme::apply`] entry point the binary calls at startup.
pub mod theme;
mod worker;

pub use ai::{AiMode, AiPanel, AiResponseView};
pub use ai_settings::AiSettingsView;
pub use connections::{
    AddFormState, ConnectionsView, EditFormState, EditKindState, KindSelector, Mode,
};
pub use history::{
    AiEntry, AiIntent, AiStatus, HistoryEntry, HistoryError, HistoryStatus, HistoryStore,
    PersistentHistoryStore, QueryEntry, AI_TEXT_CAP_BYTES, AI_TEXT_TRUNCATED_MARKER,
    CURRENT_VERSION, DEFAULT_CAPACITY, ROTATION_BYTES, ROTATION_LINES,
};
// Fixture-emission shim for the `dbboard-web` sibling's
// cross-implementation round-trip test (ADR-0017). Used only by the
// `emit_history_fixture` example — hidden from rustdoc; do not call
// from production code.
#[doc(hidden)]
pub use history::fixture;
pub use worker::{AiProviderSlot, AiProviderSwitcher, ConnectionSwitcher, SchemaSource};
// Re-export so the desktop binary can implement [`ConnectionSwitcher`]
// (return type `Result<(), DbError>`) and [`SchemaSource`] (return type
// `Arc<dyn DatabaseAdapter>`) without taking a direct dep on
// `dbboard-core` — the architecture rule is that only the server and
// adapters link to `dbboard-core` (see CLAUDE.md).
pub use dbboard_ai::{AiError, AiProvider, StopReason};
pub use dbboard_core::{DatabaseAdapter, DbError};

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, PoisonError};
use std::time::Instant;

use dbboard_config::{table_key as annotation_table_key, AnnotationsAdmin};
use dbboard_core::{
    sorted_row_order, ColumnInfo, DbResult, DumpOutcome, DumpPlan, DumpProgress, OnError,
    QueryResult, RestoreOptions, RestoreOutcome, RestorePlan, RestoreProgress, Row, SortKey,
    SqlDialect, TableInfo, TableSchema, Value, DEFAULT_BACKUP_WARN_ROWS,
};
use dbboard_i18n::{t, t_args};
use eframe::egui;

/// Request flowing UI → worker.
#[derive(Debug, Clone)]
pub enum Command {
    /// Refresh the sidebar list of user tables.
    ListTables,
    /// Run an arbitrary SQL statement entered in the editor.
    Query(String),
    /// Swap the running server's adapter to the named connection
    /// (ADR-0020). The id is the same one [`ConnectionAdmin`] uses;
    /// resolving and connecting it is delegated to the binary via
    /// [`worker::ConnectionSwitcher`].
    SwitchConnection { id: String },
    /// AI: explain the given SQL via the injected provider (ADR-0023).
    /// Routed to `AiProvider::explain` by the worker; never traverses
    /// the HTTP loopback. Surfaces as `Reply::AiResponded` /
    /// `Reply::AiFailed`. `dialect` is an optional adapter-id hint.
    AiExplain {
        sql: String,
        dialect: Option<String>,
    },
    /// AI: suggest SQL for the given prompt via the injected provider
    /// (ADR-0023). `schema` is the active connection's `list_tables()`
    /// snapshot, used as the provider's schema hint. `full_schema`
    /// (ADR-0028 Decision 8) carries the per-table `describe_table`
    /// results when the panel's "Include column details" toggle was on
    /// and a [`Reply::SchemaPrefetched`] round-trip preceded this send;
    /// `None` preserves the names-only behaviour.
    AiSuggest {
        prompt: String,
        dialect: Option<String>,
        schema: Vec<TableInfo>,
        full_schema: Option<Vec<TableSchema>>,
    },
    /// Swap the active AI provider to the entry named `id` from
    /// `ai-providers.toml` (ADR-0025). In-process, not HTTP — the swap
    /// is delegated to an injected [`worker::AiProviderSwitcher`]
    /// supplied by the binary. Surfaces as `Reply::AiProviderSwitched`
    /// or `Reply::AiProviderSwitchFailed`.
    SwitchAiProvider { id: String },
    /// Streaming counterpart to [`Command::AiExplain`] (ADR-0026
    /// Decision 6). Routed to `AiProvider::stream_explain` by the
    /// worker. Each provider chunk surfaces as [`Reply::AiChunk`];
    /// the stream terminates with [`Reply::AiStreamComplete`] or with
    /// [`Reply::AiFailed`] on mid-stream error. A [`Command::CancelAiRequest`]
    /// arriving while this stream is in flight drops the underlying
    /// future and surfaces [`Reply::AiCancelled`].
    AiExplainStream {
        sql: String,
        dialect: Option<String>,
    },
    /// Streaming counterpart to [`Command::AiSuggest`] (ADR-0026
    /// Decision 6). Same dispatch / reply semantics as
    /// [`Command::AiExplainStream`]; `full_schema` follows the
    /// [`Command::AiSuggest`] contract (ADR-0028 Decision 8).
    AiSuggestStream {
        prompt: String,
        dialect: Option<String>,
        schema: Vec<TableInfo>,
        full_schema: Option<Vec<TableSchema>>,
    },
    /// Cancel the in-flight AI request, if any (ADR-0026 Decision 5).
    /// The worker drops the active stream / one-shot future and emits
    /// [`Reply::AiCancelled`]; no-op when no request is in flight.
    /// The cancel button surfaces this in both streaming and atomic
    /// paths per ADR-0026 Decision 10.
    CancelAiRequest,
    /// Fan out `describe_table` over the listed tables before a Suggest
    /// fires (ADR-0028 Decision 9). In-process, not HTTP — the worker
    /// snapshots the live adapter through the injected
    /// [`worker::SchemaSource`] and answers with
    /// [`Reply::SchemaPrefetched`]. Issued by the AI panel only when
    /// the "Include column details" toggle is on and the adapter
    /// advertises `has_describe_table`.
    PrefetchSchema { tables: Vec<TableInfo> },
    /// Describe a single table for the structure tab (ADR-0031). Same
    /// in-process `describe_table` path as [`Command::PrefetchSchema`],
    /// but scoped to one table and answered with [`Reply::TableDescribed`]
    /// so the structure view stays independent of the AI prefetch flow.
    /// Issued when the user clicks a table in the sidebar.
    DescribeTable { table: TableInfo },
    /// Preflight a logical-dump backup of the live connection (ADR-0049
    /// slice e): list the tables and `COUNT(*)` each. In-process via the
    /// injected [`worker::SchemaSource`], answered with
    /// [`Reply::BackupPlanned`]. The UI uses the returned plan to show the
    /// huge-DB warning and size the progress bar before it opens the save
    /// dialog and sends [`Command::StartBackup`].
    PlanBackup,
    /// Run the backup to `path`, using the plan a preceding
    /// [`Command::PlanBackup`] produced (ADR-0049 slice e). The worker
    /// snapshots the live adapter, derives its dialect, and spawns the dump
    /// task — emitting [`Reply::BackupProgress`] as it pages and a terminal
    /// [`Reply::BackupComplete`] / [`Reply::BackupFailed`]. Cancellable via
    /// [`Command::CancelBackup`].
    StartBackup { path: PathBuf, plan: DumpPlan },
    /// Cancel the in-flight backup, if any (ADR-0049 Decision 9). The dump
    /// stops at the next table/page boundary and still reports a
    /// [`Reply::BackupComplete`] whose outcome is marked cancelled, so the
    /// partial file is surfaced honestly rather than as an error.
    CancelBackup,
    /// Preflight a logical restore of the `.sql` file at `path` (ADR-0051):
    /// read it, classify its statements, and list the target's existing
    /// tables. In-process via the injected [`worker::SchemaSource`], answered
    /// with [`Reply::RestorePlanned`]. The UI uses the returned plan to size
    /// the progress bar and — when the target is not empty — to require the
    /// caller's confirmation before it sends [`Command::StartRestore`].
    PlanRestore { path: PathBuf },
    /// Apply the plan a preceding [`Command::PlanRestore`] produced (ADR-0051).
    /// The worker snapshots the live adapter and spawns the restore task —
    /// emitting [`Reply::RestoreProgress`] as it applies statements and a
    /// terminal [`Reply::RestoreComplete`] / [`Reply::RestoreFailed`].
    /// Cancellable via [`Command::CancelRestore`]. `options.confirmed` carries
    /// the empty-target-gate acknowledgement the UI collected.
    StartRestore {
        plan: RestorePlan,
        options: RestoreOptions,
    },
    /// Cancel the in-flight restore, if any (ADR-0051). The restore stops at
    /// the next statement boundary (per-statement engines) and still reports a
    /// [`Reply::RestoreComplete`] whose outcome is marked cancelled, so a
    /// partial restore is surfaced honestly rather than as an error. An atomic
    /// engine that has not yet committed unwinds cleanly.
    CancelRestore,
}

/// Result flowing worker → UI.
#[derive(Debug)]
pub enum Reply {
    Tables(DbResult<Vec<TableInfo>>),
    QueryResult(DbResult<QueryResult>),
    /// The swap succeeded; the named connection is now active. The UI
    /// uses this to update the active-row marker and to stamp `id` on
    /// subsequent history records.
    ConnectionSwitched {
        id: String,
    },
    /// The swap failed; the previous adapter is still live. The id of
    /// the failed target travels with the error so the UI can show
    /// "could not connect to <id>".
    SwitchFailed {
        id: String,
        error: DbError,
    },
    /// AI provider returned a response (ADR-0023). The panel replaces
    /// any stale content with `text`; token counts are recorded for the
    /// future cost-meter wiring deferred to Stage 2.
    ///
    /// `provider`/`model` (ADR-0027 Decision 4) carry the spawn-time
    /// identity the worker snapshotted for this request — same on every
    /// terminal reply variant so a mid-flight `SwitchAiProvider` never
    /// re-labels an in-flight response. `slice (c)` uses these to stamp
    /// the AI history record.
    AiResponded {
        text: String,
        tokens_in: u32,
        tokens_out: u32,
        provider: String,
        model: String,
    },
    /// AI request failed (ADR-0023). The panel renders the error using
    /// its own translation table — the AI taxonomy is independent of
    /// the HTTP `DbError` taxonomy (ADR-0023 Decision 8).
    ///
    /// See [`Reply::AiResponded`] for the `provider`/`model` contract.
    /// A no-provider failure surfaces here with `provider = "unknown"`
    /// / `model = ""` — the identity is nominal, but the record stays
    /// well-formed (ADR-0027 §Implementation Slice b).
    AiFailed {
        error: AiError,
        provider: String,
        model: String,
    },
    /// AI provider swap succeeded (ADR-0025). The Settings UI (slice b)
    /// uses this to update the active-row marker and dismiss any prior
    /// switch error.
    AiProviderSwitched {
        id: String,
    },
    /// AI provider swap failed (ADR-0025). `reason` carries the
    /// `AiError::Display` text so the panel can show it inline without
    /// re-translating the AI taxonomy through `DbError`. The previous
    /// provider (if any) remains live; the swap is atomic.
    AiProviderSwitchFailed {
        reason: String,
    },
    /// One chunk of an in-flight AI stream (ADR-0026 Decision 6).
    /// `text_delta` is the incremental text to append to the panel's
    /// accumulated response (empty string for usage-only events such as
    /// the initial `message_start`). `tokens_in` / `tokens_out` are
    /// `Some` only on events that carry cumulative usage (typically the
    /// initial `message_start` and the final `message_delta`); the UI
    /// **replaces** the running meter with `Some` values, leaves it
    /// alone otherwise (ADR-0026 Decision 7 — Anthropic
    /// `usage.output_tokens` is cumulative, not incremental).
    AiChunk {
        text_delta: String,
        tokens_in: Option<u32>,
        tokens_out: Option<u32>,
    },
    /// Terminal marker for a successful AI stream (ADR-0026 Decision 6).
    /// Carries the final cumulative token counts and the provider's
    /// `stop_reason`. The panel clears its busy flag here and persists
    /// the final tokens to its visible meter.
    ///
    /// See [`Reply::AiResponded`] for the `provider`/`model` contract.
    AiStreamComplete {
        tokens_in: u32,
        tokens_out: u32,
        stop_reason: StopReason,
        provider: String,
        model: String,
    },
    /// The in-flight AI request was cancelled by the user (ADR-0026
    /// Decision 5). Reset the panel's busy flag and render "Cancelled."
    /// without surfacing an error banner (ADR-0026 Decision 12). Emitted
    /// for both the streaming and the atomic dispatch paths
    /// (Decision 10).
    ///
    /// See [`Reply::AiResponded`] for the `provider`/`model` contract.
    /// Even a pre-first-chunk cancel carries the spawn-time identity so
    /// the eventual history record (slice c) has a stable label.
    AiCancelled {
        provider: String,
        model: String,
    },
    /// Result of a [`Command::PrefetchSchema`] fan-out (ADR-0028
    /// Decision 9). `schemas` holds the successful `describe_table`
    /// results in the input's table order; `errors` pairs each failed
    /// table with its error message. Partial failure is non-blocking:
    /// the panel surfaces a warning banner and fires the pending
    /// Suggest with whatever `schemas` carries.
    SchemaPrefetched {
        schemas: Vec<TableSchema>,
        errors: Vec<(TableInfo, String)>,
    },
    /// Result of a [`Command::DescribeTable`] (ADR-0031). Carries the
    /// requested table so a stale reply for a since-reselected table can
    /// be ignored, and the `describe_table` outcome for the structure tab.
    TableDescribed {
        table: TableInfo,
        result: DbResult<TableSchema>,
    },
    /// Result of a [`Command::PlanBackup`] preflight (ADR-0049 slice e).
    /// `Ok` carries the per-table plan the UI needs to warn on a huge DB
    /// and size the progress bar; `Err` surfaces a listing failure (or an
    /// unsupported connection) so the UI can abandon the backup.
    BackupPlanned {
        result: DbResult<DumpPlan>,
    },
    /// One progress snapshot of an in-flight backup (ADR-0049 slice e).
    /// The UI replaces its running meter with `progress` and repaints.
    BackupProgress {
        progress: DumpProgress,
    },
    /// Terminal marker for a finished backup (ADR-0049 slice e). Carries
    /// the outcome — rows written, per-table failures, truncations, and
    /// whether it was cancelled — so the UI can render an honest summary.
    BackupComplete {
        outcome: DumpOutcome,
    },
    /// The backup could not be written (ADR-0049 slice e): the output file
    /// could not be created or a write failed. `message` is the OS error.
    /// Adapter-side per-table failures are *not* here — those ride in
    /// [`Reply::BackupComplete`]'s outcome and never abort the run.
    BackupFailed {
        message: String,
    },
    /// Result of a [`Command::PlanRestore`] preflight (ADR-0051). `Ok` carries
    /// the classified plan plus the target's existing tables, which the UI
    /// uses to size the progress bar and decide whether the empty-target
    /// confirmation is needed; `Err` surfaces an unreadable file, an
    /// unsupported connection, or a listing failure so the UI can abandon the
    /// restore.
    RestorePlanned {
        result: DbResult<RestorePlan>,
    },
    /// One progress snapshot of an in-flight restore (ADR-0051). The UI
    /// replaces its running meter with `progress` and repaints.
    RestoreProgress {
        progress: RestoreProgress,
    },
    /// Terminal marker for a finished restore (ADR-0051). Carries the outcome —
    /// statements applied, per-statement failures, and whether it was
    /// cancelled — so the UI can render an honest summary.
    RestoreComplete {
        outcome: RestoreOutcome,
    },
    /// The restore could not be applied (ADR-0051): a non-empty target the
    /// caller did not confirm, an adapter that cannot execute writes, or an
    /// atomic batch that unwound. `message` is the fatal error. Per-statement
    /// failures on the non-atomic path are *not* here — those ride in
    /// [`Reply::RestoreComplete`]'s outcome and never abort the run.
    RestoreFailed {
        message: String,
    },
}

/// Captures everything we know at submit time about a query whose reply
/// has not yet arrived. The completion-time path consumes this on
/// [`Reply::QueryResult`] to build the rich ADR-0017 record (`duration_ms`
/// from `started.elapsed()`, `sql` carried through verbatim).
struct PendingSubmit {
    started: Instant,
    sql: String,
}

/// Submit-time snapshot for an in-flight AI request (ADR-0027 slice c).
/// Kept until a terminal AI reply lands, at which point it is combined
/// with the reply's `provider` / `model` (the spawn-time identity from
/// ADR-0027 slice b) to build the [`AiEntry`] recorded on disk.
///
/// `intent` and `prompt` are captured from the outgoing [`Command`]
/// before it is sent to the worker; `conn` is the active connection id
/// at that moment, or `None` for the in-memory-only path where the
/// label is empty.
struct PendingAiSubmit {
    started: Instant,
    intent: AiIntent,
    prompt: String,
    conn: Option<String>,
}

/// Wall-clock function used to stamp `ts` on every completion record.
/// Injected (rather than calling `SystemTime::now()` directly) so
/// `dbboard-ui` stays free of any date-formatting crate dependency and
/// so tests can pass a deterministic stub.
pub type RfcClock = fn() -> String;

/// Which tab the lower panel shows (ADR-0031). Defaults to `Results`;
/// clicking a sidebar table switches to `Structure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultTab {
    Results,
    Structure,
}

/// Structure-tab state (ADR-0031): the table whose schema is on screen and
/// the latest `describe_table` outcome. `schema == None` means the describe
/// is still in flight.
#[derive(Debug)]
struct StructureView {
    table: TableInfo,
    schema: Option<DbResult<TableSchema>>,
    /// Per-column note edit buffers (column name -> in-progress text),
    /// lazily seeded from the stored note the first time each column
    /// renders (ADR-0045). Held on the view so switching tables drops
    /// any half-typed note instead of leaking it onto the next table.
    note_buffers: BTreeMap<String, String>,
    /// Table-level note buffer, seeded and dropped like `note_buffers`.
    /// `None` until the first render seeds it from the stored note.
    table_note_buffer: Option<String>,
}

/// Which annotation field a structure-tab edit commits to (ADR-0045).
/// The connection id and table key are derived from the live
/// [`StructureView`] at commit time, so only the leaf differs here.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NoteTarget {
    Table,
    Column(String),
}

/// The open inline editor (issue 0013 slice b): a single-line text field
/// swapped in over one result cell after a double-click. `just_opened`
/// requests keyboard focus on the first frame; blur stages the buffer.
struct CellEditor {
    row: usize,
    col: usize,
    buffer: String,
    just_opened: bool,
}

/// An in-flight inline-edit save (issue 0013 slice b). Each staged row is
/// one keyed `UPDATE`; they run one at a time through the existing
/// `Command::Query` path. `current` is the statement awaiting its reply;
/// `remaining` is the rest of the queue. A reply advances the queue in
/// `drain_replies` (see [`DbboardApp::advance_save`]).
struct SaveQueue {
    current: edit::PlannedUpdate,
    remaining: VecDeque<edit::PlannedUpdate>,
}

/// What the result grid asks the app to do after a frame (issue 0013
/// slice b). Bubbled out of the free `render_result` function so the
/// mutating action runs against `&mut self` once the grid's borrows end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridIntent {
    /// Run the staged edits (Save button).
    Save,
    /// Drop all staged edits (Discard button).
    Discard,
}

/// Inline cell-editing state (issue 0013 slice b), attached to the current
/// result. It is only *populated* when the result came from a browse
/// `SELECT` of a single base table (see [`DbboardApp::run_table_browse`]);
/// an arbitrary query, a `COUNT(*)`, or a view/join leaves `source`
/// `None` and the grid stays read-only. All heavy logic lives in the pure
/// [`edit`] module; this struct is just the frame-to-frame state.
#[derive(Default)]
struct EditGrid {
    /// Base table of the current result, or `None` for a read-only result.
    source: Option<TableInfo>,
    /// `describe_table` schema for `source`, once it arrives. `None` while
    /// the describe is in flight or if it failed (either way, not
    /// editable until a schema with a primary key lands).
    schema: Option<TableSchema>,
    /// Dialect captured from the live adapter id at browse time.
    dialect: Option<SqlDialect>,
    /// The open inline editor, if any.
    active: Option<CellEditor>,
    /// Staged (仮登録) edits keyed by `(row, col)` grid position.
    staged: BTreeMap<(usize, usize), edit::StagedValue>,
    /// In-flight save queue; `Some` while `UPDATE`s are running.
    save: Option<SaveQueue>,
    /// Last save failure, shown under the Save button (ADR-0039 style).
    error: Option<errors::DisplayError>,
}

impl EditGrid {
    /// Reset to a fresh, read-only state. Called before every manual run;
    /// [`DbboardApp::run_table_browse`] re-establishes provenance after.
    fn reset(&mut self) {
        *self = EditGrid::default();
    }
}

/// Backup (logical dump) UI state (ADR-0049 slice e). A single slot: at
/// most one backup is planned or running at a time, so the toolbar button
/// gates on this being idle/terminal.
///
/// The state machine is deliberately split so its transitions are pure and
/// testable — `drain_replies` only *moves between* states, never touching
/// egui or the native file dialog. The two UI-only steps live in the render
/// path: [`BackupState::Confirming`] draws the warn modal, and
/// [`BackupState::ReadyToSave`] triggers the (blocking) save dialog, exactly
/// as the CSV-export button does.
#[derive(Debug, Default)]
enum BackupState {
    /// No backup activity.
    #[default]
    Idle,
    /// `PlanBackup` sent; awaiting [`Reply::BackupPlanned`].
    Planning,
    /// Preflight came back over the warn threshold; the render path shows
    /// the "large database" modal before proceeding. Carries the plan and
    /// the warned total so the modal can name the row count.
    Confirming { plan: DumpPlan, total_rows: u64 },
    /// Preflight accepted (either under threshold, or confirmed through the
    /// modal); the render path opens the save dialog on the next frame.
    ReadyToSave(DumpPlan),
    /// Dump running; carries the latest progress snapshot for the bar.
    Running(DumpProgress),
    /// Terminal: the dump finished (possibly cancelled, or with per-table
    /// failures / truncations recorded in the outcome).
    Done(DumpOutcome),
    /// Terminal: the output file could not be opened or written.
    Failed(String),
}

/// The outcome of one frame of the huge-DB confirm modal, captured inside
/// the render closure and applied after it (egui borrows `ui` for the
/// closure, so the state transition has to wait until it returns).
enum ConfirmAction {
    None,
    Continue,
    Cancel,
}

/// Restore (logical import) state machine (ADR-0051, slice 6). Mirrors
/// [`BackupState`], but its first step is the file *picker* rather than a
/// preflight query: the toolbar Restore button opens the open-file dialog, and
/// only a chosen `.sql` path advances to [`RestoreState::Planning`].
///
/// The transitions are pure and testable — `drain_replies` only *moves
/// between* states. The one UI-only step is the [`RestoreState::Confirming`]
/// modal, the ADR-0051 strong-confirm shown when the target is not empty; an
/// empty target skips it and runs straight from the preflight reply.
#[derive(Debug, Default)]
enum RestoreState {
    /// No restore activity.
    #[default]
    Idle,
    /// A file was picked and `PlanRestore` sent; awaiting
    /// [`Reply::RestorePlanned`].
    Planning,
    /// Preflight came back against a non-empty target; the render path shows
    /// the strong-confirm modal before proceeding (ADR-0051 empty/new-target
    /// safety model). Carries the plan so a "Restore anyway" advances it.
    Confirming { plan: RestorePlan },
    /// Restore running; carries the latest progress snapshot for the bar.
    Running(RestoreProgress),
    /// Terminal: the restore finished (possibly cancelled, or with
    /// per-statement failures recorded in the outcome).
    Done(RestoreOutcome),
    /// Terminal: a fatal error prevented (or unwound) the restore.
    Failed(String),
}

pub struct DbboardApp {
    sql: String,
    tables: DbResult<Vec<TableInfo>>,
    last_result: Option<DbResult<QueryResult>>,
    /// Which result-grid rows are selected (ADR-0035 slice 2). Reset
    /// whenever a new result replaces [`Self::last_result`] — the old
    /// indices no longer point at the same rows.
    result_selection: selection::ResultSelection,
    /// Result-grid column sort (up to three levels). Reset whenever a new
    /// result replaces [`Self::last_result`] — the columns may differ.
    result_sort: SortState,
    history: PersistentHistoryStore,
    /// `Some` between submitting a query and consuming its reply; the
    /// `drain_replies` path uses this to compute `duration_ms`.
    pending: Option<PendingSubmit>,
    /// `Some` between submitting an AI request and its terminal reply
    /// (ADR-0027 slice c). Consumed by the `drain_replies` AI arms to
    /// build the on-disk [`AiEntry`].
    pending_ai: Option<PendingAiSubmit>,
    /// Connection id stamped on every completion record (ADR-0017
    /// `conn` field). Updated on every successful `ConnectionSwitched`
    /// reply (ADR-0020) so subsequent history records carry the new id.
    /// Empty string for tests / in-memory-only flows.
    conn_label: String,
    /// Last `SwitchConnection` failure surfaced to the UI (id + error).
    /// Cleared on the next successful switch. Independent of the main
    /// query-result error path so a failed switch does not overwrite
    /// the result panel.
    last_switch_error: Option<(String, DbError)>,
    /// Wall-clock RFC 3339 source, injected.
    now_rfc3339: RfcClock,
    /// Shared, atomically-swappable AI provider slot (ADR-0023 +
    /// ADR-0025). Starts populated when the binary's precedence chain
    /// (`env > ai-providers.toml > None`) resolves a provider at
    /// startup. The `AiProviderSwitcher` injected by the binary may
    /// replace the inner `Option` at any time; the worker reads a fresh
    /// snapshot per dispatch and [`Self::has_ai_provider`] reads the
    /// slot directly, so the AI panel reveals itself the moment a swap
    /// fills a previously-empty slot.
    ai_provider_slot: AiProviderSlot,
    /// AI panel local state (slice (b) of issue 0005). Always present;
    /// the panel is only *rendered* when [`Self::has_ai_provider`]
    /// returns true, so the field carries no runtime cost on the AI-less
    /// path.
    ai_panel: AiPanel,
    /// Display name of the AI provider currently bound to
    /// [`Self::ai_provider_slot`] (ADR-0025 slice (b)). Set by the
    /// binary through [`Self::set_active_ai_provider_label`] — the
    /// resolution lives there because the `AiSettingsAdmin` that holds
    /// the id↔name mapping is binary-owned. The panel reads this each
    /// frame to render an "Active: <name>" subtitle.
    active_ai_provider_label: Option<String>,
    /// Last `SwitchAiProvider` failure surfaced through
    /// [`Reply::AiProviderSwitchFailed`]. Cleared on the next successful
    /// switch. Kept distinct from [`Self::last_switch_error`] so the
    /// connection-side and AI-side errors do not overwrite each other.
    last_ai_switch_error: Option<String>,
    /// Live-adapter view injected by the binary (ADR-0028). Read each
    /// frame by [`Self::db_has_describe_table`] to gate the AI panel's
    /// "Include column details" toggle; the same handle drives the
    /// worker's `PrefetchSchema` fan-out. `None` (tests / in-memory
    /// flows) simply hides the toggle.
    schema_source: Option<Arc<dyn SchemaSource>>,
    /// Safety net for bare `SELECT`s (ADR-0030). When on, running a plain
    /// `SELECT` with no `LIMIT` appends `LIMIT {DEFAULT_AUTO_LIMIT}` to the
    /// executed statement so an unbounded scan can't freeze the UI. Visible
    /// as a toolbar checkbox and overridable: the user can uncheck it or
    /// write their own `LIMIT`, in which case the guard backs off.
    auto_limit: bool,
    /// Which lower-panel tab is active (ADR-0031).
    active_tab: ResultTab,
    /// Structure-tab state, `Some` once a table has been clicked.
    structure: Option<StructureView>,
    /// Local table/column notes (ADR-0045). Injected by the binary via
    /// [`Self::with_annotations`]; `None` in tests / in-memory flows,
    /// where the Structure tab's Note column renders read-only and empty.
    annotations: Option<AnnotationsAdmin>,
    /// Inline cell-editing state (issue 0013 slice b). Populated only for
    /// browse-`SELECT` results of a single base table; read-only otherwise.
    edit: EditGrid,
    /// Backup (logical dump) state machine (ADR-0049 slice e). `Idle`
    /// until the toolbar Backup button fires a preflight.
    backup: BackupState,
    /// Large-database warn threshold in total rows (ADR-0050). Seeded to
    /// [`DEFAULT_BACKUP_WARN_ROWS`] and overridden by the persisted
    /// `ui-settings.toml` value the binary pushes in via
    /// [`Self::set_backup_warn_rows`]. The preflight compares the plan's
    /// total against this, not the constant.
    backup_warn_rows: u64,
    /// Restore (logical import) state machine (ADR-0051 slice 6). `Idle` until
    /// the toolbar Restore button picks a `.sql` file and fires a preflight.
    restore: RestoreState,
    busy: bool,
    cmd_tx: Sender<Command>,
    reply_rx: Receiver<Reply>,
}

impl DbboardApp {
    /// Connect the UI to a running loopback server at `base_url`
    /// (e.g. `http://127.0.0.1:54123`).
    ///
    /// Creates the command/reply channels, spawns the HTTP [`worker`]
    /// thread bound to that URL, and returns an app primed with the
    /// bootstrap `ListTables` request. `egui_ctx` lets the worker wake
    /// the UI thread when a reply lands.
    ///
    /// `history` is the persistent query-history store (ADR-0017). For
    /// in-memory-only flows pass [`PersistentHistoryStore::in_memory_only`].
    /// `conn_label` is the connection id stamped on every completion
    /// record. `now_rfc3339` is the wall-clock RFC 3339 timestamp source;
    /// the desktop binary supplies a real implementation, tests pass a
    /// fixed-string stub.
    ///
    /// `switcher` is the in-process bridge the worker calls when a
    /// `SwitchConnection` command arrives (ADR-0020). The desktop
    /// binary supplies an implementation that owns the live
    /// [`AppState`](dbboard_server::AppState), the connection store,
    /// and a runtime handle.
    ///
    /// `ai_switcher` is the in-process bridge the worker calls when a
    /// `SwitchAiProvider` command arrives (ADR-0025). The desktop
    /// binary supplies a `DesktopAiSwitcher` that owns the
    /// `AiSettingsAdmin` and writes the same [`AiProviderSlot`] the
    /// worker reads from; tests pass a stub.
    ///
    /// `ai_provider_slot` is the shared, atomically-swappable AI
    /// provider slot (ADR-0023 + ADR-0025). The binary's precedence
    /// chain seeds the slot (`Some` when env or TOML resolves a
    /// provider, `None` otherwise) and the Stage 2 AI panel registers
    /// itself only when [`Self::has_ai_provider`] returns true, which
    /// reads the slot directly so it tracks live swaps.
    ///
    /// `schema_source` is the live-adapter view for ADR-0028's
    /// `PrefetchSchema` fan-out and the per-frame capability gate on
    /// the "Include column details" toggle. `None` (tests / flows
    /// without a local server) hides the toggle and makes any stray
    /// `PrefetchSchema` command degrade into an all-errors reply.
    // Arg count grows by one with each in-process switcher we wire
    // through the worker (ADR-0020 ConnectionSwitcher, ADR-0025
    // AiProviderSwitcher, ADR-0028 SchemaSource). A struct-builder
    // refactor is queued for the next slice that adds a handle;
    // until then, allowing here keeps the slice focused.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn connect(
        base_url: String,
        egui_ctx: egui::Context,
        history: PersistentHistoryStore,
        conn_label: String,
        now_rfc3339: RfcClock,
        switcher: Arc<dyn ConnectionSwitcher>,
        ai_switcher: Arc<dyn AiProviderSwitcher>,
        ai_provider_slot: AiProviderSlot,
        schema_source: Option<Arc<dyn SchemaSource>>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (reply_tx, reply_rx) = mpsc::channel::<Reply>();
        worker::spawn_worker(
            base_url,
            cmd_rx,
            reply_tx,
            egui_ctx,
            switcher,
            ai_switcher,
            Arc::clone(&ai_provider_slot),
            schema_source.clone(),
        );
        Self::new(
            cmd_tx,
            reply_rx,
            history,
            conn_label,
            now_rfc3339,
            ai_provider_slot,
            schema_source,
        )
    }

    /// Build a fresh app and immediately request the table list so
    /// the sidebar is populated by the time the first frame renders.
    ///
    /// # Panics
    ///
    /// Sending the bootstrap `ListTables` command does not panic in
    /// practice — it is only ignored if the worker has already shut
    /// down, which the binary keeps alive for the process lifetime.
    #[must_use]
    pub fn new(
        cmd_tx: Sender<Command>,
        reply_rx: Receiver<Reply>,
        history: PersistentHistoryStore,
        conn_label: String,
        now_rfc3339: RfcClock,
        ai_provider_slot: AiProviderSlot,
        schema_source: Option<Arc<dyn SchemaSource>>,
    ) -> Self {
        let _ = cmd_tx.send(Command::ListTables);
        Self {
            sql: String::new(),
            tables: Ok(Vec::new()),
            last_result: None,
            result_selection: selection::ResultSelection::default(),
            result_sort: SortState::default(),
            history,
            pending: None,
            pending_ai: None,
            conn_label,
            last_switch_error: None,
            now_rfc3339,
            ai_provider_slot,
            ai_panel: AiPanel::new(),
            active_ai_provider_label: None,
            last_ai_switch_error: None,
            schema_source,
            auto_limit: true,
            active_tab: ResultTab::Results,
            structure: None,
            annotations: None,
            edit: EditGrid::default(),
            backup: BackupState::Idle,
            backup_warn_rows: DEFAULT_BACKUP_WARN_ROWS,
            restore: RestoreState::Idle,
            busy: false,
            cmd_tx,
            reply_rx,
        }
    }

    /// Open the structure tab for `table` and kick off its describe
    /// (ADR-0031). The reply lands on the next `drain_replies` pass and is
    /// matched back to this table so a stale describe is ignored.
    fn open_structure(&mut self, table: TableInfo) {
        self.active_tab = ResultTab::Structure;
        let _ = self.cmd_tx.send(Command::DescribeTable {
            table: table.clone(),
        });
        self.structure = Some(StructureView {
            table,
            schema: None,
            note_buffers: BTreeMap::new(),
            table_note_buffer: None,
        });
    }

    /// Attach the local annotations store so the Structure tab's Note
    /// column becomes editable (ADR-0045). Builder-style so the desktop
    /// binary can chain it onto [`Self::connect`] without growing the
    /// already-long constructor arg list; tests that need notes set the
    /// field directly.
    #[must_use]
    pub fn with_annotations(mut self, annotations: AnnotationsAdmin) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Persist an edited note for the Structure tab's current table
    /// (ADR-0045). No-op when no annotations store is wired or no table
    /// is on screen. Keyed by the live connection id plus the table's
    /// schema-qualified key so the same table name under two connections
    /// keeps independent notes. A disk-write failure is logged and
    /// swallowed — a note is documentation, never worth blocking the UI.
    fn commit_structure_note(&mut self, target: &NoteTarget, text: &str) {
        let Some(view) = &self.structure else {
            return;
        };
        let key = annotation_table_key(view.table.schema.as_deref(), &view.table.name);
        let conn = self.conn_label.clone();
        let Some(admin) = self.annotations.as_mut() else {
            return;
        };
        let res = match target {
            NoteTarget::Table => admin.set_table_note(&conn, &key, text),
            NoteTarget::Column(column) => admin.set_column_note(&conn, &key, column, text),
        };
        if let Err(e) = res {
            eprintln!("dbboard: annotation save failed: {e}");
        }
    }

    // Central reply dispatcher: one arm per `Reply` variant, mirroring the
    // command side in `worker::handle_command`. It grows one arm per feature
    // (ADR-0051 restore added four), so the line lint is not a useful signal
    // here — the same allow guards the sibling dispatcher.
    #[allow(clippy::too_many_lines)]
    fn drain_replies(&mut self) {
        while let Ok(reply) = self.reply_rx.try_recv() {
            match reply {
                Reply::Tables(r) => self.tables = r,
                Reply::QueryResult(r) => {
                    // Inline-edit save steps (issue 0013 slice b) reuse the
                    // query path; intercept their replies before the normal
                    // result/history handling so they never replace the grid
                    // or land in query history.
                    if self.advance_save(&r) {
                        continue;
                    }
                    if let Some(pending) = self.pending.take() {
                        // Best-effort completion record. A disk write
                        // failure must not block the UI's view of the
                        // result, so we log to stderr and otherwise
                        // swallow (ADR-0017 §6).
                        let entry = build_completion_entry(
                            &r,
                            &pending,
                            &self.conn_label,
                            (self.now_rfc3339)(),
                        );
                        if let Err(e) = self.history.record_completion(&entry) {
                            eprintln!("dbboard: history append failed: {e}");
                        }
                    }
                    self.last_result = Some(r);
                    // A fresh result invalidates any row selection carried
                    // over from the previous one (ADR-0035 slice 2), and any
                    // sort keyed on the previous result's columns.
                    self.result_selection.clear();
                    self.result_sort.reset();
                    // The rows just changed underneath any open inline
                    // editor; close it (issue 0013 slice b). Staged edits
                    // persist — a fresh run already reset them via run_sql.
                    self.edit.active = None;
                    self.busy = false;
                }
                // ADR-0020: swap completed. Treat the new id as the
                // active connection from now on — subsequent history
                // records stamp it, and the active-row marker tracks
                // it. Also refresh the sidebar since the new adapter
                // may have a different schema.
                Reply::ConnectionSwitched { id } => {
                    self.conn_label = id;
                    self.last_switch_error = None;
                    let _ = self.cmd_tx.send(Command::ListTables);
                }
                // The previous adapter is still live; just surface the
                // error and leave `conn_label` untouched.
                Reply::SwitchFailed { id, error } => {
                    self.last_switch_error = Some((id, error));
                }
                // ADR-0023: AI round-trip reply. Routed into the panel's
                // state machine by the arm helper — both success and
                // failure clear `busy` and replace any stale content
                // (ai::tests cover the ordering invariants).
                //
                // ADR-0027 slice c: the helper also consumes the
                // submit-time snapshot in [`Self::pending_ai`] and
                // appends an AI history record using the spawn-time
                // identity the worker stamped on the reply (ADR-0027
                // slice b).
                Reply::AiResponded {
                    text,
                    tokens_in,
                    tokens_out,
                    provider,
                    model,
                } => self.on_ai_responded(text, tokens_in, tokens_out, (provider, model)),
                Reply::AiFailed {
                    error,
                    provider,
                    model,
                } => self.on_ai_failed(&error, (provider, model)),
                // ADR-0025 slice (b): AI provider swap outcomes. On
                // success we just clear any prior error — the resolved
                // *name* lands separately via
                // [`set_active_ai_provider_label`], which the binary
                // pushes from its `AiSettingsAdmin` snapshot each frame
                // because that admin is the single id↔name map.
                Reply::AiProviderSwitched { .. } => {
                    self.last_ai_switch_error = None;
                }
                Reply::AiProviderSwitchFailed { reason } => {
                    self.last_ai_switch_error = Some(reason);
                }
                // ADR-0026 Decision 6: streaming chunks. Slice (c)
                // wires the worker channel; slice (d) extends
                // [`AiPanel`] with the accumulator + token meter. For
                // now the panel does not emit `*Stream` commands so
                // these arms only fire under tests that drive the
                // worker directly.
                Reply::AiChunk {
                    text_delta,
                    tokens_in,
                    tokens_out,
                } => {
                    self.ai_panel
                        .on_stream_chunk(&text_delta, tokens_in, tokens_out);
                }
                Reply::AiStreamComplete {
                    tokens_in,
                    tokens_out,
                    stop_reason,
                    provider,
                    model,
                } => self.on_ai_stream_complete(
                    tokens_in,
                    tokens_out,
                    &stop_reason,
                    (provider, model),
                ),
                // ADR-0026 Decision 12: a user-initiated cancel resets
                // the panel without surfacing an error banner. Lives in
                // its own arm because `AiError::Cancelled` is never the
                // payload of `Reply::AiFailed` (the cancel arm of the
                // `select!` short-circuits before `AiResult::Err` ever
                // forms).
                Reply::AiCancelled { provider, model } => {
                    self.on_ai_cancelled((provider, model));
                }
                // ADR-0028 Decision 9: the describe_table fan-out came
                // back. The panel converts it into the deferred Suggest
                // command (carrying `full_schema`), which we forward
                // exactly like a Send click — including the pending-ai
                // snapshot and the channel-closed fallback.
                Reply::SchemaPrefetched { schemas, errors } => {
                    if let Some(cmd) = self.ai_panel.on_schema_prefetched(schemas, &errors) {
                        self.send_ai_command(cmd);
                    }
                }
                // ADR-0031: structure-tab describe came back. Apply it only
                // if it still matches the table on screen — the user may
                // have clicked another table while this one was in flight.
                Reply::TableDescribed { table, result } => {
                    // Feed the inline editor's primary-key lookup when this
                    // describe is for the current browse result (issue 0013
                    // slice b). A describe error just leaves it non-editable.
                    if self.edit.source.as_ref() == Some(&table) {
                        self.edit.schema = match &result {
                            Ok(schema) => Some(schema.clone()),
                            Err(_) => None,
                        };
                    }
                    if let Some(view) = self.structure.as_mut() {
                        if view.table == table {
                            view.schema = Some(result);
                        }
                    }
                }
                // ADR-0049 slice e: preflight came back. A late reply after
                // the user already dismissed the flow (state back to Idle)
                // is ignored, so a stale plan cannot resurrect a modal.
                Reply::BackupPlanned { result } => self.on_backup_planned(result),
                Reply::BackupProgress { progress } => self.on_backup_progress(progress),
                Reply::BackupComplete { outcome } => self.backup = BackupState::Done(outcome),
                Reply::BackupFailed { message } => self.backup = BackupState::Failed(message),
                Reply::RestorePlanned { result } => self.on_restore_planned(result),
                Reply::RestoreProgress { progress } => self.on_restore_progress(progress),
                Reply::RestoreComplete { outcome } => self.restore = RestoreState::Done(outcome),
                Reply::RestoreFailed { message } => self.restore = RestoreState::Failed(message),
            }
        }
    }

    /// Forward an AI command to the worker, snapshotting the submit-time
    /// context first (ADR-0027 slice c). Shared by the Send-click path
    /// in [`Self::render_ai_panel`] and the deferred-Suggest path in
    /// `drain_replies`' `SchemaPrefetched` arm so both get the same
    /// channel-closed fallback.
    fn send_ai_command(&mut self, cmd: Command) {
        if let Some(pending) = pending_ai_from_command(&cmd, &self.conn_label) {
            self.pending_ai = Some(pending);
        }
        if self.cmd_tx.send(cmd).is_err() {
            // Worker hung up — surface a synthetic failure so the
            // panel exits the busy state immediately rather than
            // waiting forever for a reply that will never arrive.
            // Drop the just-set pending: without a worker there is
            // no terminal reply that would consume it.
            self.pending_ai = None;
            self.ai_panel
                .on_error(&AiError::Network("ai worker channel closed".into()));
        }
    }

    /// Best-effort AI history append. `record_ai` failures on disk are
    /// logged to stderr (ADR-0017 §6) — never propagated, since the
    /// panel state machine still needs the reply.
    fn record_ai_history(&mut self, entry: AiEntry) {
        if let Err(e) = self.history.record_ai(entry) {
            eprintln!("dbboard: ai history append failed: {e}");
        }
    }

    fn on_ai_responded(
        &mut self,
        text: String,
        tokens_in: u32,
        tokens_out: u32,
        identity: (String, String),
    ) {
        if let Some(pending) = self.pending_ai.take() {
            let entry = build_ai_ok_entry(
                pending,
                text.clone(),
                tokens_in,
                tokens_out,
                identity,
                None,
                (self.now_rfc3339)(),
            );
            self.record_ai_history(entry);
        }
        self.ai_panel.on_response(text, tokens_in, tokens_out);
    }

    fn on_ai_failed(&mut self, error: &AiError, identity: (String, String)) {
        if let Some(pending) = self.pending_ai.take() {
            let entry = build_ai_failed_entry(pending, error, identity, (self.now_rfc3339)());
            self.record_ai_history(entry);
        }
        self.ai_panel.on_error(error);
    }

    fn on_ai_stream_complete(
        &mut self,
        tokens_in: u32,
        tokens_out: u32,
        stop_reason: &StopReason,
        identity: (String, String),
    ) {
        // Peek the accumulator BEFORE the panel drains it — ADR-0027
        // slice c wants the full streamed body in the history record,
        // and `on_stream_complete` consumes it.
        let response = self
            .ai_panel
            .streaming()
            .map(|acc| acc.text.clone())
            .unwrap_or_default();
        if let Some(pending) = self.pending_ai.take() {
            let entry = build_ai_ok_entry(
                pending,
                response,
                tokens_in,
                tokens_out,
                identity,
                Some(stop_reason_wire(stop_reason)),
                (self.now_rfc3339)(),
            );
            self.record_ai_history(entry);
        }
        self.ai_panel
            .on_stream_complete(tokens_in, tokens_out, stop_reason);
    }

    /// Draw the AI panel and forward any Send-click command to the
    /// worker, snapshotting the submit-time context (ADR-0027 slice c).
    /// A `Cancel` command MUST NOT overwrite `pending_ai` — the pending
    /// record belongs to the request the cancel is targeting, and its
    /// terminal reply will consume it.
    fn render_ai_panel(&mut self, ctx: &egui::Context) {
        // `dialect` is the active adapter id (e.g. "postgres", "neon").
        // The UI does not currently reach the loopback server's adapter
        // id — bridging that requires either a `Command::GetCapabilities`
        // round-trip or a dedicated binary-side accessor. Slice (b)
        // ships without the hint; Stage 2 wires it once the adapter-id
        // surface is decided.
        let dialect: Option<&str> = None;
        // Borrow the cached tables rather than cloning them every frame;
        // the panel only allocates a Vec when Send is clicked and the
        // Suggest arm fires.
        let schema_slice: &[TableInfo] = self.tables.as_ref().map_or(&[], Vec::as_slice);
        let active_label = self.active_ai_provider_label.as_deref();
        // ADR-0026 Decision 6: pick streaming over atomic at Send time
        // iff the active provider declares the capability. The slot
        // read happens here, not in the panel, so the panel stays a
        // pure presentation layer over its passed-in flags.
        let has_streaming = self.ai_has_streaming();
        // ADR-0028: per-frame capability read, mirroring has_streaming —
        // a connection switch flips the toggle's visibility on the next
        // render tick without extra plumbing.
        let has_describe_table = self.db_has_describe_table();
        if let Some(cmd) = self.ai_panel.ui(
            ctx,
            dialect,
            schema_slice,
            active_label,
            has_streaming,
            has_describe_table,
        ) {
            self.send_ai_command(cmd);
        }
        // Drive a follow-up frame while the AI request is in flight so
        // the reply drains promptly without a user gesture.
        if self.ai_panel.is_busy() {
            ctx.request_repaint();
        }
    }

    fn on_ai_cancelled(&mut self, identity: (String, String)) {
        // Peek the accumulator before `on_cancelled` moves it into
        // `last_response`. ADR-0027 §Decision 5: cancelled records
        // preserve any partial content the user has already paid tokens
        // for, and use `Some(tokens)` only when a usage event actually
        // arrived (typed as "atomic cancel or cancel before first chunk
        // => None").
        let partial = self.ai_panel.streaming().cloned();
        if let Some(pending) = self.pending_ai.take() {
            let entry = build_ai_cancelled_entry(pending, partial, identity, (self.now_rfc3339)());
            self.record_ai_history(entry);
        }
        self.ai_panel.on_cancelled();
    }

    fn run_sql(&mut self) {
        if self.busy || self.sql.trim().is_empty() {
            return;
        }
        // A fresh manual run drops any inline-edit provenance (issue 0013
        // slice b): an arbitrary query, a COUNT(*), or a re-typed SELECT is
        // read-only until a browse-SELECT re-establishes an editable table.
        // `run_table_browse` re-sets provenance right after calling this.
        self.edit.reset();
        // Bring the result forward: a query run while the user is on the
        // Structure tab (ADR-0031) would otherwise leave its output hidden
        // behind the table inspector. Switch at submit time so the busy
        // spinner shows on the Results tab the output will land on.
        self.active_tab = ResultTab::Results;
        // Apply the bare-SELECT guard once, up front, so history, the
        // pending record, and the executed statement all agree on exactly
        // what ran (ADR-0030). A no-op unless auto_limit is on and the
        // statement is a plain unbounded SELECT.
        let effective = apply_auto_limit(&self.sql, self.auto_limit, DEFAULT_AUTO_LIMIT);
        // Submit-time: push the executed SQL into the in-memory ring so the
        // history panel updates instantly; disk append happens at reply
        // time, once we know duration / rows / status (ADR-0017).
        self.history.record_submit(effective.clone());
        self.pending = Some(PendingSubmit {
            started: Instant::now(),
            sql: effective.clone(),
        });
        self.busy = true;
        let _ = self.cmd_tx.send(Command::Query(effective));
        // Tables may have changed as a side effect (CREATE/DROP), so
        // refresh the sidebar after every run.
        let _ = self.cmd_tx.send(Command::ListTables);
    }

    /// Drop a table right-click starter query into the editor and run it in
    /// one action (issue 0012). Split out from the panel so the "pick ⇒
    /// run" behaviour is unit-testable without egui. Starters are read-only
    /// (`SELECT` / `COUNT(*)`), so auto-running one is safe by construction;
    /// the text stays in the editor so the user can tweak and re-run.
    /// Ignored while a query is in flight, mirroring the disabled Run
    /// button — this avoids swapping the editor text out from under an
    /// in-flight run.
    fn run_quick_sql(&mut self, sql: String) {
        if self.busy {
            return;
        }
        self.sql = sql;
        self.run_sql();
    }

    /// Run a table's browse `SELECT *` and mark the result editable (issue
    /// 0013 slice b). Unlike [`Self::run_quick_sql`], this remembers the
    /// source [`TableInfo`] and kicks off a `describe_table` so the inline
    /// editor can resolve a primary key. Read-only until the schema lands;
    /// non-primary-key or view results simply never become editable.
    fn run_table_browse(&mut self, table: TableInfo) {
        if self.busy {
            return;
        }
        // `run_sql` resets edit state, so establish provenance afterwards.
        self.run_quick_sql(quick_select_sql(&table));
        self.edit.source = Some(table.clone());
        self.edit.dialect = self.current_dialect();
        let _ = self.cmd_tx.send(Command::DescribeTable { table });
    }

    /// SQL dialect of the live adapter, or `None` when no adapter is wired
    /// or its id is unknown (issue 0013 slice b). Read off the injected
    /// [`SchemaSource`] so a connection switch is reflected immediately,
    /// mirroring [`Self::db_has_describe_table`].
    fn current_dialect(&self) -> Option<SqlDialect> {
        self.schema_source
            .as_ref()
            .and_then(|s| edit::dialect_for_adapter_id(s.current_adapter().id()))
    }

    /// Turn the staged inline edits into keyed `UPDATE`s and start running
    /// them one at a time (issue 0013 slice b). A planning error (e.g. a
    /// missing primary-key column) is surfaced under the Save button and
    /// leaves every edit staged; nothing runs. No-op when nothing is
    /// staged or the result is not editable.
    fn begin_save(&mut self) {
        let (Some(table), Some(schema), Some(dialect)) = (
            self.edit.source.clone(),
            self.edit.schema.clone(),
            self.edit.dialect,
        ) else {
            return;
        };
        let Some(Ok(result)) = self.last_result.as_ref() else {
            return;
        };
        let plans =
            match edit::build_update_plans(&table, &schema, dialect, result, &self.edit.staged) {
                Ok(plans) => plans,
                Err(e) => {
                    self.edit.error = Some(errors::DisplayError::plain(e.to_string()));
                    return;
                }
            };
        let mut queue: VecDeque<_> = plans.into();
        let Some(current) = queue.pop_front() else {
            return; // nothing staged
        };
        self.edit.error = None;
        self.edit.active = None;
        self.busy = true;
        let sql = current.sql.clone();
        self.edit.save = Some(SaveQueue {
            current,
            remaining: queue,
        });
        let _ = self.cmd_tx.send(Command::Query(sql));
    }

    /// Handle a `Reply::QueryResult` that belongs to an in-flight inline
    /// save (issue 0013 slice b). Returns `true` when the reply was
    /// consumed as a save step so the caller skips the normal
    /// result-handling path. A confirmed step (`rows_affected == 1`) clears
    /// its staged cells and sends the next; any other count or an error
    /// stops the run, surfaces a message, and leaves the rest staged.
    fn advance_save(&mut self, reply: &DbResult<QueryResult>) -> bool {
        let Some(mut queue) = self.edit.save.take() else {
            return false;
        };
        match reply {
            Ok(qr) if qr.rows_affected == 1 => {
                // Commit: the cells this UPDATE wrote are no longer dirty.
                for &col in &queue.current.columns {
                    self.edit.staged.remove(&(queue.current.row, col));
                }
                if let Some(next) = queue.remaining.pop_front() {
                    let sql = next.sql.clone();
                    queue.current = next;
                    self.edit.save = Some(queue);
                    let _ = self.cmd_tx.send(Command::Query(sql));
                } else {
                    // All rows committed. Re-read the table so the grid
                    // shows engine-normalised values (a typed/triggered
                    // column may differ from the text we sent).
                    self.busy = false;
                    self.refresh_after_save();
                }
            }
            // 0 rows = the keyed row vanished (concurrent delete); >1 =
            // the "primary key" was not unique. Either way, stop and keep
            // the remaining edits staged for the user to retry.
            Ok(qr) => {
                self.edit.error = Some(errors::DisplayError::plain(t_args!(
                    "edit-save-unexpected-rows",
                    rows = qr.rows_affected
                )));
                self.busy = false;
            }
            Err(e) => {
                self.edit.error = Some(errors::db_error_display(e));
                self.busy = false;
            }
        }
        true
    }

    /// Re-run the browse `SELECT` after a fully-committed save so the grid
    /// reflects what actually landed, keeping the table editable.
    fn refresh_after_save(&mut self) {
        if let Some(table) = self.edit.source.clone() {
            self.run_table_browse(table);
        }
    }

    /// Drop all staged inline edits and any open editor (Discard button).
    fn discard_edits(&mut self) {
        self.edit.staged.clear();
        self.edit.active = None;
        self.edit.error = None;
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    /// Request a switch to the named connection (ADR-0020). The actual
    /// adapter rebuild + swap happens in the worker thread via the
    /// [`worker::ConnectionSwitcher`] the binary injected at startup;
    /// the UI just signals intent here. A `ConnectionSwitched` /
    /// `SwitchFailed` reply lands on the next `drain_replies` pass.
    ///
    /// Clears any prior [`Self::last_switch_error`] up front: a new
    /// attempt supersedes the old failure, and the Connections window's
    /// auto-close poll would otherwise read the stale error as an
    /// immediate failure of the in-flight switch.
    pub fn switch_connection(&mut self, id: String) {
        self.last_switch_error = None;
        let _ = self.cmd_tx.send(Command::SwitchConnection { id });
    }

    /// Id of the connection currently active in the running server
    /// (ADR-0020). Returns the empty string for the early bootstrap
    /// window when no startup label was supplied (tests / in-memory).
    #[must_use]
    pub fn active_connection_id(&self) -> &str {
        &self.conn_label
    }

    /// Last `SwitchConnection` failure, if any. Cleared on the next
    /// successful switch. The id is the target the user asked for; the
    /// `DbError` is the wire error. Surfaced read-only so the UI can
    /// render "could not connect to <id>".
    #[must_use]
    pub fn last_switch_error(&self) -> Option<(&str, &DbError)> {
        self.last_switch_error
            .as_ref()
            .map(|(id, err)| (id.as_str(), err))
    }

    /// Localized, display-ready message for the last connection-switch
    /// failure, or `None` when the last switch succeeded / none was
    /// attempted. Threaded into the Connections window so a failed
    /// "Connect" click is visible instead of silently swallowed — before
    /// this was wired the only signal a switch failed was that nothing
    /// happened (ADR-0020). Matches the `ai.rs` error-prefix house style:
    /// a localized prefix followed by the target id and the wire error.
    #[must_use]
    pub fn switch_error_message(&self) -> Option<String> {
        self.last_switch_error
            .as_ref()
            .map(|(id, err)| format!("{}: {id}: {err}", t!("connections-switch-error")))
    }

    /// Read-only view of the recently-run SQL statements (ADR-0014 /
    /// ADR-0017). The returned `HistoryStore` is the in-memory ring
    /// the UI panel reads from; the persistent backing file is owned
    /// internally and not exposed.
    #[must_use]
    pub fn history(&self) -> &HistoryStore {
        self.history.store()
    }

    /// True when the shared AI provider slot is currently populated
    /// (ADR-0023 + ADR-0025). The AI panel registers itself only when
    /// this returns `true`. The slot starts populated when the binary's
    /// precedence chain resolves a provider at startup (env var or
    /// ai-providers.toml active id); graceful degradation = absence, so
    /// neither configured simply hides the panel rather than aborting
    /// startup. A successful `SwitchAiProvider` performed at runtime
    /// fills a previously-empty slot, revealing the panel on the next
    /// render tick.
    #[must_use]
    pub fn has_ai_provider(&self) -> bool {
        self.ai_provider_slot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// `true` when the currently-bound AI provider advertises
    /// [`AiCapabilities::has_streaming`](dbboard_ai::AiCapabilities::has_streaming)
    /// (ADR-0026 Decision 6). The AI panel reads this on Send to pick
    /// between the streaming and atomic command variants. Returns
    /// `false` when no provider is wired — the panel is hidden in that
    /// case so the value is moot, but the function stays total to keep
    /// the `ui()` call site branch-free.
    #[must_use]
    pub fn ai_has_streaming(&self) -> bool {
        self.ai_provider_slot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|p| p.capabilities().has_streaming)
    }

    /// `true` when the live adapter advertises
    /// [`has_describe_table`](dbboard_core::AdapterCapabilities::has_describe_table)
    /// (ADR-0028). Gates the AI panel's "Include column details"
    /// toggle. Read per frame off the injected [`SchemaSource`] —
    /// mirrors [`Self::ai_has_streaming`], so a connection switch is
    /// reflected on the next render tick. `false` when no source is
    /// wired (tests / in-memory flows), which hides the toggle.
    #[must_use]
    pub fn db_has_describe_table(&self) -> bool {
        self.schema_source
            .as_ref()
            .is_some_and(|s| s.current_adapter().capabilities().has_describe_table)
    }

    /// True when the AI panel window is currently open. Always false
    /// when no provider is wired (the menu button that flips this is
    /// suppressed by [`Self::has_ai_provider`]).
    #[must_use]
    pub fn ai_panel_is_open(&self) -> bool {
        self.has_ai_provider() && self.ai_panel.is_open()
    }

    /// Whether the active connection can be backed up (ADR-0049): a live
    /// adapter is wired and its id maps to a SQL dialect the dumper knows.
    /// Read per frame to gate the toolbar button, mirroring
    /// [`Self::db_has_describe_table`]; `false` in tests / in-memory flows,
    /// which simply hides the button.
    #[must_use]
    pub fn can_backup(&self) -> bool {
        self.schema_source
            .as_ref()
            .is_some_and(|s| edit::dialect_for_adapter_id(s.current_adapter().id()).is_some())
    }

    /// Whether a backup is mid-flight (planning or dumping) — used to gate
    /// the toolbar button so a second click cannot start a parallel run. A
    /// terminal state (`Done`/`Failed`) does not count as busy: its summary
    /// window is dismissible and a fresh backup may start behind it.
    #[must_use]
    fn backup_in_progress(&self) -> bool {
        matches!(
            self.backup,
            BackupState::Planning
                | BackupState::Confirming { .. }
                | BackupState::ReadyToSave(_)
                | BackupState::Running(_)
        )
    }

    /// Start a backup by asking the worker to preflight the active
    /// connection (ADR-0049 slice e). No-op if one is already in flight or
    /// the connection cannot be dumped — the toolbar already gates on both,
    /// so this is defence-in-depth. A closed channel leaves the state
    /// untouched (the UI is shutting down anyway).
    fn start_backup(&mut self) {
        if self.backup_in_progress() || !self.can_backup() {
            return;
        }
        if self.cmd_tx.send(Command::PlanBackup).is_ok() {
            self.backup = BackupState::Planning;
        }
    }

    /// Fold a preflight reply into the state machine. A late reply that
    /// arrives after the user dismissed the flow (state no longer
    /// `Planning`) is ignored so a stale plan cannot reopen a modal.
    fn on_backup_planned(&mut self, result: DbResult<DumpPlan>) {
        if !matches!(self.backup, BackupState::Planning) {
            return;
        }
        self.backup = match result {
            Ok(plan) => match plan.exceeds_threshold(self.backup_warn_rows) {
                Some(total_rows) => BackupState::Confirming { plan, total_rows },
                None => BackupState::ReadyToSave(plan),
            },
            Err(e) => BackupState::Failed(e.message().to_string()),
        };
    }

    /// Apply a progress tick. Ticks only matter while a dump is running;
    /// one arriving after cancellation/completion (state already terminal)
    /// is dropped so it cannot resurrect the progress window.
    fn on_backup_progress(&mut self, progress: DumpProgress) {
        if matches!(self.backup, BackupState::Running(_)) {
            self.backup = BackupState::Running(progress);
        }
    }

    /// Ask the worker to cancel the in-flight dump (ADR-0049 Decision 9).
    /// The task still reports a (cancelled) `BackupComplete`, so the state
    /// stays `Running` until that terminal reply lands.
    fn cancel_backup(&mut self) {
        let _ = self.cmd_tx.send(Command::CancelBackup);
    }

    /// Dismiss a terminal backup summary, returning the slot to idle.
    fn dismiss_backup(&mut self) {
        self.backup = BackupState::Idle;
    }

    /// Render whichever backup window the current state calls for, and
    /// drive the two UI-only transitions. `Idle`/`Planning` own no window —
    /// the toolbar carries their affordance (button / spinner). Matching the
    /// state place with non-binding patterns lets each arm freely re-borrow
    /// `self` for its sub-render.
    fn render_backup(&mut self, ctx: &egui::Context) {
        match self.backup {
            BackupState::Idle | BackupState::Planning => {}
            BackupState::Confirming { .. } => self.render_backup_confirm(ctx),
            // Move the plan out and open the (blocking) native save dialog.
            // Resetting to Idle first means a cancelled dialog lands back at
            // Idle with no extra bookkeeping.
            BackupState::ReadyToSave(_) => {
                if let BackupState::ReadyToSave(plan) =
                    std::mem::replace(&mut self.backup, BackupState::Idle)
                {
                    self.pick_path_and_start(plan);
                }
            }
            BackupState::Running(_) => self.render_backup_running(ctx),
            BackupState::Done(_) => self.render_backup_done(ctx),
            BackupState::Failed(_) => self.render_backup_failed(ctx),
        }
    }

    /// The huge-DB warning (ADR-0049 Decision 8, warn-and-allow). Reads the
    /// total off the `Confirming` state; "Back up anyway" advances to the
    /// save dialog, "Cancel" abandons the flow.
    fn render_backup_confirm(&mut self, ctx: &egui::Context) {
        let BackupState::Confirming { total_rows, .. } = self.backup else {
            return;
        };
        let mut action = ConfirmAction::None;
        egui::Window::new(t!("backup-warn-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t_args!("backup-warn-body", rows = total_rows));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("backup-warn-continue")).clicked() {
                        action = ConfirmAction::Continue;
                    }
                    if ui.button(t!("backup-warn-cancel")).clicked() {
                        action = ConfirmAction::Cancel;
                    }
                });
            });
        match action {
            ConfirmAction::Continue => {
                if let BackupState::Confirming { plan, .. } =
                    std::mem::replace(&mut self.backup, BackupState::Idle)
                {
                    self.backup = BackupState::ReadyToSave(plan);
                }
            }
            ConfirmAction::Cancel => self.backup = BackupState::Idle,
            ConfirmAction::None => {}
        }
    }

    /// The live progress window: a bar plus table/row counters and a Cancel
    /// button. Cancel only signals the worker; the terminal reply flips the
    /// state, so the window stays up (showing the last progress) until then.
    fn render_backup_running(&mut self, ctx: &egui::Context) {
        let BackupState::Running(progress) = &self.backup else {
            return;
        };
        let fraction = backup_fraction(progress);
        let table_line = t_args!(
            "backup-progress-table",
            done = progress.tables_done,
            total = progress.tables_total
        );
        let rows_line = t_args!(
            "backup-progress-rows",
            done = progress.rows_done,
            total = progress.rows_total
        );
        let current = progress
            .current_table
            .as_ref()
            .map(|t| t_args!("backup-progress-current", table = t.clone()));
        let mut cancel = false;
        egui::Window::new(t!("backup-progress-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                ui.label(table_line);
                ui.label(rows_line);
                if let Some(current) = current {
                    ui.label(current);
                }
                ui.add_space(8.0);
                if ui.button(t!("backup-cancel-button")).clicked() {
                    cancel = true;
                }
            });
        // Keep the frame ticking so progress replies keep draining.
        ctx.request_repaint();
        if cancel {
            self.cancel_backup();
        }
    }

    /// Completion summary. Names the dumped totals and surfaces the honest
    /// caveats — cancellation, skipped tables, truncations — so a partial
    /// backup is never mistaken for a clean one. Close returns to idle.
    fn render_backup_done(&mut self, ctx: &egui::Context) {
        let BackupState::Done(outcome) = &self.backup else {
            return;
        };
        let summary = t_args!(
            "backup-done-summary",
            tables = outcome.tables_dumped,
            rows = outcome.rows_written
        );
        let cancelled = outcome.cancelled;
        let failures = outcome.failures.len();
        let truncations = outcome.truncations.len();
        let mut close = false;
        egui::Window::new(t!("backup-done-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(summary);
                if cancelled {
                    ui.label(t!("backup-done-cancelled"));
                }
                if failures > 0 {
                    ui.label(t_args!("backup-done-failures", count = failures));
                }
                if truncations > 0 {
                    ui.label(t_args!("backup-done-truncations", count = truncations));
                }
                ui.add_space(8.0);
                if ui.button(t!("backup-close-button")).clicked() {
                    close = true;
                }
            });
        if close {
            self.dismiss_backup();
        }
    }

    /// Failure window: the output could not be opened or written. Close
    /// returns to idle.
    fn render_backup_failed(&mut self, ctx: &egui::Context) {
        let BackupState::Failed(message) = &self.backup else {
            return;
        };
        // A raw I/O error string has no separate localized/original halves,
        // so both slots carry the same text (only one line renders).
        let display = errors::DisplayError::new(message.clone(), message.clone());
        let mut close = false;
        egui::Window::new(t!("backup-failed-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                errors::render_error(ui, Some(&display));
                ui.add_space(8.0);
                if ui.button(t!("backup-close-button")).clicked() {
                    close = true;
                }
            });
        if close {
            self.dismiss_backup();
        }
    }

    /// Blocking "Save As" for the dump, then hand off to the worker
    /// (ADR-0049 slice e). Mirrors [`save_csv_via_dialog`]: opens in
    /// Downloads with a non-colliding default name, and a cancelled dialog
    /// abandons the flow (state already reset to `Idle` by the caller). On a
    /// chosen path we send `StartBackup` and move to `Running`; a closed
    /// channel is reported as a failure rather than silently dropped.
    fn pick_path_and_start(&mut self, plan: DumpPlan) {
        let download_dir = directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(std::path::Path::to_path_buf));
        let file_name = match &download_dir {
            Some(dir) => {
                export::next_available_name("dbboard-backup", "sql", |name| dir.join(name).exists())
            }
            None => "dbboard-backup.sql".to_string(),
        };
        let mut dialog = rfd::FileDialog::new()
            .set_title(t!("backup-dialog-title"))
            .add_filter("SQL", &["sql"])
            .set_file_name(file_name);
        if let Some(dir) = download_dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return; // user cancelled; state is already Idle
        };
        if self
            .cmd_tx
            .send(Command::StartBackup { path, plan })
            .is_ok()
        {
            self.backup = BackupState::Running(DumpProgress::default());
        } else {
            self.backup = BackupState::Failed("backup worker unavailable".to_string());
        }
    }

    /// Whether the active connection can be restored into (ADR-0051): a live
    /// adapter is wired, its id maps to a SQL dialect the classifier knows, and
    /// it advertises `has_execute` so statements can actually be applied. Read
    /// per frame to gate the toolbar button, mirroring [`Self::can_backup`];
    /// `false` in tests / in-memory flows, which simply hides the button.
    #[must_use]
    pub fn can_restore(&self) -> bool {
        self.schema_source.as_ref().is_some_and(|s| {
            let adapter = s.current_adapter();
            edit::dialect_for_adapter_id(adapter.id()).is_some()
                && adapter.capabilities().has_execute
        })
    }

    /// Whether a restore is mid-flight (planning, confirming, or applying) —
    /// used to gate the toolbar button so a second click cannot start a
    /// parallel run. A terminal state (`Done`/`Failed`) does not count as busy:
    /// its summary window is dismissible and a fresh restore may start behind
    /// it.
    #[must_use]
    fn restore_in_progress(&self) -> bool {
        matches!(
            self.restore,
            RestoreState::Planning | RestoreState::Confirming { .. } | RestoreState::Running(_)
        )
    }

    /// Start a restore by picking a `.sql` file and asking the worker to
    /// preflight it (ADR-0051). No-op if one is already in flight or the
    /// connection cannot be restored into — the toolbar already gates on both,
    /// so this is defence-in-depth. A cancelled file dialog leaves the state
    /// untouched; a closed channel does too (the UI is shutting down anyway).
    fn start_restore(&mut self) {
        if self.restore_in_progress() || !self.can_restore() {
            return;
        }
        let download_dir = directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(std::path::Path::to_path_buf));
        let mut dialog = rfd::FileDialog::new()
            .set_title(t!("restore-dialog-title"))
            .add_filter("SQL", &["sql"]);
        if let Some(dir) = download_dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return; // user cancelled; state stays Idle
        };
        if self.cmd_tx.send(Command::PlanRestore { path }).is_ok() {
            self.restore = RestoreState::Planning;
        }
    }

    /// Fold a preflight reply into the state machine. A late reply that arrives
    /// after the user dismissed the flow (state no longer `Planning`) is
    /// ignored so a stale plan cannot reopen a modal. An empty target runs
    /// straight away; a non-empty one raises the strong-confirm modal first.
    fn on_restore_planned(&mut self, result: DbResult<RestorePlan>) {
        if !matches!(self.restore, RestoreState::Planning) {
            return;
        }
        match result {
            Ok(plan) if plan.is_target_empty() => self.launch_restore(plan, false),
            Ok(plan) => self.restore = RestoreState::Confirming { plan },
            Err(e) => self.restore = RestoreState::Failed(e.message().to_string()),
        }
    }

    /// Hand a confirmed (or empty-target) plan to the worker (ADR-0051).
    /// `confirmed` carries the empty-target-gate acknowledgement — always
    /// `true` on the non-empty path the modal drives, ignored by the runner on
    /// an already-empty target. A closed channel is reported as a failure
    /// rather than silently dropped.
    fn launch_restore(&mut self, plan: RestorePlan, confirmed: bool) {
        let options = RestoreOptions {
            confirmed,
            on_error: OnError::Stop,
        };
        if self
            .cmd_tx
            .send(Command::StartRestore { plan, options })
            .is_ok()
        {
            self.restore = RestoreState::Running(RestoreProgress::default());
        } else {
            self.restore = RestoreState::Failed("restore worker unavailable".to_string());
        }
    }

    /// Apply a progress tick. Ticks only matter while a restore is running; one
    /// arriving after cancellation/completion (state already terminal) is
    /// dropped so it cannot resurrect the progress window.
    fn on_restore_progress(&mut self, progress: RestoreProgress) {
        if matches!(self.restore, RestoreState::Running(_)) {
            self.restore = RestoreState::Running(progress);
        }
    }

    /// Ask the worker to cancel the in-flight restore (ADR-0051). The task
    /// still reports a (cancelled) `RestoreComplete`, so the state stays
    /// `Running` until that terminal reply lands.
    fn cancel_restore(&mut self) {
        let _ = self.cmd_tx.send(Command::CancelRestore);
    }

    /// Dismiss a terminal restore summary, returning the slot to idle.
    fn dismiss_restore(&mut self) {
        self.restore = RestoreState::Idle;
    }

    /// Render whichever restore window the current state calls for. `Idle`/
    /// `Planning` own no window — the toolbar carries their affordance (button
    /// / spinner). Matching on a place with non-binding patterns lets each arm
    /// freely re-borrow `self` for its sub-render.
    fn render_restore(&mut self, ctx: &egui::Context) {
        match self.restore {
            RestoreState::Idle | RestoreState::Planning => {}
            RestoreState::Confirming { .. } => self.render_restore_confirm(ctx),
            RestoreState::Running(_) => self.render_restore_running(ctx),
            RestoreState::Done(_) => self.render_restore_done(ctx),
            RestoreState::Failed(_) => self.render_restore_failed(ctx),
        }
    }

    /// The non-empty-target strong-confirm (ADR-0051 empty/new-target safety
    /// model). Names how many tables the target already holds; "Restore
    /// anyway" launches with `confirmed = true`, "Cancel" abandons the flow.
    fn render_restore_confirm(&mut self, ctx: &egui::Context) {
        let RestoreState::Confirming { plan } = &self.restore else {
            return;
        };
        let existing = plan.existing_tables.len();
        let statements = plan.runnable_count();
        let mut action = ConfirmAction::None;
        egui::Window::new(t!("restore-warn-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(t_args!(
                    "restore-warn-body",
                    tables = existing,
                    statements = statements
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("restore-warn-continue")).clicked() {
                        action = ConfirmAction::Continue;
                    }
                    if ui.button(t!("restore-warn-cancel")).clicked() {
                        action = ConfirmAction::Cancel;
                    }
                });
            });
        match action {
            ConfirmAction::Continue => {
                if let RestoreState::Confirming { plan } =
                    std::mem::replace(&mut self.restore, RestoreState::Idle)
                {
                    self.launch_restore(plan, true);
                }
            }
            ConfirmAction::Cancel => self.restore = RestoreState::Idle,
            ConfirmAction::None => {}
        }
    }

    /// The live progress window: a bar plus a statement counter and a Cancel
    /// button. Cancel only signals the worker; the terminal reply flips the
    /// state, so the window stays up (showing the last progress) until then.
    fn render_restore_running(&mut self, ctx: &egui::Context) {
        let RestoreState::Running(progress) = &self.restore else {
            return;
        };
        let fraction = restore_fraction(progress);
        let statements_line = t_args!(
            "restore-progress-statements",
            done = progress.statements_done,
            total = progress.statements_total
        );
        let mut cancel = false;
        egui::Window::new(t!("restore-progress-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                ui.label(statements_line);
                ui.add_space(8.0);
                if ui.button(t!("restore-cancel-button")).clicked() {
                    cancel = true;
                }
            });
        // Keep the frame ticking so progress replies keep draining.
        ctx.request_repaint();
        if cancel {
            self.cancel_restore();
        }
    }

    /// Completion summary. Names the applied totals and surfaces the honest
    /// caveats — cancellation and per-statement failures — so a partial restore
    /// is never mistaken for a clean one. Close returns to idle.
    fn render_restore_done(&mut self, ctx: &egui::Context) {
        let RestoreState::Done(outcome) = &self.restore else {
            return;
        };
        let summary = t_args!(
            "restore-done-summary",
            statements = outcome.statements_run,
            ddl = outcome.ddl_run,
            data = outcome.data_run
        );
        let cancelled = outcome.cancelled;
        let failures = outcome.failures.len();
        let mut close = false;
        egui::Window::new(t!("restore-done-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(summary);
                if cancelled {
                    ui.label(t!("restore-done-cancelled"));
                }
                if failures > 0 {
                    ui.label(t_args!("restore-done-failures", count = failures));
                }
                ui.add_space(8.0);
                if ui.button(t!("restore-close-button")).clicked() {
                    close = true;
                }
            });
        if close {
            self.dismiss_restore();
        }
    }

    /// Failure window: a fatal error prevented (or unwound) the restore. Close
    /// returns to idle.
    fn render_restore_failed(&mut self, ctx: &egui::Context) {
        let RestoreState::Failed(message) = &self.restore else {
            return;
        };
        // A raw error string has no separate localized/original halves, so both
        // slots carry the same text (only one line renders).
        let display = errors::DisplayError::new(message.clone(), message.clone());
        let mut close = false;
        egui::Window::new(t!("restore-failed-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                errors::render_error(ui, Some(&display));
                ui.add_space(8.0);
                if ui.button(t!("restore-close-button")).clicked() {
                    close = true;
                }
            });
        if close {
            self.dismiss_restore();
        }
    }

    /// Toggle the AI panel window. Noop when no provider is wired —
    /// callers do not need to gate this themselves, but in practice the
    /// menu bar already hides the button so this is defence-in-depth.
    pub fn toggle_ai_panel(&mut self) {
        if self.has_ai_provider() {
            self.ai_panel.toggle();
        }
    }

    /// Read-only access to the AI panel state for tests and binary-side
    /// observers. Exposed because the panel's state is interesting to
    /// inspect from outside even when not rendered (e.g. integration
    /// tests asserting that `Reply::AiResponded` routed into it).
    #[must_use]
    pub fn ai_panel(&self) -> &AiPanel {
        &self.ai_panel
    }

    /// Request an in-process AI provider swap to the entry named `id`
    /// in `ai-providers.toml` (ADR-0025). The actual rebuild + slot
    /// swap happens in the worker thread via the
    /// [`worker::AiProviderSwitcher`] the binary injected at startup;
    /// the UI just signals intent here. An `AiProviderSwitched` /
    /// `AiProviderSwitchFailed` reply lands on the next `drain_replies`
    /// pass.
    pub fn switch_ai_provider(&mut self, id: String) {
        let _ = self.cmd_tx.send(Command::SwitchAiProvider { id });
    }

    /// Push the resolved display name of the currently-bound AI
    /// provider down to the panel. The binary owns the
    /// `AiSettingsAdmin` (the only id↔name source of truth) and is
    /// expected to call this each frame with the current
    /// `admin.active_id()` looked up against `admin.entries()`. `None`
    /// suppresses the panel subtitle — used when no provider is bound.
    pub fn set_active_ai_provider_label(&mut self, label: Option<String>) {
        self.active_ai_provider_label = label;
    }

    /// Snapshot of the label most recently pushed by
    /// [`Self::set_active_ai_provider_label`]. Useful for tests and for
    /// other binary-side observers that need the same string the panel
    /// will render on its next frame.
    #[must_use]
    pub fn active_ai_provider_label(&self) -> Option<&str> {
        self.active_ai_provider_label.as_deref()
    }

    /// Set the backup large-database warn threshold in total rows (ADR-0050).
    /// The binary owns `ui-settings.toml` (the persisted source of truth) and
    /// pushes the resolved value in — the persisted setting when present, the
    /// [`DEFAULT_BACKUP_WARN_ROWS`] fallback otherwise. The next preflight
    /// ([`Self::on_backup_planned`]) compares against this.
    pub fn set_backup_warn_rows(&mut self, rows: u64) {
        self.backup_warn_rows = rows;
    }

    /// The threshold the next preflight will use. Seeded to
    /// [`DEFAULT_BACKUP_WARN_ROWS`]; reflects the last value pushed by
    /// [`Self::set_backup_warn_rows`]. Exposed so the binary can read the
    /// built-in default back out (to seed the settings editor) without
    /// re-importing the core constant.
    #[must_use]
    pub fn backup_warn_rows(&self) -> u64 {
        self.backup_warn_rows
    }

    /// Last `SwitchAiProvider` failure surfaced through
    /// [`Reply::AiProviderSwitchFailed`]. Cleared on the next
    /// successful switch. The body is the `AiError::Display` text the
    /// switcher produced; the AI panel can render it inline without
    /// re-translating the AI taxonomy through `DbError`.
    #[must_use]
    pub fn last_ai_switch_error(&self) -> Option<&str> {
        self.last_ai_switch_error.as_deref()
    }
}

/// Build the completion record stamped on disk at reply time
/// (ADR-0017). Carries the connection label, RFC 3339 timestamp, and
/// the result envelope's row / affected / error split.
fn build_completion_entry(
    reply: &DbResult<QueryResult>,
    pending: &PendingSubmit,
    conn: &str,
    ts: String,
) -> HistoryEntry {
    let duration_ms = u64::try_from(pending.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match reply {
        Ok(q) => {
            // The wire shape conflates "row-returning with 0 rows" and
            // "DML/DDL with affected count" through a single rows_affected
            // u64 (defaulting to 0). The empty-columns heuristic is the
            // closest split available from the contract today: a SELECT
            // always declares columns, DML/DDL never does (Phase 1
            // adapters agree on this).
            let (rows, rows_affected) = if q.columns.is_empty() {
                (None, Some(q.rows_affected))
            } else {
                (Some(q.rows.len() as u64), None)
            };
            HistoryEntry::Query(QueryEntry {
                sql: pending.sql.clone(),
                ts,
                conn: conn.to_string(),
                status: HistoryStatus::Ok,
                duration_ms,
                rows,
                rows_affected,
                error: None,
            })
        }
        Err(e) => HistoryEntry::Query(QueryEntry {
            sql: pending.sql.clone(),
            ts,
            conn: conn.to_string(),
            status: HistoryStatus::Error,
            duration_ms,
            rows: None,
            rows_affected: None,
            error: Some(HistoryError {
                category: e.category().to_string(),
                message: e.message().to_string(),
            }),
        }),
    }
}

/// Extract a submit-time snapshot from an outgoing AI [`Command`]
/// (ADR-0027 slice c). Returns `None` for non-AI commands and for
/// [`Command::CancelAiRequest`] — a cancel must not overwrite the
/// pending record that belongs to the request it is cancelling.
///
/// The panel already gated the send on non-empty input, so the
/// `prompt` field carries the trimmed user intent verbatim (the
/// panel does not trim, so we do not trim either — keeping the
/// history text byte-identical to what the provider saw).
fn pending_ai_from_command(cmd: &Command, conn_label: &str) -> Option<PendingAiSubmit> {
    let (intent, prompt) = match cmd {
        Command::AiExplain { sql, .. } | Command::AiExplainStream { sql, .. } => {
            (AiIntent::Explain, sql.clone())
        }
        Command::AiSuggest { prompt, .. } | Command::AiSuggestStream { prompt, .. } => {
            (AiIntent::SuggestSql, prompt.clone())
        }
        // PrefetchSchema is a *precursor* to a Suggest, not the AI
        // request itself — the deferred AiSuggest[Stream] built by
        // `on_schema_prefetched` is what snapshots the pending record.
        Command::CancelAiRequest
        | Command::ListTables
        | Command::Query(_)
        | Command::SwitchConnection { .. }
        | Command::SwitchAiProvider { .. }
        | Command::PrefetchSchema { .. }
        | Command::DescribeTable { .. }
        | Command::PlanBackup
        | Command::StartBackup { .. }
        | Command::CancelBackup
        | Command::PlanRestore { .. }
        | Command::StartRestore { .. }
        | Command::CancelRestore => return None,
    };
    let conn = if conn_label.is_empty() {
        None
    } else {
        Some(conn_label.to_string())
    };
    Some(PendingAiSubmit {
        started: Instant::now(),
        intent,
        prompt,
        conn,
    })
}

/// Build the success record for an atomic [`Reply::AiResponded`] or a
/// streaming [`Reply::AiStreamComplete`]. `response` is the full text
/// (accumulated by the panel for the streaming path). `identity` is the
/// spawn-time `(provider, model)` pair the worker stamped on the reply
/// (ADR-0027 slice b). `stop_reason` is `None` for the atomic path —
/// the provider does not surface one there — and `Some(wire)` for the
/// streaming path.
fn build_ai_ok_entry(
    pending: PendingAiSubmit,
    response: String,
    tokens_in: u32,
    tokens_out: u32,
    identity: (String, String),
    stop_reason: Option<String>,
    ts: String,
) -> AiEntry {
    let duration_ms = u64::try_from(pending.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (provider, model) = identity;
    AiEntry {
        ts,
        conn: pending.conn,
        intent: pending.intent,
        prompt: pending.prompt,
        response,
        status: AiStatus::Ok,
        duration_ms,
        tokens_in: Some(u64::from(tokens_in)),
        tokens_out: Some(u64::from(tokens_out)),
        provider,
        model,
        stop_reason,
        error: None,
    }
}

/// Build the error record for [`Reply::AiFailed`]. Response text is
/// empty (the provider never produced one) and token counts are
/// `None` since the failure taxonomy is orthogonal to usage — a
/// mid-stream error may follow a usage event, but preserving that
/// partial count against a failure is out of scope for slice c.
fn build_ai_failed_entry(
    pending: PendingAiSubmit,
    error: &AiError,
    identity: (String, String),
    ts: String,
) -> AiEntry {
    let duration_ms = u64::try_from(pending.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (category, message) = ai_error_history_parts(error);
    let (provider, model) = identity;
    AiEntry {
        ts,
        conn: pending.conn,
        intent: pending.intent,
        prompt: pending.prompt,
        response: String::new(),
        status: AiStatus::Error,
        duration_ms,
        tokens_in: None,
        tokens_out: None,
        provider,
        model,
        stop_reason: None,
        error: Some(HistoryError { category, message }),
    }
}

/// Build the cancelled record for [`Reply::AiCancelled`]. `partial` is
/// the streaming accumulator peeked just before `on_cancelled` drains
/// it: `Some` for cancels after the first streaming chunk, `None` for
/// the atomic / pre-first-chunk paths. Token counts are surfaced when
/// a usage event actually landed (any non-zero count is treated as a
/// real observation — mirrors the ADR-0027 §Decision 5 note that
/// atomic / pre-usage cancels use `None`).
fn build_ai_cancelled_entry(
    pending: PendingAiSubmit,
    partial: Option<ai::StreamingAcc>,
    identity: (String, String),
    ts: String,
) -> AiEntry {
    let duration_ms = u64::try_from(pending.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (provider, model) = identity;
    let (response, tokens_in, tokens_out) = match partial {
        Some(acc) => {
            let tin =
                (acc.tokens_in != 0 || acc.tokens_out != 0).then_some(u64::from(acc.tokens_in));
            let tout =
                (acc.tokens_in != 0 || acc.tokens_out != 0).then_some(u64::from(acc.tokens_out));
            (acc.text, tin, tout)
        }
        None => (String::new(), None, None),
    };
    AiEntry {
        ts,
        conn: pending.conn,
        intent: pending.intent,
        prompt: pending.prompt,
        response,
        status: AiStatus::Cancelled,
        duration_ms,
        tokens_in,
        tokens_out,
        provider,
        model,
        stop_reason: None,
        error: None,
    }
}

/// Wire string for the `stop_reason` field on a v:2 AI record
/// (ADR-0027). Free-form on read (`Option<String>`), so this is the
/// only place that pins the serialization — an unknown future
/// `StopReason::Other` value flows through verbatim.
fn stop_reason_wire(reason: &StopReason) -> String {
    match reason {
        StopReason::EndTurn => "end_turn".into(),
        StopReason::MaxTokens => "max_tokens".into(),
        StopReason::StopSequence => "stop_sequence".into(),
        StopReason::ToolUse => "tool_use".into(),
        StopReason::Refusal => "refusal".into(),
        StopReason::Other(s) => s.clone(),
    }
}

/// Split an [`AiError`] into the `(category, message)` pair the v:2
/// wire uses (ADR-0027 §Decision 4 mirrors ADR-0023 §5). `Cancelled`
/// is not expected here — cancels flow through [`Reply::AiCancelled`],
/// not [`Reply::AiFailed`] — but we map it defensively so a stray
/// variant does not corrupt the record.
fn ai_error_history_parts(error: &AiError) -> (String, String) {
    match error {
        AiError::Configuration(m) => ("configuration".into(), m.clone()),
        AiError::Network(m) => ("network".into(), m.clone()),
        AiError::Provider(m) => ("provider".into(), m.clone()),
        AiError::Quota(m) => ("quota".into(), m.clone()),
        AiError::Cancelled => ("cancelled".into(), String::new()),
    }
}

impl eframe::App for DbboardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_replies();

        // Keyboard run triggers: F5 or the platform command modifier plus
        // Enter run the current statement without reaching for the button.
        // `run_sql` guards busy/empty, so a stray press mid-query is a
        // no-op. Read once per frame to keep the trigger rule in one place.
        let run_from_keys = ui.input(|i| {
            should_run_from_keys(
                i.key_pressed(egui::Key::F5),
                i.modifiers.command,
                i.key_pressed(egui::Key::Enter),
            )
        });
        if run_from_keys {
            self.run_sql();
        }

        // ADR-0023: AI panel as a free-floating egui::Window. Only
        // register it when a provider was wired in at startup; the panel
        // itself trusts the gate.
        if self.has_ai_provider() {
            self.render_ai_panel(ui.ctx());
        }

        self.render_tables_panel(ui);
        self.render_query_panel(ui);

        // ADR-0049 slice e: backup modals/progress float over everything,
        // like the AI panel. Rendered after the main panels so they layer
        // on top.
        self.render_backup(ui.ctx());

        // ADR-0051 slice 6: restore modals/progress float over everything,
        // like the backup ones.
        self.render_restore(ui.ctx());

        // Egui is event-driven, so request a follow-up frame while a query,
        // a backup, or a restore is in flight to keep draining the reply
        // channel. The running-progress windows also request repaints
        // themselves; this covers the Planning gap before their window exists.
        if self.busy || self.backup_in_progress() || self.restore_in_progress() {
            ui.ctx().request_repaint();
        }
    }
}

impl DbboardApp {
    /// Left sidebar: the list of user tables (ADR-0014). Clicking a table
    /// opens its structure tab (ADR-0031). The click is captured here and
    /// acted on after the `&self.tables` borrow ends.
    fn render_tables_panel(&mut self, ui: &mut egui::Ui) {
        let active = self.structure.as_ref().map(|s| s.table.clone());
        let mut clicked: Option<TableInfo> = None;
        // A quick-SQL starter picked from a row's right-click menu. Applied
        // to the editor after the `&self.tables` borrow ends, mirroring how
        // `clicked` defers `open_structure`.
        let mut quick_sql: Option<String> = None;
        // A right-click "Select" browses the table as an *editable* result
        // (issue 0013 slice b); it carries the source `TableInfo` rather
        // than a bare SQL string so provenance survives. "Count" stays a
        // plain read-only starter.
        let mut quick_browse: Option<TableInfo> = None;
        egui::Panel::left("tables").show_inside(ui, |ui| {
            ui.heading(t!("tables-heading"));
            ui.separator();
            match &self.tables {
                Ok(tables) if tables.is_empty() => {
                    ui.label(t!("tables-empty"));
                }
                Ok(tables) => {
                    // Justified top-down layout stretches each row to the
                    // panel's full width so the whole row is the click
                    // target, not just the text glyphs. A short table name
                    // was a tiny hit area before; now the empty space to
                    // its right selects it too. Text stays left-aligned.
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        for table in tables {
                            let selected = active.as_ref() == Some(table);
                            let row = ui.selectable_label(selected, &table.name);
                            if row.clicked() {
                                clicked = Some(table.clone());
                            }
                            // Right-click a table for quick starter queries
                            // that drop into the editor and run immediately
                            // (issue 0012). Read-only by design (no
                            // DELETE/DROP): this ships to a data-collection
                            // user, and a mis-click should never be
                            // destructive — so auto-running a starter is safe.
                            row.context_menu(|ui| {
                                if ui.button(t!("tables-context-select")).clicked() {
                                    quick_browse = Some(table.clone());
                                    ui.close();
                                }
                                if ui.button(t!("tables-context-count")).clicked() {
                                    quick_sql = Some(quick_count_sql(table));
                                    ui.close();
                                }
                            });
                        }
                    });
                }
                Err(e) => {
                    errors::render_error(ui, Some(&errors::db_error_display(e)));
                }
            }
        });
        if let Some(table) = quick_browse {
            self.run_table_browse(table);
        } else if let Some(sql) = quick_sql {
            self.run_quick_sql(sql);
        }
        if let Some(table) = clicked {
            self.open_structure(table);
        }
    }

    /// Central panel: the SQL editor, run controls, history, and result.
    fn render_query_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(t!("sql-heading"));
                if ui
                    .add_enabled(!self.busy, egui::Button::new(t!("sql-run-button")))
                    .clicked()
                {
                    self.run_sql();
                }
                // Visible, overridable bare-SELECT guard (ADR-0030).
                ui.checkbox(
                    &mut self.auto_limit,
                    t_args!("auto-limit-checkbox", count = DEFAULT_AUTO_LIMIT),
                )
                .on_hover_text(t!("auto-limit-hint"));
                // ADR-0049 backup: only shown when the live connection can be
                // dumped, and disabled while any backup is in flight so a
                // second click cannot start a parallel run.
                if self.can_backup() {
                    if ui
                        .add_enabled(
                            !self.backup_in_progress(),
                            egui::Button::new(t!("backup-button")),
                        )
                        .on_hover_text(t!("backup-button-hint"))
                        .clicked()
                    {
                        self.start_backup();
                    }
                    if matches!(self.backup, BackupState::Planning) {
                        ui.label(t!("backup-planning"));
                    }
                }
                // ADR-0051 restore: shown only when the live connection can be
                // restored into (dialect known + `has_execute`), and disabled
                // while any restore is in flight so a second click cannot start
                // a parallel run.
                if self.can_restore() {
                    if ui
                        .add_enabled(
                            !self.restore_in_progress(),
                            egui::Button::new(t!("restore-button")),
                        )
                        .on_hover_text(t!("restore-button-hint"))
                        .clicked()
                    {
                        self.start_restore();
                    }
                    if matches!(self.restore, RestoreState::Planning) {
                        ui.label(t!("restore-planning"));
                    }
                }
                if self.busy {
                    ui.spinner();
                }
            });
            let editor = ui.add(
                egui::TextEdit::multiline(&mut self.sql)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            // Right-click the editor to run without leaving the keyboard's
            // home for the toolbar button. Disabled while a query is in
            // flight, matching the Run button's gate.
            let busy = self.busy;
            let mut run_from_menu = false;
            editor.context_menu(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new(t!("sql-run-button")))
                    .clicked()
                {
                    run_from_menu = true;
                    ui.close();
                }
            });
            if run_from_menu {
                self.run_sql();
            }

            // Recently-run statements; click one to refill the editor
            // (ADR-0014). Restore is captured here and applied after the
            // immutable iter() borrow ends, sidestepping the borrow
            // checker without cloning the whole store.
            let mut restore: Option<String> = None;
            {
                let history = self.history.store();
                egui::CollapsingHeader::new(t_args!("history-title", count = history.len()))
                    .default_open(false)
                    .show(ui, |ui| {
                        if history.is_empty() {
                            ui.label(t!("history-empty"));
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    // Slice (a) only surfaces query entries
                                    // in the legacy history panel; the AI
                                    // record viewer lands in slice (c).
                                    for entry in history.iter() {
                                        let HistoryEntry::Query(q) = entry else {
                                            continue;
                                        };
                                        if ui.small_button(history_button_label(&q.sql)).clicked() {
                                            restore = Some(q.sql.clone());
                                        }
                                    }
                                });
                        }
                    });
            }
            if let Some(sql) = restore {
                self.sql = sql;
            }

            ui.separator();
            self.render_result_area(ui);
        });
    }

    /// Result/structure tab body and the inline-edit action handoff
    /// (ADR-0031 + issue 0013 slice b). Split out of `render_query_panel`
    /// to keep each function focused.
    fn render_result_area(&mut self, ui: &mut egui::Ui) {
        // ADR-0031: tab between the query result and the clicked table's
        // structure.
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, ResultTab::Results, t!("tab-results"));
            ui.selectable_value(
                &mut self.active_tab,
                ResultTab::Structure,
                t!("tab-structure"),
            );
        });
        // Inline-edit actions bubble out of the grid so they run against
        // `&mut self` once the grid's field borrows end (issue 0013 slice b).
        let mut grid_intent = None;
        let busy = self.busy;
        match self.active_tab {
            ResultTab::Results => match &self.last_result {
                None => {
                    ui.label(t!("result-empty"));
                }
                Some(Ok(result)) => {
                    grid_intent = render_result(
                        ui,
                        result,
                        &mut self.result_selection,
                        &mut self.result_sort,
                        &mut self.edit,
                        busy,
                    );
                }
                Some(Err(e)) => {
                    errors::render_error(ui, Some(&errors::db_error_display(e)));
                }
            },
            ResultTab::Structure => self.render_structure(ui),
        }
        match grid_intent {
            Some(GridIntent::Save) => self.begin_save(),
            Some(GridIntent::Discard) => self.discard_edits(),
            None => {}
        }
    }

    /// Structure tab body (ADR-0031): the selected table's name and its
    /// `describe_table` outcome rendered as a column grid.
    fn render_structure(&mut self, ui: &mut egui::Ui) {
        let Some(view) = &self.structure else {
            ui.label(t!("structure-empty"));
            return;
        };
        ui.strong(&view.table.name);
        ui.separator();

        // Take the columns to render, dropping the borrow on `self.structure`
        // so the note buffers (also on the view) and the annotations store
        // can be borrowed mutably below.
        let columns = match &view.schema {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t!("structure-loading"));
                });
                return;
            }
            Some(Err(e)) => {
                errors::render_error(ui, Some(&errors::db_error_display(e)));
                return;
            }
            Some(Ok(schema)) => schema.columns.clone(),
        };
        let table_key = annotation_table_key(view.table.schema.as_deref(), &view.table.name);
        let conn = self.conn_label.clone();
        // The Note column is inert without a wired store (tests / in-memory
        // flows): still rendered, but disabled so it reads as "not here yet"
        // rather than silently swallowing edits.
        let has_store = self.annotations.is_some();

        self.render_table_note(ui, &table_key, &conn, has_store);

        if columns.is_empty() {
            ui.label(t!("structure-no-columns"));
            return;
        }

        self.render_schema_grid(ui, &columns, &table_key, &conn, has_store);
    }

    /// Render the column grid with the editable Note column (ADR-0045).
    /// Split from [`Self::render_structure`] so each stays under the size
    /// limit; takes the already-derived `table_key`/`conn` so it does not
    /// re-touch `self.structure` while iterating the cloned columns.
    fn render_schema_grid(
        &mut self,
        ui: &mut egui::Ui,
        columns: &[ColumnInfo],
        table_key: &str,
        conn: &str,
        has_store: bool,
    ) {
        use egui_extras::{Column, TableBuilder};

        let row_height = egui::TextStyle::Body.resolve(ui.style()).size + 8.0;
        let headers: [String; 7] = [
            t!("structure-col-ordinal"),
            t!("structure-col-name"),
            t!("structure-col-type"),
            t!("structure-col-nullable"),
            t!("structure-col-pk"),
            t!("structure-col-default"),
            t!("structure-col-note"),
        ];

        // Bind the two disjoint `self` fields as locals so the table
        // closures capture only these, not `self` — `note_buffers` lives on
        // `self.structure`, the stored notes on `self.annotations`.
        let buffers = &mut self
            .structure
            .as_mut()
            .expect("structure present")
            .note_buffers;
        let annotations = self.annotations.as_ref();
        // (column name, edited text) pairs to persist once the borrows drop.
        let mut commits: Vec<(String, String)> = Vec::new();

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .auto_shrink([false, false]);
        for _ in &headers {
            table = table.column(Column::auto().at_least(48.0).clip(true).resizable(true));
        }
        table
            .header(row_height, |mut header| {
                for h in &headers {
                    header.col(|ui| {
                        ui.strong(h.as_str());
                    });
                }
            })
            .body(|mut body| {
                for col in columns {
                    body.row(row_height, |mut row| {
                        row.col(|ui| {
                            ui.label(col.ordinal.to_string());
                        });
                        row.col(|ui| {
                            ui.label(&col.name);
                        });
                        row.col(|ui| {
                            ui.label(col.declared_type.as_deref().unwrap_or(""));
                        });
                        // A checkmark reads the same in every locale, so the
                        // nullable / PK cells stay text-key-free.
                        row.col(|ui| {
                            ui.label(if col.nullable { "✓" } else { "" });
                        });
                        row.col(|ui| {
                            ui.label(if col.primary_key { "PK" } else { "" });
                        });
                        row.col(|ui| {
                            ui.label(col.default_value.as_deref().unwrap_or(""));
                        });
                        row.col(|ui| {
                            let stored = annotations
                                .and_then(|a| a.column_note(conn, table_key, &col.name))
                                .unwrap_or("")
                                .to_string();
                            let buf = buffers
                                .entry(col.name.clone())
                                .or_insert_with(|| stored.clone());
                            let resp = ui.add_enabled(
                                has_store,
                                egui::TextEdit::singleline(buf)
                                    .hint_text(t!("structure-note-hint"))
                                    .desired_width(f32::INFINITY),
                            );
                            // Enter and click-away both surrender focus; only
                            // persist when the text actually changed so an
                            // idle focus pass never rewrites the file.
                            if resp.lost_focus() && buf.trim() != stored.trim() {
                                commits.push((col.name.clone(), buf.clone()));
                            }
                        });
                    });
                }
            });

        for (column, text) in commits {
            self.commit_structure_note(&NoteTarget::Column(column), &text);
        }
    }

    /// Render the single-line table-level note editor above the column
    /// grid (ADR-0045). Split out of [`Self::render_structure`] to keep
    /// that method under the size limit and to isolate the table-note
    /// buffer's borrow from the per-column one.
    fn render_table_note(&mut self, ui: &mut egui::Ui, table_key: &str, conn: &str, enabled: bool) {
        let stored = self
            .annotations
            .as_ref()
            .and_then(|a| a.table_note(conn, table_key))
            .unwrap_or("")
            .to_string();
        let buf = self
            .structure
            .as_mut()
            .expect("structure present")
            .table_note_buffer
            .get_or_insert_with(|| stored.clone());
        let mut commit: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label(t!("structure-table-note"));
            let resp = ui.add_enabled(
                enabled,
                egui::TextEdit::singleline(buf)
                    .hint_text(t!("structure-note-hint"))
                    .desired_width(f32::INFINITY),
            );
            if resp.lost_focus() && buf.trim() != stored.trim() {
                commit = Some(buf.clone());
            }
        });
        if let Some(text) = commit {
            self.commit_structure_note(&NoteTarget::Table, &text);
        }
    }
}

/// Label for a history-entry button: first line of the SQL, truncated
/// to a one-row width with an ellipsis so a multi-statement entry does
/// not stretch the panel.
fn history_button_label(sql: &str) -> String {
    const MAX_CHARS: usize = 80;
    let line = sql.lines().next().unwrap_or("").trim();
    let char_count = line.chars().count();
    if char_count > MAX_CHARS {
        let head: String = line.chars().take(MAX_CHARS.saturating_sub(3)).collect();
        format!("{head}...")
    } else {
        line.to_string()
    }
}

/// Render a `DbError` as `<translated prefix>: <wire message>`. The
/// prefix comes from the active locale's `error-prefix-*` keys; the
/// message body is the server-returned English string and stays as-is
/// to preserve the ADR-0009 HTTP contract (see ADR-0015).
/// Whether the current frame's keyboard state should trigger a run.
///
/// The editor is a multiline field, so a bare Enter must insert a
/// newline — only `F5` or the platform command modifier plus Enter
/// (Ctrl+Enter on Windows/Linux, Cmd+Enter on macOS) count as "run".
/// Kept pure so the trigger rules are testable without an egui frame.
fn should_run_from_keys(f5_pressed: bool, cmd_held: bool, enter_pressed: bool) -> bool {
    f5_pressed || (cmd_held && enter_pressed)
}

/// Row cap appended to unbounded bare `SELECT`s (ADR-0030). A safety net,
/// not a hard limit: the user can raise it by writing their own `LIMIT` or
/// disable it with the toolbar checkbox.
const DEFAULT_AUTO_LIMIT: u32 = 100;

/// True when `sql` is a single plain `SELECT` with no `LIMIT` — the only
/// shape the auto-limit guard touches. CTEs (`WITH …`), multi-statement
/// input, and anything already carrying a `LIMIT` are left alone so the
/// guard never changes a query's meaning.
fn is_bare_select(sql: &str) -> bool {
    let stripped = sql.trim().trim_end_matches(';').trim();
    // Internal `;` means multiple statements; appending a LIMIT would bind
    // to the wrong one, so bail.
    if stripped.contains(';') {
        return false;
    }
    let lower = stripped.to_ascii_lowercase();
    let starts_select = lower == "select"
        || lower
            .strip_prefix("select")
            .is_some_and(|rest| rest.starts_with([' ', '\n', '\t', '\r']));
    starts_select && !has_limit_token(&lower)
}

/// Whether a `limit` keyword appears as a standalone token (not as a
/// substring of an identifier like `limits`).
fn has_limit_token(lower: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|tok| tok == "limit")
}

/// The statement to actually execute: `sql` unchanged, or with
/// ` LIMIT {limit}` appended when the guard is on and `sql` is a bare
/// `SELECT`. A trailing semicolon is dropped so the result stays valid.
fn apply_auto_limit(sql: &str, enabled: bool, limit: u32) -> String {
    if !enabled || !is_bare_select(sql) {
        return sql.to_owned();
    }
    let trimmed = sql.trim_end();
    let body = trimmed.strip_suffix(';').unwrap_or(trimmed).trim_end();
    format!("{body} LIMIT {limit}")
}

/// Longest single-line cell rendered inline before the expand affordance
/// appears. Tuned to keep wide tables scannable, not to any exact pixel
/// width.
const CELL_PREVIEW_CHARS: usize = 80;

/// egui memory key for the single full-text popup. One result grid is ever
/// on screen, so a fixed id is enough and keeps `render_result` a free
/// function (no popup state threaded through `DbboardApp`).
fn expanded_cell_id() -> egui::Id {
    egui::Id::new("dbboard-result-expanded-cell")
}

/// A cell earns the "expand" affordance when it spans multiple lines or is
/// longer than the inline preview budget — the cases where truncation
/// actually hides something.
fn is_long_cell(text: &str) -> bool {
    text.contains('\n') || text.chars().count() > CELL_PREVIEW_CHARS
}

/// Single-line, length-capped preview of a cell value. Newlines collapse to
/// spaces so every row keeps a uniform height; a trailing ellipsis marks
/// that content was elided.
fn cell_preview(text: &str) -> String {
    let single_line = text.replace(['\r', '\n'], " ");
    if !is_long_cell(text) {
        return single_line;
    }
    let head: String = single_line.chars().take(CELL_PREVIEW_CHARS).collect();
    format!("{head}…")
}

/// Quote a single SQL identifier by wrapping it in double quotes and
/// doubling any embedded quote. Double-quoted identifiers are the SQL
/// standard and are accepted by every backend dbboard targets (the
/// Postgres wire family + SQLite/libSQL), so one quoting rule covers all
/// of them. Doubling the inner quote keeps an awkward table name (or a
/// quote-injection attempt) from breaking out of the literal.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Fully-qualified, quoted table reference. Schema-qualified only where
/// the engine has schemas — SQLite/libSQL tables keep `schema: None` and
/// render unqualified.
fn quoted_table_ref(table: &TableInfo) -> String {
    match &table.schema {
        Some(schema) => format!("{}.{}", quote_ident(schema), quote_ident(&table.name)),
        None => quote_ident(&table.name),
    }
}

/// `SELECT *` starter query for the table right-click menu. The bare
/// SELECT is intentional: the ADR-0030 auto-limit guard wraps it with a
/// LIMIT at run time unless the user overrides, so we do not hard-code a
/// row cap here.
fn quick_select_sql(table: &TableInfo) -> String {
    format!("SELECT * FROM {};", quoted_table_ref(table))
}

/// `SELECT COUNT(*)` starter query for the table right-click menu — a
/// cheap "how big is this table" the collector reaches for constantly.
fn quick_count_sql(table: &TableInfo) -> String {
    format!("SELECT COUNT(*) FROM {};", quoted_table_ref(table))
}

/// Copy / save controls above the result grid (ADR-0035). The always-on
/// actions cover the whole result: "Copy" puts it on the clipboard as
/// TSV, "Save CSV" writes RFC 4180 CSV through a native "Save As" dialog.
/// Once rows are selected (slice 2) a second group appears with the same
/// two actions scoped to just the selection, a count, and a clear button.
/// A failed save surfaces through a native error dialog — kept out of the
/// egui frame so the export path never blocks a repaint.
fn render_export_toolbar(
    ui: &mut egui::Ui,
    result: &QueryResult,
    selection: &mut selection::ResultSelection,
) {
    ui.horizontal(|ui| {
        if ui
            .button(t!("result-copy-all"))
            .on_hover_text(t!("result-copy-all-hint"))
            .clicked()
        {
            ui.ctx()
                .copy_text(export::to_tsv(&result.columns, &result.rows));
        }
        if ui.button(t!("result-export-csv")).clicked() {
            save_csv_via_dialog(&result.columns, &result.rows);
        }

        // Selected-row actions only make sense once something is selected;
        // hiding them keeps the toolbar quiet on the common whole-result
        // path (ADR-0035 slice 2).
        if selection.is_empty() {
            return;
        }
        ui.separator();
        ui.label(t_args!("result-selected-count", count = selection.len()));
        if ui
            .button(t!("result-copy-selected"))
            .on_hover_text(t!("result-copy-selected-hint"))
            .clicked()
        {
            let subset = selected_rows(result, selection);
            ui.ctx().copy_text(export::to_tsv(&result.columns, &subset));
        }
        if ui.button(t!("result-export-selected-csv")).clicked() {
            let subset = selected_rows(result, selection);
            save_csv_via_dialog(&result.columns, &subset);
        }
        if ui.button(t!("result-clear-selection")).clicked() {
            selection.clear();
        }
    });
}

/// Materialize the selected rows as an owned slice for export. Cloning is
/// bounded by the selection size and only happens on a copy/save click
/// (not per frame); ascending [`selection::ResultSelection::iter`] order
/// preserves the grid's top-to-bottom order. Indices are bounds-checked
/// so a selection that somehow outlives its result cannot panic.
fn selected_rows(
    result: &QueryResult,
    selection: &selection::ResultSelection,
) -> Vec<dbboard_core::Row> {
    selection
        .iter()
        .filter_map(|i| result.rows.get(i).cloned())
        .collect()
}

/// Blocking "Save As" flow for the CSV export (ADR-0035). Returns early
/// if the user cancels the dialog. A write failure is reported with a
/// native message box rather than swallowed, so a full disk or a
/// read-only target is not silently lost. `rfd`'s dialogs are native and
/// synchronous; the brief frame stall while the OS dialog is open is the
/// expected desktop behaviour.
///
/// The dialog opens in the user's Downloads folder with an
/// Explorer/browser-style non-colliding default name (`dbboard-result
/// (2).csv`, …), so repeated exports do not pre-fill a name that would
/// overwrite the previous file.
fn save_csv_via_dialog(columns: &[dbboard_core::Column], rows: &[dbboard_core::Row]) {
    let download_dir = directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(std::path::Path::to_path_buf));
    let file_name = match &download_dir {
        Some(dir) => {
            export::next_available_name("dbboard-result", "csv", |name| dir.join(name).exists())
        }
        None => "dbboard-result.csv".to_string(),
    };
    let mut dialog = rfd::FileDialog::new()
        .add_filter("CSV", &["csv"])
        .set_file_name(file_name);
    if let Some(dir) = download_dir {
        dialog = dialog.set_directory(dir);
    }
    let Some(path) = dialog.save_file() else {
        return;
    };
    if let Err(e) = std::fs::write(&path, export::to_csv_with_bom(columns, rows)) {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(t!("result-export-error"))
            .set_description(e.to_string())
            .show();
    }
}

/// Progress-bar fraction for a running dump, in `[0.0, 1.0]`.
///
/// Rows are the fine-grained signal, so they drive the bar whenever the
/// plan counted any (`rows_total > 0`). A schema-only dump (every table
/// empty) has no rows to divide, so it falls back to the coarser
/// table-step count. With neither — the pre-first-report snapshot — the bar
/// sits at zero.
//
// A progress bar only needs a few significant figures, so the u64→f32 cast's
// precision loss on very large row counts is immaterial (it shifts the bar by
// sub-pixel amounts); the alternative — exact 64-bit ratio arithmetic — buys
// nothing a viewer can perceive.
#[allow(clippy::cast_precision_loss)]
fn backup_fraction(progress: &DumpProgress) -> f32 {
    let ratio = |done: u64, total: u64| -> f32 {
        if total == 0 {
            0.0
        } else {
            // total > 0, so the clamp only guards the (shouldn't-happen)
            // done > total case.
            (done as f32 / total as f32).clamp(0.0, 1.0)
        }
    };
    if progress.rows_total > 0 {
        ratio(progress.rows_done, progress.rows_total)
    } else {
        ratio(progress.tables_done as u64, progress.tables_total as u64)
    }
}

/// Progress-bar fraction for a running restore, in `[0.0, 1.0]`. Statements are
/// the only signal restore has, so they drive the bar directly; the
/// pre-first-report snapshot (`statements_total == 0`) sits at zero.
#[allow(clippy::cast_precision_loss)]
fn restore_fraction(progress: &RestoreProgress) -> f32 {
    if progress.statements_total == 0 {
        0.0
    } else {
        // total > 0, so the clamp only guards the (shouldn't-happen)
        // done > total case.
        (progress.statements_done as f32 / progress.statements_total as f32).clamp(0.0, 1.0)
    }
}

/// Most sort levels the grid tracks at once — a primary, secondary, and
/// tertiary key. Matches what a user can reasonably reason about and keeps
/// the header indicator (level numbers 1–3) legible.
const MAX_SORT_KEYS: usize = 3;

/// Result-grid sort state: the ordered sort keys plus a cached row
/// permutation. The order is recomputed only when the keys change or the
/// result's row count no longer matches, so a displayed grid isn't re-sorted
/// every frame. Sorting reorders *display*, never the underlying rows — the
/// row indices used for selection and inline editing stay valid.
#[derive(Default)]
struct SortState {
    /// Active sort keys, primary first, capped at [`MAX_SORT_KEYS`].
    keys: Vec<SortKey>,
    /// Cached permutation of the current result's row indices.
    order: Vec<usize>,
    /// The keys changed since `order` was last computed.
    dirty: bool,
}

impl SortState {
    /// Drop all sorting. Called when a fresh result replaces the old one —
    /// the columns (and their meaning) may have changed entirely.
    fn reset(&mut self) {
        self.keys.clear();
        self.order.clear();
        self.dirty = true;
    }

    /// Apply a header click on `column`. A plain click sorts by that column
    /// alone, cycling ascending → descending → off. An `additive` click
    /// (Ctrl / Shift held) instead builds a multi-level sort: a new column is
    /// appended as the next level (up to [`MAX_SORT_KEYS`]), and clicking an
    /// existing level cycles its own direction ascending → descending → gone.
    fn on_header_click(&mut self, column: usize, additive: bool) {
        let existing = self.keys.iter().position(|k| k.column == column);
        if additive {
            match existing {
                Some(i) if self.keys[i].ascending => self.keys[i].ascending = false,
                Some(i) => {
                    self.keys.remove(i);
                }
                None if self.keys.len() < MAX_SORT_KEYS => {
                    self.keys.push(SortKey {
                        column,
                        ascending: true,
                    });
                }
                // At the cap: ignore the extra column rather than silently
                // evicting a level the user set on purpose.
                None => {}
            }
        } else {
            match self.keys.as_slice() {
                [only] if only.column == column && only.ascending => {
                    self.keys[0].ascending = false;
                }
                [only] if only.column == column => self.keys.clear(),
                _ => {
                    self.keys.clear();
                    self.keys.push(SortKey {
                        column,
                        ascending: true,
                    });
                }
            }
        }
        self.dirty = true;
    }

    /// The display order for `rows`, recomputing the cached permutation when
    /// the keys changed or the row count no longer matches (a safety net for
    /// any result swap that didn't call [`Self::reset`]).
    fn order_for(&mut self, rows: &[Row]) -> &[usize] {
        if self.dirty || self.order.len() != rows.len() {
            self.order = sorted_row_order(rows, &self.keys);
            self.dirty = false;
        }
        &self.order
    }

    /// If `column` participates in the sort, its 1-based level and direction
    /// for the header indicator; `None` when the column isn't sorted.
    fn indicator(&self, column: usize) -> Option<(usize, bool)> {
        self.keys
            .iter()
            .position(|k| k.column == column)
            .map(|i| (i + 1, self.keys[i].ascending))
    }
}

fn render_result(
    ui: &mut egui::Ui,
    result: &QueryResult,
    selection: &mut selection::ResultSelection,
    sort: &mut SortState,
    edit: &mut EditGrid,
    busy: bool,
) -> Option<GridIntent> {
    use egui_extras::{Column, TableBuilder};

    if result.rows.is_empty() {
        ui.label(t_args!("result-affected", rows = result.rows_affected));
        return None;
    }

    // Whether this result can be inline-edited (issue 0013 slice b): it
    // needs single-table provenance with a resolved primary key. `busy`
    // (a query or save in flight) freezes cell interaction but still lets
    // the Save row show its progress, so keep the two flags apart.
    let has_identity = matches!(
        (edit.schema.as_ref(), edit.dialect),
        (Some(schema), Some(dialect)) if edit::is_editable(schema, dialect)
    );
    let interactive = has_identity && !busy;

    // Export toolbar (ADR-0035): copy/save the whole grid, plus — once
    // rows are selected — copy/save just the selection.
    render_export_toolbar(ui, result, selection);

    // Row height sized to one line of body text plus a little breathing
    // room; the virtualized body relies on a uniform height.
    let row_height = egui::TextStyle::Body.resolve(ui.style()).size + 8.0;
    let expand_id = expanded_cell_id();

    // Outer horizontal scroll: egui_extras' TableBuilder only scrolls
    // vertically (its internal ScrollArea is hard-coded to `[false, vscroll]`),
    // so a wide result set overflows the panel and the rightmost columns clip
    // at the window edge with no way to reach them. Wrapping the table in a
    // horizontal ScrollArea lets it keep its full content width and pan to the
    // hidden columns, while the table's own vscroll still virtualizes rows.
    // A click lands on at most one row per frame; capture it here and
    // apply it after the table so the selection can't shift mid-iteration
    // (which would make virtualized rows below the click read a stale
    // highlight). ADR-0035 slice 2.
    let mut pending_click: Option<(usize, selection::ClickModifiers)> = None;

    // Precompute the sort display order and per-column indicators before the
    // table closures, which can't borrow `sort` (they also need to *mutate*
    // it on a header click). A header click is captured and applied after the
    // table, the same way the row click above is deferred.
    let order: Vec<usize> = sort.order_for(&result.rows).to_vec();
    let indicators: Vec<Option<(usize, bool)>> = (0..result.columns.len())
        .map(|c| sort.indicator(c))
        .collect();
    let show_levels = sort.keys.len() > 1;
    let mut pending_header_click: Option<(usize, bool)> = None;

    // Pin the Save row to the bottom of the result area *before* laying out
    // the grid, so it stays on screen no matter how tall the grid grows.
    // The grid's ScrollArea claims all remaining height, so a Save row added
    // after it was pushed past the window edge and the user couldn't find it
    // (issue 0013 follow-up). A bottom panel reserves its slice up front.
    let mut intent = None;
    if has_identity && has_pending_save(edit) {
        egui::Panel::bottom("edit-save-row")
            .show_inside(ui, |ui| intent = render_save_row(ui, edit));
    }

    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .auto_shrink([false, false]);
            // Leading selector gutter (ADR-0035 slice 2): a narrow
            // row-number column that is the *only* click target for row
            // selection. Keeping selection off the data cells leaves them
            // free for future in-cell interaction (edit, text-select for a
            // partial copy) without fighting the row picker — and it fixes
            // the sluggish feel of sensing clicks across the whole row,
            // which competed with the cells' own expand affordance.
            table = table.column(Column::auto().at_least(40.0).clip(true));
            // One resizable column per result column. Resizable columns draw
            // the faint vertical separators the striping alone could not, and
            // clipping stops a stray wide value from ballooning the column.
            for _ in &result.columns {
                table = table.column(Column::auto().at_least(48.0).clip(true).resizable(true));
            }

            table
                .header(row_height, |mut header| {
                    pending_header_click =
                        render_sort_header(&mut header, &result.columns, &indicators, show_levels);
                })
                .body(|body| {
                    body.rows(row_height, result.rows.len(), |mut row| {
                        // `display` is the on-screen position; `index` is the
                        // actual row in `result.rows` it maps to under the
                        // current sort. Selection and editing key on `index`
                        // so they stay correct whatever the display order.
                        let display = row.index();
                        let index = order[display];
                        // Highlight the whole row even though only the
                        // gutter is clickable, so the selection reads
                        // across all columns.
                        row.set_selected(selection.is_selected(index));
                        row.col(|ui| {
                            ui.with_layout(
                                egui::Layout::top_down_justified(egui::Align::Center),
                                |ui| {
                                    // Sequential 1-based display position, like
                                    // a spreadsheet row header. The justified
                                    // layout makes the whole gutter cell the
                                    // click target, not just the digits.
                                    let response = ui
                                        .selectable_label(
                                            selection.is_selected(index),
                                            (display + 1).to_string(),
                                        )
                                        .on_hover_text(t!("result-select-row-hint"));
                                    if response.clicked() {
                                        let mods = ui.input(|i| i.modifiers);
                                        pending_click = Some((
                                            index,
                                            selection::ClickModifiers {
                                                ctrl: mods.command,
                                                shift: mods.shift,
                                            },
                                        ));
                                    }
                                },
                            );
                        });
                        for col_idx in 0..result.columns.len() {
                            let value = result.rows[index].get(col_idx);
                            row.col(|ui| {
                                if interactive {
                                    render_editable_cell(ui, edit, index, col_idx, value);
                                } else {
                                    let text = value.map(ToString::to_string).unwrap_or_default();
                                    render_cell(ui, &text, expand_id);
                                }
                            });
                        }
                    });
                });
        });

    if let Some((index, mods)) = pending_click {
        selection.click(index, mods);
    }
    if let Some((column, additive)) = pending_header_click {
        sort.on_header_click(column, additive);
    }

    render_expanded_cell_popup(ui, expand_id);
    intent
}

/// Render the result grid's header row: an empty gutter cell, then one
/// clickable, sort-aware cell per column. Returns the column a click landed on
/// (with whether a modifier was held), for the caller to apply after the table
/// so the sort can't change mid-layout.
fn render_sort_header(
    header: &mut egui_extras::TableRow<'_, '_>,
    columns: &[dbboard_core::Column],
    indicators: &[Option<(usize, bool)>],
    show_levels: bool,
) -> Option<(usize, bool)> {
    let mut click = None;
    // Empty gutter header above the row-number column.
    header.col(|_ui| {});
    for (col_idx, col) in columns.iter().enumerate() {
        header.col(|ui| {
            // Clickable header: sorts by this column. The selected highlight
            // marks an active sort column; the label carries the ▲/▼ arrow and
            // (when multi-level) the sort level number.
            let label = sort_header_label(&col.name, indicators[col_idx], show_levels);
            let response = ui
                .selectable_label(
                    indicators[col_idx].is_some(),
                    egui::RichText::new(label).strong(),
                )
                .on_hover_text(t!("result-sort-hint"));
            if response.clicked() {
                let mods = ui.input(|i| i.modifiers);
                click = Some((col_idx, mods.command || mods.shift));
            }
        });
    }
    click
}

/// Build a column header label with its sort indicator: an ▲/▼ arrow when the
/// column is sorted, plus a 1-based level number when more than one column is
/// sorted, so the primary/secondary/tertiary order stays legible.
fn sort_header_label(name: &str, indicator: Option<(usize, bool)>, show_levels: bool) -> String {
    match indicator {
        None => name.to_string(),
        Some((level, ascending)) => {
            let arrow = if ascending { '▲' } else { '▼' };
            if show_levels {
                format!("{name} {arrow}{level}")
            } else {
                format!("{name} {arrow}")
            }
        }
    }
}

/// Whether the inline-edit Save row has anything to show: pending staged
/// edits, a save in flight, or a lingering error. Gates the bottom panel
/// so an idle grid isn't topped by an empty reserved strip.
fn has_pending_save(edit: &EditGrid) -> bool {
    !edit.staged.is_empty() || edit.save.is_some() || edit.error.is_some()
}

/// Inline-edit Save row below the grid (issue 0013 slice b). Shown only
/// when there are staged edits, a save in flight, or a lingering save
/// error to report; returns the button the user pressed, if any.
fn render_save_row(ui: &mut egui::Ui, edit: &EditGrid) -> Option<GridIntent> {
    if edit.staged.is_empty() && edit.save.is_none() && edit.error.is_none() {
        return None;
    }
    let mut intent = None;
    // Small top margin so the buttons don't touch the panel's divider line;
    // the bottom panel already supplies the separator from the grid.
    ui.add_space(4.0);
    let saving = edit.save.is_some();
    let staged_count = edit.staged.len();
    let can_act = !saving && staged_count > 0;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_act, egui::Button::new(t!("edit-save-button")))
            .clicked()
        {
            intent = Some(GridIntent::Save);
        }
        if ui
            .add_enabled(can_act, egui::Button::new(t!("edit-discard-button")))
            .clicked()
        {
            intent = Some(GridIntent::Discard);
        }
        if saving {
            ui.spinner();
        }
        ui.label(t_args!("edit-staged-count", count = staged_count));
    });
    if let Some(err) = edit.error.as_ref() {
        errors::render_error(ui, Some(err));
    }
    intent
}

/// One editable result-grid cell (issue 0013 slice b). Shows the staged
/// value with a faint dirty tint when the cell has an uncommitted edit,
/// otherwise the original value. Double-click swaps in a single-line
/// editor; losing focus stages the buffer (仮登録). A right-click menu
/// sets SQL NULL or reverts the cell. Blob cells have no text form and
/// stay read-only.
fn render_editable_cell(
    ui: &mut egui::Ui,
    edit: &mut EditGrid,
    row: usize,
    col: usize,
    value: Option<&Value>,
) {
    // Active editor for this exact cell: render the text box instead of a
    // label, and stage on blur.
    if edit
        .active
        .as_ref()
        .is_some_and(|e| e.row == row && e.col == col)
    {
        let editor = edit.active.as_mut().expect("checked just above");
        let resp =
            ui.add(egui::TextEdit::singleline(&mut editor.buffer).desired_width(f32::INFINITY));
        if std::mem::take(&mut editor.just_opened) {
            resp.request_focus();
        }
        // Blur = 仮登録. An emptied box stages empty text, never NULL —
        // NULL is an explicit right-click action so it can't be typed by
        // accident.
        if resp.lost_focus() {
            let buffer = editor.buffer.clone();
            edit.staged
                .insert((row, col), edit::StagedValue::Text(buffer));
            edit.active = None;
        }
        return;
    }

    let staged = edit.staged.get(&(row, col)).cloned();
    let (display, is_staged) = match &staged {
        Some(edit::StagedValue::Null) => ("NULL".to_owned(), true),
        Some(edit::StagedValue::Text(text)) => (text.clone(), true),
        None => (value.map(ToString::to_string).unwrap_or_default(), false),
    };

    // Faint dirty tint keyed off the brand accent so it holds up in both
    // light and dark themes (ADR-0041/ADR-0056) instead of a hard-coded
    // RGB. The accent keeps its full RGB across themes (it is opaque),
    // unlike `selection.bg_fill` whose premultiplied translucent value
    // would read back dimmed.
    if is_staged {
        let accent = crate::theme::accent(ui.visuals().dark_mode);
        let tint = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 48);
        ui.painter().rect_filled(ui.max_rect(), 2.0, tint);
    }

    let shown = if is_long_cell(&display) {
        cell_preview(&display)
    } else {
        display.clone()
    };
    // A blob has no editable text form; render it as a plain, unsensed
    // label so double-click and the NULL/revert menu don't apply.
    let editable = !matches!(value, Some(Value::Blob(_)));
    if !editable {
        ui.label(shown);
        return;
    }

    // Claim the whole cell as the interaction surface, then paint the text
    // ourselves. A text-sized `Label` left empty or short cells with almost
    // nothing to click, so an emptied/NULL value couldn't be re-opened for
    // edit or given the NULL menu — the two follow-up bugs (issue 0013).
    let response = ui.allocate_response(ui.available_size(), egui::Sense::click());
    if !shown.is_empty() {
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        ui.painter().text(
            response.rect.left_center(),
            egui::Align2::LEFT_CENTER,
            &shown,
            font_id,
            ui.visuals().text_color(),
        );
    }
    let response = response.on_hover_text(t!("edit-cell-hint"));
    response.context_menu(|ui| {
        if ui.button(t!("edit-set-null")).clicked() {
            edit.staged.insert((row, col), edit::StagedValue::Null);
            ui.close();
        }
        if is_staged && ui.button(t!("edit-revert-cell")).clicked() {
            edit.staged.remove(&(row, col));
            ui.close();
        }
    });
    if response.double_clicked() {
        let seed = match &staged {
            Some(edit::StagedValue::Text(text)) => text.clone(),
            Some(edit::StagedValue::Null) => String::new(),
            None => value_edit_text(value).unwrap_or_default(),
        };
        edit.active = Some(CellEditor {
            row,
            col,
            buffer: seed,
            just_opened: true,
        });
    }
}

/// Text used to seed the inline editor from a typed value (issue 0013
/// slice b). Returns `None` for a blob, which has no editable text form.
fn value_edit_text(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::Integer(n)) => Some(n.to_string()),
        Some(Value::Real(x)) => Some(x.to_string()),
        Some(Value::Text(s)) => Some(s.clone()),
        Some(Value::Blob(_)) => None,
    }
}

/// One result-grid cell: a plain label for short values, or a truncated
/// preview plus an expand button that parks the full text in egui memory
/// for the popup to pick up.
fn render_cell(ui: &mut egui::Ui, text: &str, expand_id: egui::Id) {
    if !is_long_cell(text) {
        ui.label(text);
        return;
    }
    ui.horizontal(|ui| {
        ui.label(cell_preview(text));
        if ui
            .small_button("⋯")
            .on_hover_text(t!("cell-expand-hint"))
            .clicked()
        {
            ui.data_mut(|d| d.insert_temp(expand_id, text.to_owned()));
        }
    });
}

/// Full-text viewer for a truncated cell. Renders only while a value is
/// parked under `expand_id`; closing the window clears it.
fn render_expanded_cell_popup(ui: &mut egui::Ui, expand_id: egui::Id) {
    let Some(text) = ui.data(|d| d.get_temp::<String>(expand_id)) else {
        return;
    };
    let mut open = true;
    egui::Window::new(t!("cell-full-text-title"))
        .id(expand_id.with("window"))
        .collapsible(false)
        .resizable(true)
        .default_size([520.0, 360.0])
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            if ui.button(t!("cell-copy")).clicked() {
                ui.ctx().copy_text(text.clone());
            }
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text.as_str()).monospace())
                        .selectable(true),
                );
            });
        });
    if !open {
        ui.data_mut(|d| d.remove::<String>(expand_id));
    }
}

#[cfg(test)]
mod tests {
    use super::errors::db_error_display;
    use super::{
        apply_auto_limit, backup_fraction, cell_preview, is_bare_select, is_long_cell,
        quick_count_sql, quick_select_sql, quote_ident, restore_fraction, should_run_from_keys,
        AiProviderSlot, AnnotationsAdmin, BackupState, Command, DbboardApp, HistoryStatus,
        NoteTarget, PersistentHistoryStore, Reply, RestoreState, ResultTab, CELL_PREVIEW_CHARS,
        DEFAULT_CAPACITY,
    };
    use dbboard_core::{
        Column, ColumnInfo, DbError, DumpOutcome, DumpPlan, DumpProgress, QueryResult, Row,
        TableInfo, TablePlan, TableSchema, Value, DEFAULT_BACKUP_WARN_ROWS,
    };
    use std::sync::mpsc;
    use std::sync::{Arc, RwLock};

    /// Header-click cycling and cached-order behaviour of the result-grid
    /// sort. Pure state logic, so it needs no egui context.
    mod sort_state {
        use super::super::{sort_header_label, SortState, MAX_SORT_KEYS};
        use dbboard_core::{Row, SortKey, Value};

        fn rows(vals: &[i64]) -> Vec<Row> {
            vals.iter()
                .map(|v| Row::new(vec![Value::Integer(*v)]))
                .collect()
        }

        fn key(column: usize, ascending: bool) -> SortKey {
            SortKey { column, ascending }
        }

        #[test]
        fn plain_click_cycles_ascending_descending_off() {
            let mut s = SortState::default();
            s.on_header_click(0, false);
            assert_eq!(s.keys, vec![key(0, true)]);
            s.on_header_click(0, false);
            assert_eq!(s.keys, vec![key(0, false)]);
            s.on_header_click(0, false);
            assert!(s.keys.is_empty());
        }

        #[test]
        fn plain_click_on_a_new_column_replaces_the_sort() {
            let mut s = SortState::default();
            s.on_header_click(0, true); // build a two-level sort first
            s.on_header_click(1, true);
            s.on_header_click(2, false); // plain click on a third column
            assert_eq!(s.keys, vec![key(2, true)]);
        }

        #[test]
        fn additive_click_builds_a_multi_level_sort() {
            let mut s = SortState::default();
            s.on_header_click(0, false);
            s.on_header_click(1, true);
            s.on_header_click(2, true);
            assert_eq!(s.keys.len(), 3);
            assert_eq!(s.indicator(0), Some((1, true)));
            assert_eq!(s.indicator(1), Some((2, true)));
            assert_eq!(s.indicator(2), Some((3, true)));
        }

        #[test]
        fn additive_click_cycles_then_drops_an_existing_level() {
            let mut s = SortState::default();
            s.on_header_click(0, false);
            s.on_header_click(1, true); // add level 2 ascending
            s.on_header_click(1, true); // ascending -> descending
            assert_eq!(s.indicator(1), Some((2, false)));
            s.on_header_click(1, true); // descending -> removed
            assert_eq!(s.indicator(1), None);
            assert_eq!(s.keys, vec![key(0, true)]); // primary survives
        }

        #[test]
        fn additive_click_ignores_a_fourth_column_at_the_cap() {
            let mut s = SortState::default();
            s.on_header_click(0, false);
            s.on_header_click(1, true);
            s.on_header_click(2, true);
            s.on_header_click(3, true); // would be a fourth level
            assert_eq!(s.keys.len(), MAX_SORT_KEYS);
            assert_eq!(s.indicator(3), None);
        }

        #[test]
        fn order_for_reflects_the_active_sort() {
            let mut s = SortState::default();
            s.on_header_click(0, false); // ascending
            assert_eq!(s.order_for(&rows(&[3, 1, 2])), &[1, 2, 0]);
        }

        #[test]
        fn order_for_recomputes_when_the_row_count_changes() {
            let mut s = SortState::default();
            s.on_header_click(0, false);
            assert_eq!(s.order_for(&rows(&[2, 1])), &[1, 0]);
            // A shorter/longer result must not reuse the stale permutation.
            assert_eq!(s.order_for(&rows(&[5, 6, 4])), &[2, 0, 1]);
        }

        #[test]
        fn reset_clears_the_keys() {
            let mut s = SortState::default();
            s.on_header_click(0, false);
            let _ = s.order_for(&rows(&[3, 1, 2]));
            s.reset();
            assert!(s.keys.is_empty());
        }

        #[test]
        fn header_label_appends_arrow_and_level_number() {
            assert_eq!(sort_header_label("id", None, false), "id");
            assert_eq!(sort_header_label("id", Some((1, true)), false), "id ▲");
            assert_eq!(sort_header_label("id", Some((1, false)), false), "id ▼");
            // The level number appears only once more than one column sorts.
            assert_eq!(sort_header_label("id", Some((2, true)), true), "id ▲2");
        }
    }

    const FIXED_TS: &str = "2026-06-04T00:00:00.000Z";

    fn stub_clock() -> String {
        FIXED_TS.to_string()
    }

    fn empty_ai_slot() -> AiProviderSlot {
        Arc::new(RwLock::new(None))
    }

    fn build() -> (DbboardApp, mpsc::Receiver<Command>, mpsc::Sender<Reply>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        let history = PersistentHistoryStore::in_memory_only(DEFAULT_CAPACITY);
        let app = DbboardApp::new(
            cmd_tx,
            reply_rx,
            history,
            String::new(),
            stub_clock as super::RfcClock,
            empty_ai_slot(),
            None,
        );
        (app, cmd_rx, reply_tx)
    }

    fn build_with_persistent(
        history: PersistentHistoryStore,
        conn_label: &str,
    ) -> (DbboardApp, mpsc::Receiver<Command>, mpsc::Sender<Reply>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        let app = DbboardApp::new(
            cmd_tx,
            reply_rx,
            history,
            conn_label.to_string(),
            stub_clock as super::RfcClock,
            empty_ai_slot(),
            None,
        );
        (app, cmd_rx, reply_tx)
    }

    #[test]
    fn new_app_bootstraps_a_list_tables_command() {
        let (_app, cmd_rx, _reply_tx) = build();
        let cmd = cmd_rx.try_recv().expect("bootstrap command emitted");
        assert!(matches!(cmd, Command::ListTables));
    }

    #[test]
    fn new_app_starts_idle_with_empty_state() {
        let (app, _cmd_rx, _reply_tx) = build();
        assert!(!app.is_busy());
    }

    #[test]
    fn run_sql_with_empty_input_is_a_noop() {
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.run_sql();
        assert!(!app.is_busy());
        assert!(cmd_rx.try_recv().is_err());
    }

    #[test]
    fn run_sql_sends_query_and_table_refresh_then_marks_busy() {
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.auto_limit = false; // isolate plumbing from the ADR-0030 guard
        app.sql = "SELECT 1".into();
        app.run_sql();

        assert!(app.is_busy());
        let first = cmd_rx.try_recv().expect("Query command emitted");
        let second = cmd_rx.try_recv().expect("ListTables command emitted");
        assert!(matches!(first, Command::Query(sql) if sql == "SELECT 1"));
        assert!(matches!(second, Command::ListTables));
    }

    #[test]
    fn quick_sql_pick_sets_the_editor_and_runs_immediately() {
        // Issue 0012: picking a table right-click starter must both drop the
        // SQL into the editor and execute it in one action, not leave the
        // user to press Run afterwards.
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.auto_limit = false; // isolate from the ADR-0030 guard
        let sql = quick_select_sql(&TableInfo::unqualified("widgets"));

        app.run_quick_sql(sql.clone());

        assert_eq!(
            app.sql, sql,
            "the starter query stays visible in the editor"
        );
        assert!(app.is_busy(), "the pick runs immediately");
        let first = cmd_rx.try_recv().expect("Query command emitted");
        let second = cmd_rx.try_recv().expect("ListTables command emitted");
        assert!(matches!(first, Command::Query(q) if q == sql));
        assert!(matches!(second, Command::ListTables));
    }

    #[test]
    fn quick_sql_pick_is_ignored_while_busy() {
        // Mirrors the Run button (disabled while busy): a starter picked
        // during an in-flight query must not swap the editor text mid-run or
        // dispatch a second query.
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.sql = "SELECT 1".into();
        app.busy = true;

        app.run_quick_sql(quick_count_sql(&TableInfo::unqualified("widgets")));

        assert_eq!(app.sql, "SELECT 1", "editor text is untouched while busy");
        assert!(
            cmd_rx.try_recv().is_err(),
            "no command dispatched while busy"
        );
    }

    #[test]
    fn run_sql_from_structure_tab_switches_to_results() {
        // Running a query while inspecting a table's structure must bring
        // the result forward — otherwise the freshly-run query's output is
        // hidden behind the Structure tab the user was last on (ADR-0031).
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.active_tab = ResultTab::Structure;
        app.sql = "SELECT 1".into();
        app.run_sql();
        assert_eq!(
            app.active_tab,
            ResultTab::Results,
            "a submitted query must switch the lower panel to the Results tab"
        );
    }

    #[test]
    fn run_sql_while_busy_does_not_switch_tab() {
        // The busy guard short-circuits before any state change, so a
        // second run while a query is in flight must not yank the user off
        // the Structure tab they navigated to.
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.sql = "SELECT 1".into();
        app.run_sql(); // marks busy, switches to Results
        app.active_tab = ResultTab::Structure; // user navigates away mid-flight
        app.run_sql(); // no-op: still busy
        assert_eq!(
            app.active_tab,
            ResultTab::Structure,
            "a busy no-op run must not switch tabs"
        );
    }

    #[test]
    fn run_sql_appends_auto_limit_to_a_bare_select() {
        // End-to-end: with the guard on (default), a bare SELECT reaches
        // the worker and history already carrying LIMIT 100.
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        assert!(app.auto_limit, "guard defaults on");
        app.sql = "SELECT * FROM t".into();
        app.run_sql();

        let first = cmd_rx.try_recv().expect("Query command emitted");
        assert!(matches!(first, Command::Query(sql) if sql == "SELECT * FROM t LIMIT 100"));
        let head = app.history().iter().next().unwrap();
        let super::HistoryEntry::Query(q) = head else {
            panic!("expected Query");
        };
        assert_eq!(q.sql, "SELECT * FROM t LIMIT 100");
    }

    #[test]
    fn run_sql_while_busy_is_a_noop() {
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        app.sql = "SELECT 1".into();
        app.run_sql();
        let _ = cmd_rx.try_recv();
        let _ = cmd_rx.try_recv();

        app.run_sql();
        assert!(cmd_rx.try_recv().is_err());
    }

    #[test]
    fn query_reply_clears_busy_flag() {
        let (mut app, _cmd_rx, reply_tx) = build();
        app.sql = "SELECT 1".into();
        app.run_sql();
        assert!(app.is_busy());

        reply_tx
            .send(Reply::QueryResult(Ok(QueryResult {
                columns: vec![Column {
                    name: "x".into(),
                    declared_type: None,
                }],
                rows: vec![Row::new(vec![Value::Integer(1)])],
                rows_affected: 0,
            })))
            .unwrap();
        app.drain_replies();

        assert!(!app.is_busy());
    }

    #[test]
    fn tables_reply_updates_sidebar_state() {
        let (mut app, _cmd_rx, reply_tx) = build();
        reply_tx
            .send(Reply::Tables(Ok(vec![TableInfo::unqualified("users")])))
            .unwrap();
        app.drain_replies();

        match &app.tables {
            Ok(tables) => assert_eq!(tables[0].name, "users"),
            other => panic!("expected Ok with users table, got {other:?}"),
        }
    }

    #[test]
    fn new_app_has_empty_history() {
        let (app, _cmd_rx, _reply_tx) = build();
        assert!(app.history().is_empty());
    }

    #[test]
    fn run_sql_pushes_to_history() {
        let (mut app, _cmd_rx, _reply_tx) = build();
        app.auto_limit = false; // isolate plumbing from the ADR-0030 guard
        app.sql = "SELECT 1".into();
        app.run_sql();

        assert_eq!(app.history().len(), 1);
        let head = app.history().iter().next().unwrap();
        let super::HistoryEntry::Query(q) = head else {
            panic!("expected Query");
        };
        assert_eq!(q.sql, "SELECT 1");
    }

    #[test]
    fn run_sql_empty_input_does_not_push_to_history() {
        let (mut app, _cmd_rx, _reply_tx) = build();
        app.run_sql();
        assert!(app.history().is_empty());
    }

    #[test]
    fn run_sql_consecutive_duplicates_collapse_in_history() {
        // Two Run clicks on identical SQL: the second push happens after
        // the reply clears `busy`, but adjacent-dedup in HistoryStore
        // keeps the list at one entry.
        let (mut app, _cmd_rx, reply_tx) = build();
        app.sql = "SELECT 1".into();
        app.run_sql();

        reply_tx
            .send(Reply::QueryResult(Ok(QueryResult {
                columns: vec![Column {
                    name: "x".into(),
                    declared_type: None,
                }],
                rows: vec![Row::new(vec![Value::Integer(1)])],
                rows_affected: 0,
            })))
            .unwrap();
        app.drain_replies();

        app.run_sql();
        assert_eq!(app.history().len(), 1);
    }

    #[test]
    fn error_display_prefixes_translated_category_to_raw_message() {
        // No init() call in this binary -> the loader stays on its
        // en-fallback initial state. The wire message stays English
        // (ADR-0009 / ADR-0015) and shows up verbatim after the
        // translated prefix.
        let e = DbError::Connection("host unreachable".into());
        let shown = db_error_display(&e);
        let rendered = shown.localized();
        assert!(
            rendered.starts_with("Connection error"),
            "translated prefix missing: {rendered}"
        );
        assert!(
            rendered.ends_with("host unreachable"),
            "raw message missing: {rendered}"
        );
    }

    #[test]
    fn error_display_covers_every_db_error_category() {
        for e in [
            DbError::Connection("c".into()),
            DbError::Query("q".into()),
            DbError::Schema("s".into()),
            DbError::TypeConversion("t".into()),
            DbError::Capability("cap".into()),
        ] {
            let shown = db_error_display(&e);
            let rendered = shown.localized();
            // Each rendered string contains the category prefix word
            // ("error" / "unavailable") and the wire message body.
            assert!(
                rendered.to_ascii_lowercase().contains("error")
                    || rendered.to_ascii_lowercase().contains("unavailable"),
                "no recognisable category in: {rendered}"
            );
            assert!(rendered.ends_with(e.message()));
        }
    }

    #[test]
    fn run_sql_while_busy_does_not_push_to_history() {
        let (mut app, _cmd_rx, _reply_tx) = build();
        app.auto_limit = false; // isolate plumbing from the ADR-0030 guard
        app.sql = "SELECT 1".into();
        app.run_sql();
        assert_eq!(app.history().len(), 1);

        // Still busy (no reply drained); a second Run with a different
        // statement should not pollute history.
        app.sql = "SELECT 2".into();
        app.run_sql();
        assert_eq!(app.history().len(), 1);
        let head = app.history().iter().next().unwrap();
        let super::HistoryEntry::Query(q) = head else {
            panic!("expected Query");
        };
        assert_eq!(q.sql, "SELECT 1");
    }

    // --- ADR-0017 reply-time disk-append path ---

    fn ok_select_one() -> QueryResult {
        QueryResult {
            columns: vec![Column {
                name: "x".into(),
                declared_type: None,
            }],
            rows: vec![Row::new(vec![Value::Integer(1)])],
            rows_affected: 0,
        }
    }

    fn ok_insert_one() -> QueryResult {
        QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 1,
        }
    }

    fn read_history_jsonl(path: &std::path::Path) -> Vec<serde_json::Value> {
        let contents = std::fs::read_to_string(path).expect("history.jsonl readable");
        contents
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("valid JSON line"))
            .collect()
    }

    #[test]
    fn ok_reply_appends_a_row_returning_completion_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let history = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let (mut app, _cmd_rx, reply_tx) = build_with_persistent(history, "prod-pg");

        app.auto_limit = false; // isolate plumbing from the ADR-0030 guard
        app.sql = "SELECT 1".into();
        app.run_sql();
        reply_tx
            .send(Reply::QueryResult(Ok(ok_select_one())))
            .unwrap();
        app.drain_replies();

        let lines = read_history_jsonl(&path);
        assert_eq!(lines.len(), 1);
        let r = &lines[0];
        assert_eq!(r["v"], 2);
        assert_eq!(r["kind"], "query");
        assert_eq!(r["actor"], serde_json::Value::Null);
        assert_eq!(r["conn"], "prod-pg");
        assert_eq!(r["ts"], FIXED_TS);
        assert_eq!(r["sql"], "SELECT 1");
        assert_eq!(r["status"], "ok");
        assert_eq!(r["rows"], 1, "row-returning record carries rows count");
        assert_eq!(
            r["rows_affected"],
            serde_json::Value::Null,
            "row-returning record's rows_affected must be null"
        );
        assert_eq!(r["error"], serde_json::Value::Null);
    }

    #[test]
    fn ok_reply_for_dml_appends_rows_affected_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let history = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let (mut app, _cmd_rx, reply_tx) = build_with_persistent(history, "prod-pg");

        app.sql = "INSERT INTO users VALUES (1)".into();
        app.run_sql();
        reply_tx
            .send(Reply::QueryResult(Ok(ok_insert_one())))
            .unwrap();
        app.drain_replies();

        let lines = read_history_jsonl(&path);
        assert_eq!(lines.len(), 1);
        let r = &lines[0];
        assert_eq!(r["status"], "ok");
        assert_eq!(r["rows"], serde_json::Value::Null);
        assert_eq!(r["rows_affected"], 1);
    }

    #[test]
    fn error_reply_appends_error_record_with_category() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let history = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let (mut app, _cmd_rx, reply_tx) = build_with_persistent(history, "prod-pg");

        app.sql = "SELCT 1".into();
        app.run_sql();
        reply_tx
            .send(Reply::QueryResult(Err(DbError::Query(
                "syntax error at or near 'SELCT'".into(),
            ))))
            .unwrap();
        app.drain_replies();

        let lines = read_history_jsonl(&path);
        assert_eq!(lines.len(), 1);
        let r = &lines[0];
        assert_eq!(r["status"], "error");
        assert_eq!(r["rows"], serde_json::Value::Null);
        assert_eq!(r["rows_affected"], serde_json::Value::Null);
        assert_eq!(r["error"]["category"], "query");
        assert_eq!(r["error"]["message"], "syntax error at or near 'SELCT'");
    }

    #[test]
    fn tables_reply_does_not_append_to_history() {
        // Only QueryResult replies are user-initiated queries; the
        // bootstrap ListTables (and any post-query refresh) must not
        // pollute the on-disk log with synthetic records.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let history = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let (mut app, _cmd_rx, reply_tx) = build_with_persistent(history, "prod-pg");

        reply_tx
            .send(Reply::Tables(Ok(vec![TableInfo::unqualified("users")])))
            .unwrap();
        app.drain_replies();

        assert!(
            !path.exists() || std::fs::read_to_string(&path).unwrap().is_empty(),
            "ListTables reply must not produce a history record"
        );
    }

    #[test]
    fn reply_with_no_in_flight_query_does_not_append_to_history() {
        // Defensive: a stray QueryResult reply with no pending submit
        // (e.g. a duplicated reply, a worker bug) must be a no-op for
        // the disk log rather than producing a record with a synthetic
        // duration.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("history.jsonl");
        let history = PersistentHistoryStore::load_tail(path.clone(), 100).expect("load");
        let (mut app, _cmd_rx, reply_tx) = build_with_persistent(history, "prod-pg");

        reply_tx
            .send(Reply::QueryResult(Ok(ok_select_one())))
            .unwrap();
        app.drain_replies();

        assert!(!path.exists() || std::fs::read_to_string(&path).unwrap().is_empty());
    }

    // --- ADR-0020 in-process connection switching ---

    #[test]
    fn switch_connection_sends_command_over_channel() {
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap ListTables
        app.switch_connection("prod-pg".into());
        let cmd = cmd_rx.try_recv().expect("SwitchConnection command emitted");
        assert!(matches!(cmd, Command::SwitchConnection { id } if id == "prod-pg"));
    }

    #[test]
    fn connection_switched_reply_updates_active_id_and_refreshes_tables() {
        let (mut app, cmd_rx, reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap ListTables
        reply_tx
            .send(Reply::ConnectionSwitched {
                id: "prod-pg".into(),
            })
            .unwrap();
        app.drain_replies();

        assert_eq!(app.active_connection_id(), "prod-pg");
        assert!(app.last_switch_error().is_none());
        // Side-effect: a follow-up ListTables runs so the sidebar
        // reflects the new adapter's schema.
        let cmd = cmd_rx
            .try_recv()
            .expect("post-switch ListTables refresh emitted");
        assert!(matches!(cmd, Command::ListTables));
    }

    #[test]
    fn switch_failed_reply_records_error_without_changing_active_id() {
        let (mut app, _cmd_rx, reply_tx) = build();
        // `build()` leaves conn_label empty; record the value before the
        // failure so we can assert it is untouched.
        let before = app.active_connection_id().to_string();
        reply_tx
            .send(Reply::SwitchFailed {
                id: "prod-pg".into(),
                error: DbError::Connection("host unreachable".into()),
            })
            .unwrap();
        app.drain_replies();

        assert_eq!(
            app.active_connection_id(),
            before,
            "active id must not change on a failed swap"
        );
        let (id, err) = app
            .last_switch_error()
            .expect("failure surfaced via last_switch_error");
        assert_eq!(id, "prod-pg");
        assert_eq!(err.category(), "connection");
    }

    #[test]
    fn successful_switch_clears_a_prior_switch_failure() {
        let (mut app, cmd_rx, reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        reply_tx
            .send(Reply::SwitchFailed {
                id: "prod-pg".into(),
                error: DbError::Connection("first try".into()),
            })
            .unwrap();
        app.drain_replies();
        assert!(app.last_switch_error().is_some());

        reply_tx
            .send(Reply::ConnectionSwitched {
                id: "prod-pg".into(),
            })
            .unwrap();
        app.drain_replies();
        assert!(
            app.last_switch_error().is_none(),
            "successful switch clears the prior failure"
        );
    }

    #[test]
    fn dispatching_a_new_switch_clears_a_prior_switch_failure() {
        // Guards the Connections window auto-close poll: a fresh Connect
        // click must wipe the previous failure so the in-flight switch
        // is not misread as an immediate failure before its reply lands.
        let (mut app, cmd_rx, reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap
        reply_tx
            .send(Reply::SwitchFailed {
                id: "prod-pg".into(),
                error: DbError::Connection("first try".into()),
            })
            .unwrap();
        app.drain_replies();
        assert!(app.last_switch_error().is_some());

        app.switch_connection("store-a".into());
        assert!(
            app.last_switch_error().is_none(),
            "dispatching a new switch clears the stale failure up front"
        );
    }

    #[test]
    fn switch_error_message_surfaces_id_and_wire_error_for_the_ui() {
        let (mut app, _cmd_rx, reply_tx) = build();
        // No attempt yet: nothing to render in the Connections window.
        assert!(app.switch_error_message().is_none());

        reply_tx
            .send(Reply::SwitchFailed {
                id: "store-a".into(),
                error: DbError::Connection("host unreachable".into()),
            })
            .unwrap();
        app.drain_replies();

        // The message the Connections window renders must name the target
        // the user clicked and carry the underlying wire error so a silent
        // failure becomes a diagnosable one. The localized prefix is not
        // asserted (locale-dependent); the id + error always appear.
        let msg = app
            .switch_error_message()
            .expect("failed switch produces a display message");
        assert!(msg.contains("store-a"), "message names target: {msg}");
        assert!(
            msg.contains("host unreachable"),
            "message carries the wire error: {msg}"
        );

        // Cleared once a later switch succeeds, so a stale error never
        // lingers next to the now-active connection.
        reply_tx
            .send(Reply::ConnectionSwitched {
                id: "store-a".into(),
            })
            .unwrap();
        app.drain_replies();
        assert!(app.switch_error_message().is_none());
    }

    // --- ADR-0023 optional AI provider injection ---

    /// Compile-time stub the slice (a) tests use as a stand-in for a
    /// real `Arc<dyn AiProvider>`. Slice (b) replaces these with a real
    /// AI panel + worker round-trip; for slice (a) all we need to assert
    /// is that the field round-trips through the constructor.
    struct StubAiProvider;

    #[async_trait::async_trait]
    impl dbboard_ai::AiProvider for StubAiProvider {
        fn id(&self) -> &'static str {
            "stub"
        }
        fn capabilities(&self) -> dbboard_ai::AiCapabilities {
            dbboard_ai::AiCapabilities::default()
        }
        async fn explain(
            &self,
            _req: &dbboard_ai::ExplainRequest,
        ) -> dbboard_ai::AiResult<dbboard_ai::AiResponse> {
            Err(dbboard_ai::AiError::Cancelled)
        }
        async fn suggest_sql(
            &self,
            _req: &dbboard_ai::SuggestRequest,
        ) -> dbboard_ai::AiResult<dbboard_ai::AiResponse> {
            Err(dbboard_ai::AiError::Cancelled)
        }
    }

    fn build_with_ai_provider(
        provider: std::sync::Arc<dyn dbboard_ai::AiProvider>,
    ) -> (DbboardApp, mpsc::Receiver<Command>, mpsc::Sender<Reply>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        let history = PersistentHistoryStore::in_memory_only(DEFAULT_CAPACITY);
        let slot: AiProviderSlot = Arc::new(RwLock::new(Some(provider)));
        let app = DbboardApp::new(
            cmd_tx,
            reply_rx,
            history,
            String::new(),
            stub_clock as super::RfcClock,
            slot,
            None,
        );
        (app, cmd_rx, reply_tx)
    }

    #[test]
    fn has_ai_provider_is_false_when_none_was_injected() {
        let (app, _cmd_rx, _reply_tx) = build();
        assert!(!app.has_ai_provider());
    }

    #[test]
    fn has_ai_provider_is_true_when_some_was_injected() {
        let provider: std::sync::Arc<dyn dbboard_ai::AiProvider> =
            std::sync::Arc::new(StubAiProvider);
        let (app, _cmd_rx, _reply_tx) = build_with_ai_provider(provider);
        assert!(app.has_ai_provider());
    }

    #[test]
    fn completion_entry_factory_classifies_status_and_rows() {
        use super::build_completion_entry;
        use super::PendingSubmit;
        use std::time::Instant;

        let pending = PendingSubmit {
            started: Instant::now(),
            sql: "SELECT 1".into(),
        };

        let ok = build_completion_entry(
            &Ok(ok_select_one()),
            &pending,
            "prod-pg",
            FIXED_TS.to_string(),
        );
        let super::HistoryEntry::Query(ok) = ok else {
            panic!("expected Query variant");
        };
        assert_eq!(ok.status, HistoryStatus::Ok);
        assert_eq!(ok.rows, Some(1));
        assert_eq!(ok.rows_affected, None);
        assert_eq!(ok.conn, "prod-pg");
        assert_eq!(ok.ts, FIXED_TS);
        assert!(ok.error.is_none());

        let err = build_completion_entry(
            &Err(DbError::Connection("nope".into())),
            &pending,
            "prod-pg",
            FIXED_TS.to_string(),
        );
        let super::HistoryEntry::Query(err) = err else {
            panic!("expected Query variant");
        };
        assert_eq!(err.status, HistoryStatus::Error);
        assert_eq!(err.error.as_ref().unwrap().category, "connection");
    }

    // --- ADR-0027 slice c: AI history write point ---

    use super::{
        ai_error_history_parts, build_ai_cancelled_entry, build_ai_failed_entry, build_ai_ok_entry,
        pending_ai_from_command, stop_reason_wire, AiEntry, AiError, AiIntent, AiStatus,
        HistoryEntry, PendingAiSubmit, StopReason, TableInfo as UiTableInfo,
    };
    use std::time::Instant;

    fn ai_pending(intent: AiIntent, prompt: &str, conn: Option<&str>) -> PendingAiSubmit {
        PendingAiSubmit {
            started: Instant::now(),
            intent,
            prompt: prompt.into(),
            conn: conn.map(str::to_string),
        }
    }

    fn only_ai_entry(app: &DbboardApp) -> AiEntry {
        let mut ai = None;
        for e in app.history().iter() {
            if let HistoryEntry::Ai(entry) = e {
                assert!(ai.is_none(), "expected exactly one AI entry");
                ai = Some(entry.clone());
            }
        }
        ai.expect("history must contain an AI entry")
    }

    #[test]
    fn pending_ai_from_command_maps_ai_explain_to_explain_intent() {
        let cmd = Command::AiExplain {
            sql: "SELECT 1".into(),
            dialect: Some("postgres".into()),
        };
        let p = pending_ai_from_command(&cmd, "prod-pg").expect("some");
        assert_eq!(p.intent, AiIntent::Explain);
        assert_eq!(p.prompt, "SELECT 1");
        assert_eq!(p.conn.as_deref(), Some("prod-pg"));
    }

    #[test]
    fn pending_ai_from_command_maps_ai_suggest_to_suggest_sql_intent() {
        let cmd = Command::AiSuggest {
            prompt: "top 10 users by MRR".into(),
            dialect: None,
            schema: vec![UiTableInfo::unqualified("users")],
            full_schema: None,
        };
        let p = pending_ai_from_command(&cmd, "prod-pg").expect("some");
        assert_eq!(p.intent, AiIntent::SuggestSql);
        assert_eq!(p.prompt, "top 10 users by MRR");
    }

    #[test]
    fn pending_ai_from_command_maps_streaming_variants() {
        let cmd = Command::AiExplainStream {
            sql: "SELECT 1".into(),
            dialect: None,
        };
        let p = pending_ai_from_command(&cmd, "prod-pg").expect("some");
        assert_eq!(p.intent, AiIntent::Explain);
        assert_eq!(p.prompt, "SELECT 1");

        let cmd = Command::AiSuggestStream {
            prompt: "monthly active users".into(),
            dialect: None,
            schema: vec![],
            full_schema: None,
        };
        let p = pending_ai_from_command(&cmd, "prod-pg").expect("some");
        assert_eq!(p.intent, AiIntent::SuggestSql);
        assert_eq!(p.prompt, "monthly active users");
    }

    #[test]
    fn pending_ai_from_command_returns_none_for_cancel_and_non_ai_commands() {
        // Cancel MUST NOT overwrite the pending snapshot belonging to the
        // request it is cancelling.
        assert!(pending_ai_from_command(&Command::CancelAiRequest, "prod-pg").is_none());
        assert!(pending_ai_from_command(&Command::ListTables, "prod-pg").is_none());
        assert!(pending_ai_from_command(&Command::Query("SELECT 1".into()), "prod-pg").is_none());
        assert!(
            pending_ai_from_command(&Command::SwitchConnection { id: "x".into() }, "prod-pg")
                .is_none()
        );
        assert!(
            pending_ai_from_command(&Command::SwitchAiProvider { id: "x".into() }, "prod-pg")
                .is_none()
        );
        // The prefetch precursor is not the AI request itself — the
        // deferred AiSuggest[Stream] snapshots pending_ai instead.
        assert!(
            pending_ai_from_command(&Command::PrefetchSchema { tables: vec![] }, "prod-pg")
                .is_none()
        );
    }

    #[test]
    fn pending_ai_from_command_empty_conn_label_maps_to_none() {
        let cmd = Command::AiExplain {
            sql: "SELECT 1".into(),
            dialect: None,
        };
        let p = pending_ai_from_command(&cmd, "").expect("some");
        assert!(p.conn.is_none());
    }

    #[test]
    fn build_ai_ok_entry_atomic_path_records_no_stop_reason() {
        let p = ai_pending(AiIntent::Explain, "SELECT 1", Some("prod-pg"));
        let e = build_ai_ok_entry(
            p,
            "this query returns one row".into(),
            42,
            7,
            ("anthropic".into(), "claude-sonnet-4-6".into()),
            None,
            FIXED_TS.to_string(),
        );
        assert_eq!(e.status, AiStatus::Ok);
        assert_eq!(e.intent, AiIntent::Explain);
        assert_eq!(e.prompt, "SELECT 1");
        assert_eq!(e.response, "this query returns one row");
        assert_eq!(e.tokens_in, Some(42));
        assert_eq!(e.tokens_out, Some(7));
        assert_eq!(e.provider, "anthropic");
        assert_eq!(e.model, "claude-sonnet-4-6");
        assert_eq!(e.stop_reason, None);
        assert_eq!(e.conn.as_deref(), Some("prod-pg"));
        assert!(e.error.is_none());
    }

    #[test]
    fn build_ai_ok_entry_streaming_path_records_stop_reason() {
        let p = ai_pending(AiIntent::SuggestSql, "prompt", Some("prod-pg"));
        let e = build_ai_ok_entry(
            p,
            "SELECT 1".into(),
            10,
            3,
            ("anthropic".into(), "claude-sonnet-4-6".into()),
            Some("end_turn".into()),
            FIXED_TS.to_string(),
        );
        assert_eq!(e.status, AiStatus::Ok);
        assert_eq!(e.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn build_ai_failed_entry_records_error_category_and_message() {
        let p = ai_pending(AiIntent::Explain, "SELECT 1", None);
        let e = build_ai_failed_entry(
            p,
            &AiError::Quota("monthly cap hit".into()),
            ("anthropic".into(), "claude-sonnet-4-6".into()),
            FIXED_TS.to_string(),
        );
        assert_eq!(e.status, AiStatus::Error);
        assert_eq!(e.response, "");
        assert_eq!(e.tokens_in, None);
        assert_eq!(e.tokens_out, None);
        let err = e.error.expect("error present");
        assert_eq!(err.category, "quota");
        assert_eq!(err.message, "monthly cap hit");
    }

    #[test]
    fn build_ai_cancelled_entry_without_partial_records_empty_response_and_none_tokens() {
        let p = ai_pending(AiIntent::Explain, "SELECT 1", Some("prod-pg"));
        let e = build_ai_cancelled_entry(
            p,
            None,
            ("anthropic".into(), "claude-sonnet-4-6".into()),
            FIXED_TS.to_string(),
        );
        assert_eq!(e.status, AiStatus::Cancelled);
        assert_eq!(e.response, "");
        assert_eq!(e.tokens_in, None);
        assert_eq!(e.tokens_out, None);
        assert_eq!(e.stop_reason, None);
        assert!(e.error.is_none());
    }

    #[test]
    fn build_ai_cancelled_entry_with_partial_records_partial_response_and_tokens() {
        use super::ai::StreamingAcc;

        let p = ai_pending(AiIntent::Explain, "SELECT 1", Some("prod-pg"));
        let acc = StreamingAcc {
            text: "partial answer".into(),
            tokens_in: 15,
            tokens_out: 5,
        };
        let e = build_ai_cancelled_entry(
            p,
            Some(acc),
            ("anthropic".into(), "claude-sonnet-4-6".into()),
            FIXED_TS.to_string(),
        );
        assert_eq!(e.status, AiStatus::Cancelled);
        assert_eq!(e.response, "partial answer");
        assert_eq!(e.tokens_in, Some(15));
        assert_eq!(e.tokens_out, Some(5));
    }

    #[test]
    fn build_ai_cancelled_entry_with_zero_token_partial_omits_tokens() {
        // ADR-0027 §Decision 5: no usage event yet ⇒ tokens `None`.
        use super::ai::StreamingAcc;

        let p = ai_pending(AiIntent::Explain, "SELECT 1", Some("prod-pg"));
        let acc = StreamingAcc {
            text: "hi".into(),
            tokens_in: 0,
            tokens_out: 0,
        };
        let e = build_ai_cancelled_entry(
            p,
            Some(acc),
            ("anthropic".into(), "claude-sonnet-4-6".into()),
            FIXED_TS.to_string(),
        );
        assert_eq!(e.response, "hi");
        assert_eq!(e.tokens_in, None);
        assert_eq!(e.tokens_out, None);
    }

    #[test]
    fn stop_reason_wire_maps_all_known_variants_and_other() {
        assert_eq!(stop_reason_wire(&StopReason::EndTurn), "end_turn");
        assert_eq!(stop_reason_wire(&StopReason::MaxTokens), "max_tokens");
        assert_eq!(stop_reason_wire(&StopReason::StopSequence), "stop_sequence");
        assert_eq!(stop_reason_wire(&StopReason::ToolUse), "tool_use");
        assert_eq!(stop_reason_wire(&StopReason::Refusal), "refusal");
        assert_eq!(
            stop_reason_wire(&StopReason::Other("mystery".into())),
            "mystery"
        );
    }

    #[test]
    fn ai_error_history_parts_maps_all_variants_to_wire_category() {
        assert_eq!(
            ai_error_history_parts(&AiError::Configuration("k".into())),
            ("configuration".into(), "k".into())
        );
        assert_eq!(
            ai_error_history_parts(&AiError::Network("x".into())),
            ("network".into(), "x".into())
        );
        assert_eq!(
            ai_error_history_parts(&AiError::Provider("p".into())),
            ("provider".into(), "p".into())
        );
        assert_eq!(
            ai_error_history_parts(&AiError::Quota("q".into())),
            ("quota".into(), "q".into())
        );
        assert_eq!(
            ai_error_history_parts(&AiError::Cancelled),
            ("cancelled".into(), String::new())
        );
    }

    #[test]
    fn ai_responded_reply_appends_ok_ai_entry_to_history() {
        let (mut app, _cmd_rx, reply_tx) = build();
        app.pending_ai = Some(ai_pending(AiIntent::Explain, "SELECT 1", None));

        reply_tx
            .send(Reply::AiResponded {
                text: "this query returns one row".into(),
                tokens_in: 42,
                tokens_out: 7,
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            })
            .unwrap();
        app.drain_replies();

        let entry = only_ai_entry(&app);
        assert_eq!(entry.status, AiStatus::Ok);
        assert_eq!(entry.intent, AiIntent::Explain);
        assert_eq!(entry.prompt, "SELECT 1");
        assert_eq!(entry.response, "this query returns one row");
        assert_eq!(entry.tokens_in, Some(42));
        assert_eq!(entry.tokens_out, Some(7));
        assert_eq!(entry.provider, "anthropic");
        assert_eq!(entry.model, "claude-sonnet-4-6");
        assert_eq!(entry.stop_reason, None);
        assert_eq!(entry.ts, FIXED_TS);
        assert!(app.pending_ai.is_none());
    }

    #[test]
    fn ai_failed_reply_appends_error_ai_entry_to_history() {
        let (mut app, _cmd_rx, reply_tx) = build();
        app.pending_ai = Some(ai_pending(AiIntent::SuggestSql, "monthly MRR", None));

        reply_tx
            .send(Reply::AiFailed {
                error: AiError::Network("conn reset".into()),
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            })
            .unwrap();
        app.drain_replies();

        let entry = only_ai_entry(&app);
        assert_eq!(entry.status, AiStatus::Error);
        assert_eq!(entry.intent, AiIntent::SuggestSql);
        assert_eq!(entry.response, "");
        assert_eq!(entry.tokens_in, None);
        let err = entry.error.expect("error present");
        assert_eq!(err.category, "network");
        assert_eq!(err.message, "conn reset");
        assert!(app.pending_ai.is_none());
    }

    #[test]
    fn ai_stream_complete_reply_appends_ok_entry_with_stop_reason_and_streamed_body() {
        let (mut app, _cmd_rx, reply_tx) = build();
        // Prime the streaming accumulator so the drain arm can peek the
        // full body before the panel drains it.
        app.ai_panel.on_stream_chunk("SELECT 1;", Some(10), Some(3));
        app.pending_ai = Some(ai_pending(AiIntent::SuggestSql, "prompt", None));

        reply_tx
            .send(Reply::AiStreamComplete {
                tokens_in: 10,
                tokens_out: 3,
                stop_reason: StopReason::EndTurn,
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            })
            .unwrap();
        app.drain_replies();

        let entry = only_ai_entry(&app);
        assert_eq!(entry.status, AiStatus::Ok);
        assert_eq!(entry.response, "SELECT 1;");
        assert_eq!(entry.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(entry.tokens_in, Some(10));
        assert_eq!(entry.tokens_out, Some(3));
    }

    #[test]
    fn ai_cancelled_reply_after_partial_stream_records_partial_body() {
        let (mut app, _cmd_rx, reply_tx) = build();
        app.ai_panel.on_stream_chunk("SELECT ", Some(10), Some(2));
        app.pending_ai = Some(ai_pending(AiIntent::SuggestSql, "prompt", None));

        reply_tx
            .send(Reply::AiCancelled {
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            })
            .unwrap();
        app.drain_replies();

        let entry = only_ai_entry(&app);
        assert_eq!(entry.status, AiStatus::Cancelled);
        assert_eq!(entry.response, "SELECT ");
        assert_eq!(entry.tokens_in, Some(10));
        assert_eq!(entry.tokens_out, Some(2));
        assert!(entry.error.is_none());
    }

    #[test]
    fn ai_cancelled_reply_without_stream_records_empty_body() {
        let (mut app, _cmd_rx, reply_tx) = build();
        // No streaming chunks arrived — atomic-path cancel.
        app.pending_ai = Some(ai_pending(AiIntent::Explain, "SELECT 1", None));

        reply_tx
            .send(Reply::AiCancelled {
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
            })
            .unwrap();
        app.drain_replies();

        let entry = only_ai_entry(&app);
        assert_eq!(entry.status, AiStatus::Cancelled);
        assert_eq!(entry.response, "");
        assert_eq!(entry.tokens_in, None);
        assert_eq!(entry.tokens_out, None);
    }

    #[test]
    fn ai_reply_without_pending_snapshot_does_not_record_history() {
        // Defensive: a stray terminal reply (e.g. from a leftover cancel
        // race) with no `pending_ai` set must be a no-op on the history
        // ring, not a panic.
        let (mut app, _cmd_rx, reply_tx) = build();
        assert!(app.pending_ai.is_none());

        reply_tx
            .send(Reply::AiFailed {
                error: AiError::Cancelled,
                provider: "unknown".into(),
                model: String::new(),
            })
            .unwrap();
        app.drain_replies();

        // No AI entry recorded.
        for e in app.history().iter() {
            assert!(!matches!(e, HistoryEntry::Ai(_)));
        }
    }

    // --- Run keyboard shortcuts (F5 / Ctrl+Enter) ---

    #[test]
    fn f5_triggers_a_run() {
        assert!(should_run_from_keys(true, false, false));
    }

    #[test]
    fn command_modifier_plus_enter_triggers_a_run() {
        assert!(should_run_from_keys(false, true, true));
    }

    #[test]
    fn bare_enter_does_not_trigger_a_run() {
        // A newline in the multiline editor must not submit.
        assert!(!should_run_from_keys(false, false, true));
    }

    #[test]
    fn command_modifier_alone_does_not_trigger_a_run() {
        assert!(!should_run_from_keys(false, true, false));
    }

    #[test]
    fn short_single_line_cell_is_not_long() {
        assert!(!is_long_cell("hello"));
        assert!(!is_long_cell(""));
    }

    #[test]
    fn multiline_cell_is_long_even_when_short() {
        assert!(is_long_cell("a\nb"));
    }

    #[test]
    fn overlong_single_line_cell_is_long() {
        let text = "x".repeat(CELL_PREVIEW_CHARS + 1);
        assert!(is_long_cell(&text));
    }

    #[test]
    fn preview_leaves_short_values_untouched() {
        assert_eq!(cell_preview("hello"), "hello");
    }

    #[test]
    fn preview_collapses_newlines_to_spaces() {
        // A short multi-line value still gets an ellipsis because it hides
        // its line structure once flattened.
        assert_eq!(cell_preview("a\nb"), "a b…");
    }

    #[test]
    fn preview_caps_length_and_marks_elision() {
        let text = "y".repeat(CELL_PREVIEW_CHARS + 20);
        let preview = cell_preview(&text);
        assert_eq!(preview.chars().count(), CELL_PREVIEW_CHARS + 1); // + ellipsis
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn bare_select_is_detected_case_insensitively() {
        assert!(is_bare_select("select * from t"));
        assert!(is_bare_select("  SELECT a, b FROM t  "));
        assert!(is_bare_select("select * from t;"));
        assert!(is_bare_select("SELECT\n  *\nFROM t"));
    }

    #[test]
    fn non_select_and_limited_select_are_not_bare() {
        assert!(!is_bare_select("select * from t limit 5"));
        assert!(!is_bare_select("SELECT * FROM t LIMIT 10;"));
        assert!(!is_bare_select("update t set a = 1"));
        assert!(!is_bare_select("with x as (select 1) select * from x"));
        // Multi-statement input must be left alone.
        assert!(!is_bare_select("select 1; select 2"));
    }

    #[test]
    fn limit_substring_in_identifier_does_not_count_as_a_limit_clause() {
        // A column literally named `limits` must not suppress the guard.
        assert!(is_bare_select("select limits from t"));
    }

    #[test]
    fn apply_auto_limit_appends_only_to_bare_selects() {
        assert_eq!(
            apply_auto_limit("select * from t", true, 100),
            "select * from t LIMIT 100"
        );
        // Trailing semicolon is dropped so the result stays valid.
        assert_eq!(
            apply_auto_limit("select * from t;", true, 100),
            "select * from t LIMIT 100"
        );
    }

    #[test]
    fn apply_auto_limit_is_a_noop_when_disabled_or_not_needed() {
        assert_eq!(
            apply_auto_limit("select * from t", false, 100),
            "select * from t"
        );
        assert_eq!(
            apply_auto_limit("select * from t limit 5", true, 100),
            "select * from t limit 5"
        );
        assert_eq!(
            apply_auto_limit("update t set a = 1", true, 100),
            "update t set a = 1"
        );
    }

    // --- ADR-0031 structure tab ------------------------------------------

    fn one_column_schema(table: &str) -> TableSchema {
        TableSchema {
            table: TableInfo::unqualified(table),
            columns: vec![ColumnInfo {
                name: "id".into(),
                declared_type: Some("INTEGER".into()),
                nullable: false,
                primary_key: true,
                ordinal: 1,
                default_value: None,
            }],
            primary_key: vec!["id".into()],
        }
    }

    #[test]
    fn clicking_a_table_opens_structure_and_requests_describe() {
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv(); // drain bootstrap ListTables
        app.open_structure(TableInfo::unqualified("stores"));

        assert_eq!(app.active_tab, ResultTab::Structure);
        let cmd = cmd_rx.try_recv().expect("DescribeTable emitted");
        assert!(matches!(cmd, Command::DescribeTable { table } if table.name == "stores"));
        // Schema stays in flight until the reply lands.
        assert!(app.structure.as_ref().unwrap().schema.is_none());
    }

    #[test]
    fn table_described_reply_populates_the_matching_structure_view() {
        let (mut app, cmd_rx, reply_tx) = build();
        let _ = cmd_rx.try_recv();
        app.open_structure(TableInfo::unqualified("stores"));
        let _ = cmd_rx.try_recv();

        reply_tx
            .send(Reply::TableDescribed {
                table: TableInfo::unqualified("stores"),
                result: Ok(one_column_schema("stores")),
            })
            .unwrap();
        app.drain_replies();

        let view = app.structure.as_ref().unwrap();
        let schema = view.schema.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(schema.columns[0].name, "id");
    }

    #[test]
    fn stale_table_described_reply_is_ignored() {
        let (mut app, cmd_rx, reply_tx) = build();
        let _ = cmd_rx.try_recv();
        app.open_structure(TableInfo::unqualified("stores"));
        let _ = cmd_rx.try_recv();

        // A describe for a table the user already navigated away from must
        // not overwrite the in-flight view.
        reply_tx
            .send(Reply::TableDescribed {
                table: TableInfo::unqualified("orders"),
                result: Ok(one_column_schema("orders")),
            })
            .unwrap();
        app.drain_replies();

        assert!(app.structure.as_ref().unwrap().schema.is_none());
    }

    // --- ADR-0045: local table/column notes on the Structure tab ---

    #[test]
    fn open_structure_drops_stale_note_buffers() {
        // A half-typed note on one table must not bleed onto the next: the
        // buffers live on the view and reset when a new table opens.
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv();
        app.open_structure(TableInfo::unqualified("stores"));
        let view = app.structure.as_mut().unwrap();
        view.note_buffers.insert("id".into(), "in progress".into());
        view.table_note_buffer = Some("table memo".into());

        app.open_structure(TableInfo::unqualified("orders"));

        let view = app.structure.as_ref().unwrap();
        assert!(view.note_buffers.is_empty());
        assert!(view.table_note_buffer.is_none());
    }

    #[test]
    fn commit_structure_note_persists_column_keyed_by_conn_and_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("annotations.toml");
        let admin = AnnotationsAdmin::new_with_file(path.clone()).unwrap();
        let (mut app, cmd_rx, _reply_tx) = build_with_persistent(
            PersistentHistoryStore::in_memory_only(DEFAULT_CAPACITY),
            "store-a",
        );
        app.annotations = Some(admin);
        let _ = cmd_rx.try_recv();
        app.open_structure(TableInfo::unqualified("stores"));

        app.commit_structure_note(&NoteTarget::Column("id".into()), "  primary id  ");
        app.commit_structure_note(&NoteTarget::Table, "the stores table");

        // Reopen from disk: the note is stored under conn `store-a`, table
        // key `stores`, column `id`, and trimmed on the way in.
        let reopened = AnnotationsAdmin::new_with_file(path).unwrap();
        assert_eq!(
            reopened.column_note("store-a", "stores", "id"),
            Some("primary id")
        );
        assert_eq!(
            reopened.table_note("store-a", "stores"),
            Some("the stores table")
        );
    }

    #[test]
    fn commit_structure_note_is_a_noop_without_a_store() {
        // No annotations store wired (the tests / in-memory posture): the
        // commit must simply do nothing rather than panic.
        let (mut app, cmd_rx, _reply_tx) = build();
        let _ = cmd_rx.try_recv();
        app.open_structure(TableInfo::unqualified("stores"));
        app.commit_structure_note(&NoteTarget::Column("id".into()), "note");
        assert!(app.annotations.is_none());
    }

    #[test]
    fn quote_ident_wraps_in_double_quotes() {
        // Double-quoted identifiers are SQL-standard and accepted by every
        // backend dbboard targets (Postgres wire + SQLite/libSQL).
        assert_eq!(quote_ident("users"), "\"users\"");
    }

    #[test]
    fn quote_ident_doubles_embedded_quotes() {
        // A `"` inside an identifier must be escaped by doubling, or a
        // maliciously/awkwardly named table could break out of the quotes.
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn quick_select_sql_for_unqualified_table() {
        let sql = quick_select_sql(&TableInfo::unqualified("orders"));
        assert_eq!(sql, "SELECT * FROM \"orders\";");
    }

    #[test]
    fn quick_select_sql_qualifies_schema_when_present() {
        // Postgres-family tables carry a schema; both parts are quoted and
        // dot-joined so `public.orders` becomes `"public"."orders"`.
        let sql = quick_select_sql(&TableInfo::qualified("public", "orders"));
        assert_eq!(sql, "SELECT * FROM \"public\".\"orders\";");
    }

    #[test]
    fn quick_count_sql_counts_rows() {
        let sql = quick_count_sql(&TableInfo::unqualified("orders"));
        assert_eq!(sql, "SELECT COUNT(*) FROM \"orders\";");
    }

    // ADR-0049 slice e: the backup state machine. These exercise the pure
    // transitions `drain_replies` drives — no egui, no file dialog — so the
    // warn/confirm/progress/terminal flow is verified without a live adapter.
    mod backup {
        use super::{
            backup_fraction, build, BackupState, Command, DumpOutcome, DumpPlan, DumpProgress,
            Reply, TableInfo, TablePlan, DEFAULT_BACKUP_WARN_ROWS,
        };

        fn plan_of(rows: u64) -> DumpPlan {
            DumpPlan::new(vec![TablePlan::new(TableInfo::unqualified("t"), rows)])
        }

        #[test]
        fn planned_under_threshold_goes_straight_to_save() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Planning;
            reply_tx
                .send(Reply::BackupPlanned {
                    result: Ok(plan_of(10)),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.backup, BackupState::ReadyToSave(_)));
        }

        #[test]
        fn planned_over_threshold_asks_for_confirmation_with_the_total() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Planning;
            let rows = DEFAULT_BACKUP_WARN_ROWS + 1;
            reply_tx
                .send(Reply::BackupPlanned {
                    result: Ok(plan_of(rows)),
                })
                .unwrap();
            app.drain_replies();
            match app.backup {
                BackupState::Confirming { total_rows, .. } => assert_eq!(total_rows, rows),
                other => panic!("expected Confirming, got {other:?}"),
            }
        }

        #[test]
        fn a_lowered_threshold_warns_where_the_default_would_not() {
            // ADR-0050: the preflight compares against the runtime threshold,
            // not the constant. A plan that sits under the 500k default must
            // warn once the user lowers the threshold below it.
            let (mut app, _cmd_rx, reply_tx) = build();
            app.set_backup_warn_rows(100);
            app.backup = BackupState::Planning;
            reply_tx
                .send(Reply::BackupPlanned {
                    result: Ok(plan_of(500)),
                })
                .unwrap();
            app.drain_replies();
            match app.backup {
                BackupState::Confirming { total_rows, .. } => assert_eq!(total_rows, 500),
                other => panic!("expected Confirming, got {other:?}"),
            }
        }

        #[test]
        fn a_raised_threshold_skips_the_warning_the_default_would_show() {
            // The mirror of the above: raising the threshold above a plan the
            // 500k default would have flagged skips the warning entirely.
            let (mut app, _cmd_rx, reply_tx) = build();
            app.set_backup_warn_rows(DEFAULT_BACKUP_WARN_ROWS * 10);
            app.backup = BackupState::Planning;
            reply_tx
                .send(Reply::BackupPlanned {
                    result: Ok(plan_of(DEFAULT_BACKUP_WARN_ROWS + 1)),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.backup, BackupState::ReadyToSave(_)));
        }

        #[test]
        fn the_default_threshold_is_the_core_constant() {
            // A freshly built app uses the domain default until the binary
            // pushes a persisted override.
            let (app, _cmd_rx, _reply_tx) = build();
            assert_eq!(app.backup_warn_rows(), DEFAULT_BACKUP_WARN_ROWS);
        }

        #[test]
        fn planned_error_surfaces_as_failed() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Planning;
            reply_tx
                .send(Reply::BackupPlanned {
                    result: Err(dbboard_core::DbError::Capability("nope".into())),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.backup, BackupState::Failed(_)));
        }

        #[test]
        fn a_stale_plan_after_dismissal_is_ignored() {
            // The user dismissed the flow (state back to Idle) before the
            // preflight reply arrived; it must not resurrect a modal.
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Idle;
            reply_tx
                .send(Reply::BackupPlanned {
                    result: Ok(plan_of(10)),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.backup, BackupState::Idle));
        }

        #[test]
        fn progress_updates_only_while_running() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Running(DumpProgress::default());
            reply_tx
                .send(Reply::BackupProgress {
                    progress: DumpProgress {
                        rows_done: 7,
                        rows_total: 10,
                        ..DumpProgress::default()
                    },
                })
                .unwrap();
            app.drain_replies();
            match &app.backup {
                BackupState::Running(p) => assert_eq!(p.rows_done, 7),
                other => panic!("expected Running, got {other:?}"),
            }
        }

        #[test]
        fn a_late_progress_tick_after_completion_is_dropped() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Done(DumpOutcome::default());
            reply_tx
                .send(Reply::BackupProgress {
                    progress: DumpProgress {
                        rows_done: 3,
                        ..DumpProgress::default()
                    },
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.backup, BackupState::Done(_)));
        }

        #[test]
        fn complete_and_failed_land_on_terminal_states() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.backup = BackupState::Running(DumpProgress::default());
            reply_tx
                .send(Reply::BackupComplete {
                    outcome: DumpOutcome::default(),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.backup, BackupState::Done(_)));

            reply_tx
                .send(Reply::BackupFailed {
                    message: "disk full".into(),
                })
                .unwrap();
            app.drain_replies();
            match &app.backup {
                BackupState::Failed(m) => assert_eq!(m, "disk full"),
                other => panic!("expected Failed, got {other:?}"),
            }
        }

        #[test]
        fn start_backup_without_a_dumpable_connection_is_a_noop() {
            // build() wires no schema source, so can_backup() is false and no
            // PlanBackup should be emitted (only the bootstrap ListTables).
            let (mut app, cmd_rx, _reply_tx) = build();
            let _ = cmd_rx.try_recv(); // drain bootstrap ListTables
            app.start_backup();
            assert!(matches!(app.backup, BackupState::Idle));
            assert!(cmd_rx.try_recv().is_err());
        }

        #[test]
        fn cancel_backup_signals_the_worker() {
            let (mut app, cmd_rx, _reply_tx) = build();
            let _ = cmd_rx.try_recv(); // drain bootstrap
            app.cancel_backup();
            assert!(matches!(cmd_rx.try_recv(), Ok(Command::CancelBackup)));
        }

        #[test]
        fn fraction_prefers_rows_then_tables_then_zero() {
            // Rows drive the bar when counted.
            assert!(
                (backup_fraction(&DumpProgress {
                    rows_done: 1,
                    rows_total: 4,
                    ..DumpProgress::default()
                }) - 0.25)
                    .abs()
                    < f32::EPSILON
            );
            // No rows (schema-only): fall back to table steps.
            assert!(
                (backup_fraction(&DumpProgress {
                    tables_done: 1,
                    tables_total: 2,
                    ..DumpProgress::default()
                }) - 0.5)
                    .abs()
                    < f32::EPSILON
            );
            // Nothing counted yet: zero.
            assert!(backup_fraction(&DumpProgress::default()).abs() < f32::EPSILON);
        }
    }

    // ADR-0051 slice 6: the restore state machine. Like the backup tests these
    // exercise the pure transitions `drain_replies` drives — no egui, no file
    // dialog — so the plan/confirm/progress/terminal flow is verified without a
    // live adapter.
    mod restore {
        use super::{build, restore_fraction, Command, Reply, RestoreState};
        use dbboard_core::{
            OnError, RestoreOutcome, RestorePlan, RestoreProgress, RestoreStatement,
            StatementFailure, StatementKind,
        };

        /// A plan whose target holds `existing` tables and whose script is
        /// `statements` runnable `INSERT`s. Enough to drive the empty/non-empty
        /// branch and the progress-bar denominator.
        fn plan_of(existing: &[&str], statements: usize) -> RestorePlan {
            RestorePlan {
                statements: (0..statements)
                    .map(|i| RestoreStatement {
                        sql: format!("INSERT INTO t VALUES ({i});"),
                        kind: StatementKind::Data,
                    })
                    .collect(),
                existing_tables: existing.iter().map(|s| (*s).to_string()).collect(),
            }
        }

        #[test]
        fn planned_empty_target_runs_and_emits_start() {
            let (mut app, cmd_rx, reply_tx) = build();
            let _ = cmd_rx.try_recv(); // drain bootstrap ListTables
            app.restore = RestoreState::Planning;
            reply_tx
                .send(Reply::RestorePlanned {
                    result: Ok(plan_of(&[], 3)),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.restore, RestoreState::Running(_)));
            match cmd_rx.try_recv() {
                Ok(Command::StartRestore { options, .. }) => {
                    // An empty target needs no confirmation.
                    assert!(!options.confirmed);
                    assert_eq!(options.on_error, OnError::Stop);
                }
                other => panic!("expected StartRestore, got {other:?}"),
            }
        }

        #[test]
        fn planned_non_empty_target_asks_for_confirmation() {
            let (mut app, cmd_rx, reply_tx) = build();
            let _ = cmd_rx.try_recv();
            app.restore = RestoreState::Planning;
            reply_tx
                .send(Reply::RestorePlanned {
                    result: Ok(plan_of(&["users", "orders"], 2)),
                })
                .unwrap();
            app.drain_replies();
            match &app.restore {
                RestoreState::Confirming { plan } => {
                    assert_eq!(plan.existing_tables.len(), 2);
                }
                other => panic!("expected Confirming, got {other:?}"),
            }
            // No StartRestore until the user confirms.
            assert!(cmd_rx.try_recv().is_err());
        }

        #[test]
        fn planned_error_surfaces_as_failed() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.restore = RestoreState::Planning;
            reply_tx
                .send(Reply::RestorePlanned {
                    result: Err(dbboard_core::DbError::Query("could not read x.sql".into())),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.restore, RestoreState::Failed(_)));
        }

        #[test]
        fn a_stale_plan_after_dismissal_is_ignored() {
            // The user dismissed the flow (state back to Idle) before the
            // preflight reply arrived; it must not resurrect a modal or run.
            let (mut app, cmd_rx, reply_tx) = build();
            let _ = cmd_rx.try_recv();
            app.restore = RestoreState::Idle;
            reply_tx
                .send(Reply::RestorePlanned {
                    result: Ok(plan_of(&[], 1)),
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.restore, RestoreState::Idle));
            assert!(cmd_rx.try_recv().is_err());
        }

        #[test]
        fn progress_updates_only_while_running() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.restore = RestoreState::Running(RestoreProgress::default());
            reply_tx
                .send(Reply::RestoreProgress {
                    progress: RestoreProgress {
                        statements_done: 7,
                        statements_total: 10,
                        current_index: Some(7),
                    },
                })
                .unwrap();
            app.drain_replies();
            match &app.restore {
                RestoreState::Running(p) => assert_eq!(p.statements_done, 7),
                other => panic!("expected Running, got {other:?}"),
            }
        }

        #[test]
        fn a_late_progress_tick_after_completion_is_dropped() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.restore = RestoreState::Done(RestoreOutcome::default());
            reply_tx
                .send(Reply::RestoreProgress {
                    progress: RestoreProgress {
                        statements_done: 3,
                        ..RestoreProgress::default()
                    },
                })
                .unwrap();
            app.drain_replies();
            assert!(matches!(app.restore, RestoreState::Done(_)));
        }

        #[test]
        fn complete_and_failed_land_on_terminal_states() {
            let (mut app, _cmd_rx, reply_tx) = build();
            app.restore = RestoreState::Running(RestoreProgress::default());
            reply_tx
                .send(Reply::RestoreComplete {
                    outcome: RestoreOutcome {
                        statements_run: 4,
                        failures: vec![StatementFailure {
                            index: 2,
                            message: "boom".into(),
                        }],
                        ..RestoreOutcome::default()
                    },
                })
                .unwrap();
            app.drain_replies();
            match &app.restore {
                RestoreState::Done(o) => assert_eq!(o.statements_run, 4),
                other => panic!("expected Done, got {other:?}"),
            }

            app.restore = RestoreState::Running(RestoreProgress::default());
            reply_tx
                .send(Reply::RestoreFailed {
                    message: "target not empty".into(),
                })
                .unwrap();
            app.drain_replies();
            match &app.restore {
                RestoreState::Failed(m) => assert_eq!(m, "target not empty"),
                other => panic!("expected Failed, got {other:?}"),
            }
        }

        #[test]
        fn start_restore_without_a_restorable_connection_is_a_noop() {
            // build() wires no schema source, so can_restore() is false and no
            // PlanRestore should be emitted (only the bootstrap ListTables).
            let (mut app, cmd_rx, _reply_tx) = build();
            let _ = cmd_rx.try_recv(); // drain bootstrap ListTables
            app.start_restore();
            assert!(matches!(app.restore, RestoreState::Idle));
            assert!(cmd_rx.try_recv().is_err());
        }

        #[test]
        fn cancel_restore_signals_the_worker() {
            let (mut app, cmd_rx, _reply_tx) = build();
            let _ = cmd_rx.try_recv(); // drain bootstrap
            app.cancel_restore();
            assert!(matches!(cmd_rx.try_recv(), Ok(Command::CancelRestore)));
        }

        #[test]
        fn launch_restore_with_a_closed_channel_fails_cleanly() {
            // A closed command channel (UI shutting down) surfaces a failure
            // rather than a phantom Running with no worker behind it.
            let (mut app, cmd_rx, _reply_tx) = build();
            drop(cmd_rx);
            app.launch_restore(plan_of(&[], 1), true);
            assert!(matches!(app.restore, RestoreState::Failed(_)));
        }

        #[test]
        fn fraction_is_statements_done_over_total_then_zero() {
            assert!(
                (restore_fraction(&RestoreProgress {
                    statements_done: 1,
                    statements_total: 4,
                    current_index: Some(1),
                }) - 0.25)
                    .abs()
                    < f32::EPSILON
            );
            // Nothing counted yet: zero.
            assert!(restore_fraction(&RestoreProgress::default()).abs() < f32::EPSILON);
        }
    }
}
