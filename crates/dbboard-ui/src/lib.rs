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
mod history;
mod worker;

pub use ai::{AiMode, AiPanel, AiResponseView};
pub use ai_settings::AiSettingsView;
pub use connections::{
    AddFormState, ConnectionsView, EditFormState, EditKindState, KindSelector, Mode,
};
pub use history::{
    HistoryEntry, HistoryError, HistoryStatus, HistoryStore, PersistentHistoryStore,
    CURRENT_VERSION, DEFAULT_CAPACITY, ROTATION_BYTES, ROTATION_LINES,
};
// Fixture-emission shim for the `dbboard-web` sibling's
// cross-implementation round-trip test (ADR-0017). Used only by the
// `emit_history_fixture` example — hidden from rustdoc; do not call
// from production code.
#[doc(hidden)]
pub use history::fixture;
pub use worker::{AiProviderSlot, AiProviderSwitcher, ConnectionSwitcher};
// Re-export so the desktop binary can implement [`ConnectionSwitcher`]
// (return type `Result<(), DbError>`) without taking a direct dep on
// `dbboard-core` — the architecture rule is that only the server and
// adapters link to `dbboard-core` (see CLAUDE.md).
pub use dbboard_ai::{AiError, AiProvider};
pub use dbboard_core::DbError;

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, PoisonError};
use std::time::Instant;

use dbboard_core::{DbResult, QueryResult, TableInfo};
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
    /// snapshot, used as the provider's schema hint.
    AiSuggest {
        prompt: String,
        dialect: Option<String>,
        schema: Vec<TableInfo>,
    },
    /// Swap the active AI provider to the entry named `id` from
    /// `ai-providers.toml` (ADR-0025). In-process, not HTTP — the swap
    /// is delegated to an injected [`worker::AiProviderSwitcher`]
    /// supplied by the binary. Surfaces as `Reply::AiProviderSwitched`
    /// or `Reply::AiProviderSwitchFailed`.
    SwitchAiProvider { id: String },
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
    AiResponded {
        text: String,
        tokens_in: u32,
        tokens_out: u32,
    },
    /// AI request failed (ADR-0023). The panel renders the error using
    /// its own translation table — the AI taxonomy is independent of
    /// the HTTP `DbError` taxonomy (ADR-0023 Decision 8).
    AiFailed {
        error: AiError,
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
}

/// Captures everything we know at submit time about a query whose reply
/// has not yet arrived. The completion-time path consumes this on
/// [`Reply::QueryResult`] to build the rich ADR-0017 record (`duration_ms`
/// from `started.elapsed()`, `sql` carried through verbatim).
struct PendingSubmit {
    started: Instant,
    sql: String,
}

/// Wall-clock function used to stamp `ts` on every completion record.
/// Injected (rather than calling `SystemTime::now()` directly) so
/// `dbboard-ui` stays free of any date-formatting crate dependency and
/// so tests can pass a deterministic stub.
pub type RfcClock = fn() -> String;

pub struct DbboardApp {
    sql: String,
    tables: DbResult<Vec<TableInfo>>,
    last_result: Option<DbResult<QueryResult>>,
    history: PersistentHistoryStore,
    /// `Some` between submitting a query and consuming its reply; the
    /// `drain_replies` path uses this to compute `duration_ms`.
    pending: Option<PendingSubmit>,
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
    // Arg count grows by one with each in-process switcher we wire
    // through the worker (ADR-0020 ConnectionSwitcher, ADR-0025
    // AiProviderSwitcher). A struct-builder refactor is queued for
    // slice (b) of issue 0008 when the AI panel adds yet another
    // handle; until then, allowing here keeps the slice focused.
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
        );
        Self::new(
            cmd_tx,
            reply_rx,
            history,
            conn_label,
            now_rfc3339,
            ai_provider_slot,
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
    ) -> Self {
        let _ = cmd_tx.send(Command::ListTables);
        Self {
            sql: String::new(),
            tables: Ok(Vec::new()),
            last_result: None,
            history,
            pending: None,
            conn_label,
            last_switch_error: None,
            now_rfc3339,
            ai_provider_slot,
            ai_panel: AiPanel::new(),
            busy: false,
            cmd_tx,
            reply_rx,
        }
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
                // ADR-0023: AI round-trip reply. Route into the panel's
                // state machine — both success and failure clear `busy`
                // and replace any stale content (ai::tests cover the
                // ordering invariants).
                Reply::AiResponded {
                    text,
                    tokens_in,
                    tokens_out,
                } => {
                    self.ai_panel.on_response(text, tokens_in, tokens_out);
                }
                Reply::AiFailed { error } => {
                    self.ai_panel.on_error(&error);
                }
                // ADR-0025: AI provider swap outcomes. The Settings UI
                // that consumes these lands in slice (b) of issue 0008;
                // for now we absorb the replies so the worker channel
                // stays drained and the dispatch match remains
                // exhaustive. No state on `DbboardApp` is updated here
                // — the panel will read switch state directly off the
                // `AiSettingsAdmin` it owns once it ships.
                Reply::AiProviderSwitched { .. } | Reply::AiProviderSwitchFailed { .. } => {}
            }
        }
    }

    fn run_sql(&mut self) {
        if self.busy || self.sql.trim().is_empty() {
            return;
        }
        // Submit-time: push the bare SQL into the in-memory ring so the
        // history panel updates instantly; disk append happens at reply
        // time, once we know duration / rows / status (ADR-0017).
        self.history.record_submit(self.sql.clone());
        self.pending = Some(PendingSubmit {
            started: Instant::now(),
            sql: self.sql.clone(),
        });
        self.busy = true;
        let _ = self.cmd_tx.send(Command::Query(self.sql.clone()));
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
    pub fn switch_connection(&mut self, id: String) {
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
            HistoryEntry {
                sql: pending.sql.clone(),
                ts,
                conn: conn.to_string(),
                status: HistoryStatus::Ok,
                duration_ms,
                rows,
                rows_affected,
                error: None,
            }
        }
        Err(e) => HistoryEntry {
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
        },
    }
}

impl eframe::App for DbboardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_replies();

        // ADR-0023: AI panel as a free-floating egui::Window. Only
        // register it when a provider was wired in at startup; the panel
        // itself trusts the gate. Send-clicks return a Command that we
        // forward to the worker — failure to send (worker hung up)
        // becomes the user's next Reply::AiFailed, not a silent drop,
        // because the panel's `busy` flag would otherwise stick.
        if self.has_ai_provider() {
            // `dialect` is the active adapter id (e.g. "postgres", "neon").
            // The UI does not currently reach the loopback server's
            // adapter id — bridging that requires either a
            // `Command::GetCapabilities` round-trip or a dedicated binary-
            // side accessor. Slice (b) ships without the hint; Stage 2
            // wires it once the adapter-id surface is decided.
            let dialect: Option<&str> = None;
            // Borrow the cached tables rather than cloning them every
            // frame; the panel only allocates a Vec when Send is clicked
            // and the Suggest arm fires.
            let schema_slice: &[TableInfo] = self.tables.as_ref().map_or(&[], Vec::as_slice);
            if let Some(cmd) = self.ai_panel.ui(ui.ctx(), dialect, schema_slice) {
                if self.cmd_tx.send(cmd).is_err() {
                    // Worker hung up — surface a synthetic failure so
                    // the panel exits the busy state immediately rather
                    // than waiting forever for a reply that will never
                    // arrive.
                    self.ai_panel
                        .on_error(&AiError::Network("ai worker channel closed".into()));
                }
            }
            // Drive a follow-up frame while the AI request is in flight
            // so the reply drains promptly without a user gesture.
            if self.ai_panel.is_busy() {
                ui.ctx().request_repaint();
            }
        }

        egui::Panel::left("tables").show_inside(ui, |ui| {
            ui.heading(t!("tables-heading"));
            ui.separator();
            match &self.tables {
                Ok(tables) if tables.is_empty() => {
                    ui.label(t!("tables-empty"));
                }
                Ok(tables) => {
                    for table in tables {
                        ui.label(&table.name);
                    }
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error_display(e));
                }
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(t!("sql-heading"));
                if ui
                    .add_enabled(!self.busy, egui::Button::new(t!("sql-run-button")))
                    .clicked()
                {
                    self.run_sql();
                }
                if self.busy {
                    ui.spinner();
                }
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.sql)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );

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
                                    for entry in history.iter() {
                                        if ui
                                            .small_button(history_button_label(&entry.sql))
                                            .clicked()
                                        {
                                            restore = Some(entry.sql.clone());
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
            ui.heading(t!("result-heading"));
            match &self.last_result {
                None => {
                    ui.label(t!("result-empty"));
                }
                Some(Ok(result)) => render_result(ui, result),
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error_display(e));
                }
            }
        });

        // Egui is event-driven, so request a follow-up frame while a
        // query is in flight to keep draining the reply channel.
        if self.busy {
            ui.ctx().request_repaint();
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

fn render_result(ui: &mut egui::Ui, result: &QueryResult) {
    if result.rows.is_empty() {
        ui.label(t_args!("result-affected", rows = result.rows_affected));
        return;
    }

    egui::ScrollArea::both().show(ui, |ui| {
        egui::Grid::new("result").striped(true).show(ui, |ui| {
            for col in &result.columns {
                ui.strong(&col.name);
            }
            ui.end_row();
            for row in &result.rows {
                for v in row.values() {
                    ui.label(v.to_string());
                }
                ui.end_row();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{
        error_display, AiProviderSlot, Command, DbboardApp, HistoryStatus, PersistentHistoryStore,
        Reply, DEFAULT_CAPACITY,
    };
    use dbboard_core::{Column, DbError, QueryResult, Row, TableInfo, Value};
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
        app.sql = "SELECT 1".into();
        app.run_sql();

        assert!(app.is_busy());
        let first = cmd_rx.try_recv().expect("Query command emitted");
        let second = cmd_rx.try_recv().expect("ListTables command emitted");
        assert!(matches!(first, Command::Query(sql) if sql == "SELECT 1"));
        assert!(matches!(second, Command::ListTables));
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
        app.sql = "SELECT 1".into();
        app.run_sql();

        assert_eq!(app.history().len(), 1);
        assert_eq!(app.history().iter().next().unwrap().sql, "SELECT 1");
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
        app.sql = "SELECT 1".into();
        app.run_sql();
        assert_eq!(app.history().len(), 1);

        // Still busy (no reply drained); a second Run with a different
        // statement should not pollute history.
        app.sql = "SELECT 2".into();
        app.run_sql();
        assert_eq!(app.history().len(), 1);
        assert_eq!(app.history().iter().next().unwrap().sql, "SELECT 1");
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

        app.sql = "SELECT 1".into();
        app.run_sql();
        reply_tx
            .send(Reply::QueryResult(Ok(ok_select_one())))
            .unwrap();
        app.drain_replies();

        let lines = read_history_jsonl(&path);
        assert_eq!(lines.len(), 1);
        let r = &lines[0];
        assert_eq!(r["v"], 1);
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
        assert_eq!(err.status, HistoryStatus::Error);
        assert_eq!(err.error.as_ref().unwrap().category, "connection");
    }
}
