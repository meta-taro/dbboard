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

mod client;
mod connections;
mod history;
mod worker;

pub use connections::{
    AddFormState, ConnectionsView, EditFormState, EditKindState, KindSelector, Mode,
};
pub use history::{
    HistoryEntry, HistoryError, HistoryStatus, HistoryStore, PersistentHistoryStore,
    CURRENT_VERSION, DEFAULT_CAPACITY, ROTATION_BYTES, ROTATION_LINES,
};

use std::sync::mpsc::{self, Receiver, Sender};

use dbboard_core::{DbError, DbResult, QueryResult, TableInfo};
use dbboard_i18n::{t, t_args};
use eframe::egui;

/// Request flowing UI → worker.
#[derive(Debug, Clone)]
pub enum Command {
    /// Refresh the sidebar list of user tables.
    ListTables,
    /// Run an arbitrary SQL statement entered in the editor.
    Query(String),
}

/// Result flowing worker → UI.
#[derive(Debug)]
pub enum Reply {
    Tables(DbResult<Vec<TableInfo>>),
    QueryResult(DbResult<QueryResult>),
}

pub struct DbboardApp {
    sql: String,
    tables: DbResult<Vec<TableInfo>>,
    last_result: Option<DbResult<QueryResult>>,
    history: HistoryStore,
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
    #[must_use]
    pub fn connect(base_url: String, egui_ctx: egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (reply_tx, reply_rx) = mpsc::channel::<Reply>();
        worker::spawn_worker(base_url, cmd_rx, reply_tx, egui_ctx);
        Self::new(cmd_tx, reply_rx)
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
    pub fn new(cmd_tx: Sender<Command>, reply_rx: Receiver<Reply>) -> Self {
        let _ = cmd_tx.send(Command::ListTables);
        Self {
            sql: String::new(),
            tables: Ok(Vec::new()),
            last_result: None,
            history: HistoryStore::default(),
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
                    self.last_result = Some(r);
                    self.busy = false;
                }
            }
        }
    }

    fn run_sql(&mut self) {
        if self.busy || self.sql.trim().is_empty() {
            return;
        }
        self.history.push(self.sql.clone());
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

    /// Read-only view of the recently-run SQL statements (ADR-0014).
    #[must_use]
    pub fn history(&self) -> &HistoryStore {
        &self.history
    }
}

impl eframe::App for DbboardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_replies();

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
            egui::CollapsingHeader::new(t_args!("history-title", count = self.history.len()))
                .default_open(false)
                .show(ui, |ui| {
                    if self.history.is_empty() {
                        ui.label(t!("history-empty"));
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(160.0)
                            .show(ui, |ui| {
                                for entry in self.history.iter() {
                                    if ui.small_button(history_button_label(&entry.sql)).clicked() {
                                        restore = Some(entry.sql.clone());
                                    }
                                }
                            });
                    }
                });
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
    use super::{error_display, Command, DbboardApp, Reply};
    use dbboard_core::{Column, DbError, QueryResult, Row, TableInfo, Value};
    use std::sync::mpsc;

    fn build() -> (DbboardApp, mpsc::Receiver<Command>, mpsc::Sender<Reply>) {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();
        (DbboardApp::new(cmd_tx, reply_rx), cmd_rx, reply_tx)
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
}
