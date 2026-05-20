//! Presentation layer for dbboard.
//!
//! The UI talks to whichever adapter is bound at runtime through a
//! pair of `std::sync::mpsc` channels — it sends [`Command`]s and
//! receives [`Reply`]s. This keeps the crate dependency-free of any
//! particular adapter (libSQL today, Neon / Supabase later) and
//! preserves the architectural rule that `dbboard-ui` only depends
//! on `dbboard-core`.
//!
//! The application binary (`apps/dbboard`) is responsible for
//! spawning a worker that owns the adapter, draining the command
//! channel, and posting replies back.

use std::sync::mpsc::{Receiver, Sender};

use dbboard_core::{DbResult, QueryResult, TableInfo};
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
    busy: bool,
    cmd_tx: Sender<Command>,
    reply_rx: Receiver<Reply>,
}

impl DbboardApp {
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
}

impl eframe::App for DbboardApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_replies();

        egui::Panel::left("tables").show_inside(ui, |ui| {
            ui.heading("Tables");
            ui.separator();
            match &self.tables {
                Ok(tables) if tables.is_empty() => {
                    ui.label("(no tables)");
                }
                Ok(tables) => {
                    for t in tables {
                        ui.label(&t.name);
                    }
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
                }
            }
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SQL");
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Run"))
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

            ui.separator();
            ui.heading("Result");
            match &self.last_result {
                None => {
                    ui.label("(run a query)");
                }
                Some(Ok(result)) => render_result(ui, result),
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, e.to_string());
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

fn render_result(ui: &mut egui::Ui, result: &QueryResult) {
    if result.rows.is_empty() {
        ui.label(format!("OK ({} rows affected)", result.rows_affected));
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
    use super::{Command, DbboardApp, Reply};
    use dbboard_core::{Column, QueryResult, Row, TableInfo, Value};
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
}
