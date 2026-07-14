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
mod client;
mod connections;
mod export;
mod history;
mod selection;
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

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, PoisonError};
use std::time::Instant;

use dbboard_core::{DbResult, QueryResult, TableInfo, TableSchema};
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
}

pub struct DbboardApp {
    sql: String,
    tables: DbResult<Vec<TableInfo>>,
    last_result: Option<DbResult<QueryResult>>,
    /// Which result-grid rows are selected (ADR-0035 slice 2). Reset
    /// whenever a new result replaces [`Self::last_result`] — the old
    /// indices no longer point at the same rows.
    result_selection: selection::ResultSelection,
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
    /// Case-insensitive substring filter for the sidebar table list
    /// (friction 2026-07-14). Empty string = show every table. The list
    /// is always rendered alphabetically regardless of adapter order.
    table_filter: String,
    /// Index (newest-first, matching the panel's render order) of the
    /// history entry awaiting delete confirmation (friction 2026-07-14).
    /// `Some` while the confirm dialog is open; cleared on confirm/cancel.
    pending_history_delete: Option<usize>,
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
            table_filter: String::new(),
            pending_history_delete: None,
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
        });
    }

    fn drain_replies(&mut self) {
        while let Ok(reply) = self.reply_rx.try_recv() {
            match reply {
                Reply::Tables(r) => self.tables = r,
                Reply::QueryResult(r) => {
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
                    // over from the previous one (ADR-0035 slice 2).
                    self.result_selection.clear();
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
                    if let Some(view) = self.structure.as_mut() {
                        if view.table == table {
                            view.schema = Some(result);
                        }
                    }
                }
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
        // Running a query surfaces its output in the Results tab. If the
        // user was reading a table's Structure, snap back so the result
        // they just triggered is actually on screen (friction 2026-07-14).
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
        | Command::DescribeTable { .. } => return None,
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

        // Egui is event-driven, so request a follow-up frame while a
        // query is in flight to keep draining the reply channel.
        if self.busy {
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
        egui::Panel::left("tables").show_inside(ui, |ui| {
            ui.heading(t!("tables-heading"));
            ui.separator();
            match &self.tables {
                Ok(tables) if tables.is_empty() => {
                    ui.label(t!("tables-empty"));
                }
                Ok(tables) => {
                    // Substring filter box for projects with many tables
                    // (friction 2026-07-14). It stays pinned above the list
                    // while the results below scroll.
                    ui.add(
                        egui::TextEdit::singleline(&mut self.table_filter)
                            .hint_text(t!("tables-filter-hint"))
                            .desired_width(f32::INFINITY),
                    );
                    // Always alphabetical (case-insensitive), independent of
                    // whatever order the adapter's list_tables() returned.
                    let visible = filter_and_sort_tables(tables, &self.table_filter);
                    if visible.is_empty() {
                        ui.label(t!("tables-filter-no-match"));
                    } else {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            // Justified top-down layout stretches each row to
                            // the panel's full width so the whole row is the
                            // click target, not just the text glyphs. Text
                            // stays left-aligned.
                            ui.with_layout(
                                egui::Layout::top_down_justified(egui::Align::LEFT),
                                |ui| {
                                    for table in visible {
                                        let selected = active.as_ref() == Some(table);
                                        if ui.selectable_label(selected, &table.name).clicked() {
                                            clicked = Some(table.clone());
                                        }
                                    }
                                },
                            );
                        });
                    }
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error_display(e));
                }
            }
        });
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

            self.render_history_panel(ui);

            ui.separator();
            // ADR-0031: tab between the query result and the clicked
            // table's structure.
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, ResultTab::Results, t!("tab-results"));
                ui.selectable_value(
                    &mut self.active_tab,
                    ResultTab::Structure,
                    t!("tab-structure"),
                );
            });
            match self.active_tab {
                ResultTab::Results => match &self.last_result {
                    None => {
                        ui.label(t!("result-empty"));
                    }
                    Some(Ok(result)) => {
                        render_result(ui, result, &mut self.result_selection);
                    }
                    Some(Err(e)) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, error_display(e));
                    }
                },
                ResultTab::Structure => self.render_structure(ui),
            }
        });
    }

    /// Recently-run statements panel (ADR-0014): click an entry to refill
    /// the editor, or its × to delete it (friction 2026-07-14). `restore`
    /// and `delete_request` are captured inside the immutable `iter()`
    /// borrow and applied after it ends, sidestepping the borrow checker
    /// without cloning the whole store.
    fn render_history_panel(&mut self, ui: &mut egui::Ui) {
        let mut restore: Option<String> = None;
        let mut delete_request: Option<usize> = None;
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
                                // Only query entries surface in the legacy panel;
                                // the AI record viewer lands separately. The
                                // enumerate index matches the ring's newest-first
                                // order so the × maps to the right entry.
                                for (index, entry) in history.iter().enumerate() {
                                    let HistoryEntry::Query(q) = entry else {
                                        continue;
                                    };
                                    ui.horizontal(|ui| {
                                        if ui
                                            .small_button("×")
                                            .on_hover_text(t!("history-delete-hover"))
                                            .clicked()
                                        {
                                            delete_request = Some(index);
                                        }
                                        if ui.small_button(history_button_label(&q.sql)).clicked() {
                                            restore = Some(q.sql.clone());
                                        }
                                    });
                                }
                            });
                    }
                });
        }
        if let Some(sql) = restore {
            self.sql = sql;
        }
        if let Some(index) = delete_request {
            self.pending_history_delete = Some(index);
        }
        self.render_history_delete_confirm(ui);
    }

    /// Modal confirming a history-entry delete (friction 2026-07-14). No-op
    /// unless [`Self::pending_history_delete`] is set. Confirming removes the
    /// entry from the in-memory view only (the append-only log is preserved);
    /// cancelling or closing the window clears the pending index.
    fn render_history_delete_confirm(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.pending_history_delete else {
            return;
        };
        let mut open = true;
        let mut confirmed = false;
        egui::Window::new(t!("history-delete-title"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label(t!("history-delete-confirm"));
                ui.horizontal(|ui| {
                    if ui.button(t!("history-delete-yes")).clicked() {
                        confirmed = true;
                    }
                    if ui.button(t!("history-delete-no")).clicked() {
                        self.pending_history_delete = None;
                    }
                });
            });
        if confirmed {
            self.history.remove_from_view(index);
            self.pending_history_delete = None;
        } else if !open {
            // Window close (×/Esc) is an implicit cancel.
            self.pending_history_delete = None;
        }
    }

    /// Structure tab body (ADR-0031): the selected table's name and its
    /// `describe_table` outcome rendered as a column grid.
    fn render_structure(&self, ui: &mut egui::Ui) {
        let Some(view) = &self.structure else {
            ui.label(t!("structure-empty"));
            return;
        };
        ui.strong(&view.table.name);
        ui.separator();
        match &view.schema {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t!("structure-loading"));
                });
            }
            Some(Err(e)) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error_display(e));
            }
            Some(Ok(schema)) => render_table_schema(ui, schema),
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

/// Alphabetise the sidebar tables (case-insensitive) and keep only those
/// whose name contains `filter` (case-insensitive). A blank/whitespace
/// filter keeps everything. Returns borrowed references so the caller
/// avoids cloning the whole list every frame (friction 2026-07-14).
fn filter_and_sort_tables<'a>(tables: &'a [TableInfo], filter: &str) -> Vec<&'a TableInfo> {
    let needle = filter.trim().to_lowercase();
    let mut visible: Vec<&TableInfo> = tables
        .iter()
        .filter(|t| needle.is_empty() || t.name.to_lowercase().contains(&needle))
        .collect();
    visible.sort_by_key(|t| t.name.to_lowercase());
    visible
}

/// Render a `DbError` as `<translated prefix>: <wire message>`. The
/// prefix comes from the active locale's `error-prefix-*` keys; the
/// message body is the server-returned English string and stays as-is
/// to preserve the ADR-0009 HTTP contract (see ADR-0015).
fn error_display(e: &DbError) -> String {
    let prefix = match e.category() {
        "connection" => t!("error-prefix-connection"),
        "schema" => t!("error-prefix-schema"),
        "type_conversion" => t!("error-prefix-type-conversion"),
        "capability" => t!("error-prefix-capability"),
        // Includes "query" and any future category that landed on the
        // server before the UI was updated — degrade visibly rather
        // than silently swallowing the prefix.
        _ => t!("error-prefix-query"),
    };
    format!("{prefix}: {}", e.message())
}

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

fn render_result(
    ui: &mut egui::Ui,
    result: &QueryResult,
    selection: &mut selection::ResultSelection,
) {
    use egui_extras::{Column, TableBuilder};

    if result.rows.is_empty() {
        ui.label(t_args!("result-affected", rows = result.rows_affected));
        return;
    }

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
                    // Empty gutter header above the row-number column.
                    header.col(|_ui| {});
                    for col in &result.columns {
                        header.col(|ui| {
                            ui.strong(&col.name);
                        });
                    }
                })
                .body(|body| {
                    body.rows(row_height, result.rows.len(), |mut row| {
                        let index = row.index();
                        // Highlight the whole row even though only the
                        // gutter is clickable, so the selection reads
                        // across all columns.
                        row.set_selected(selection.is_selected(index));
                        row.col(|ui| {
                            ui.with_layout(
                                egui::Layout::top_down_justified(egui::Align::Center),
                                |ui| {
                                    // 1-based row number, like a spreadsheet
                                    // row header. The justified layout makes
                                    // the whole gutter cell the click target,
                                    // not just the digits.
                                    let response = ui
                                        .selectable_label(
                                            selection.is_selected(index),
                                            (index + 1).to_string(),
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
                        let values: Vec<String> = result.rows[index]
                            .values()
                            .iter()
                            .map(ToString::to_string)
                            .collect();
                        for value in &values {
                            row.col(|ui| {
                                render_cell(ui, value, expand_id);
                            });
                        }
                    });
                });
        });

    if let Some((index, mods)) = pending_click {
        selection.click(index, mods);
    }

    render_expanded_cell_popup(ui, expand_id);
}

/// Column grid for the structure tab (ADR-0031): one row per column with
/// ordinal, name, declared type, nullability, primary-key flag, and the
/// raw default expression. Not virtualized — a table has few columns.
fn render_table_schema(ui: &mut egui::Ui, schema: &TableSchema) {
    use egui_extras::{Column, TableBuilder};

    if schema.columns.is_empty() {
        ui.label(t!("structure-no-columns"));
        return;
    }

    let row_height = egui::TextStyle::Body.resolve(ui.style()).size + 8.0;
    let headers: [String; 7] = [
        t!("structure-col-ordinal"),
        t!("structure-col-name"),
        t!("structure-col-type"),
        t!("structure-col-nullable"),
        t!("structure-col-pk"),
        t!("structure-col-default"),
        t!("structure-col-comment"),
    ];

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
            for col in &schema.columns {
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
                        ui.label(col.comment.as_deref().unwrap_or(""));
                    });
                });
            }
        });
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
    use super::{
        apply_auto_limit, cell_preview, error_display, is_bare_select, is_long_cell,
        should_run_from_keys, AiProviderSlot, Command, DbboardApp, HistoryStatus,
        PersistentHistoryStore, Reply, ResultTab, CELL_PREVIEW_CHARS, DEFAULT_CAPACITY,
    };
    use dbboard_core::{
        Column, ColumnInfo, DbError, QueryResult, Row, TableInfo, TableSchema, Value,
    };
    use std::sync::mpsc;
    use std::sync::{Arc, RwLock};

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
    fn run_sql_snaps_back_to_the_results_tab() {
        // Friction 2026-07-14: running a query while browsing a table's
        // structure left the result invisible behind the Structure tab.
        let (mut app, _cmd_rx, _reply_tx) = build();
        app.auto_limit = false;
        app.active_tab = ResultTab::Structure;
        app.sql = "SELECT 1".into();
        app.run_sql();
        assert_eq!(app.active_tab, ResultTab::Results);
    }

    fn tbls(names: &[&str]) -> Vec<TableInfo> {
        names.iter().map(|n| TableInfo::unqualified(*n)).collect()
    }

    #[test]
    fn filter_and_sort_orders_case_insensitively_regardless_of_input() {
        let tables = tbls(&["Zebra", "apple", "Mango"]);
        let out = super::filter_and_sort_tables(&tables, "");
        let names: Vec<&str> = out.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Mango", "Zebra"]);
    }

    #[test]
    fn filter_and_sort_keeps_case_insensitive_substring_matches() {
        let tables = tbls(&["users", "user_roles", "orders", "AUDIT_user"]);
        let out = super::filter_and_sort_tables(&tables, "USER");
        let names: Vec<&str> = out.iter().map(|t| t.name.as_str()).collect();
        // Alphabetised, and "orders" (no "user") is dropped.
        assert_eq!(names, vec!["AUDIT_user", "user_roles", "users"]);
    }

    #[test]
    fn filter_and_sort_blank_or_whitespace_filter_keeps_everything() {
        let tables = tbls(&["b", "a"]);
        assert_eq!(super::filter_and_sort_tables(&tables, "   ").len(), 2);
    }

    #[test]
    fn filter_and_sort_returns_empty_when_nothing_matches() {
        let tables = tbls(&["users", "orders"]);
        assert!(super::filter_and_sort_tables(&tables, "zzz").is_empty());
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
        let rendered = error_display(&e);
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
            let rendered = error_display(&e);
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

        app.switch_connection("store-cabaret".into());
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
                id: "store-cabaret".into(),
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
        assert!(msg.contains("store-cabaret"), "message names target: {msg}");
        assert!(
            msg.contains("host unreachable"),
            "message carries the wire error: {msg}"
        );

        // Cleared once a later switch succeeds, so a stale error never
        // lingers next to the now-active connection.
        reply_tx
            .send(Reply::ConnectionSwitched {
                id: "store-cabaret".into(),
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
                comment: None,
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
}
