//! dbboard desktop binary entry point.
//!
//! Wires the egui UI (`dbboard-ui`) to a libSQL adapter
//! (`dbboard-turso`) through a pair of `std::sync::mpsc` channels.
//! The adapter lives on a dedicated worker thread that owns a
//! single-threaded tokio runtime, so the UI thread never blocks on
//! database I/O. The architecture rule — `dbboard-ui` depends only
//! on `dbboard-core` — is preserved because the UI sees the worker
//! as opaque `Command`/`Reply` traffic.
//!
//! The database path is taken from `DBBOARD_TURSO_PATH`; the default
//! is `":memory:"` so a fresh checkout runs without configuration.

use std::sync::mpsc;
use std::thread;

use dbboard_core::{DbError, DbResult};
use dbboard_turso::TursoAdapter;
use dbboard_ui::{Command, DbboardApp, Reply};

const TURSO_PATH_ENV: &str = "DBBOARD_TURSO_PATH";
const DEFAULT_TURSO_PATH: &str = ":memory:";

fn main() -> Result<(), eframe::Error> {
    let path = std::env::var(TURSO_PATH_ENV).unwrap_or_else(|_| DEFAULT_TURSO_PATH.into());

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (reply_tx, reply_rx) = mpsc::channel::<Reply>();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dbboard",
        native_options,
        Box::new(move |cc| {
            // Cloned so the worker can wake the UI thread after each
            // reply — egui is event-driven and would otherwise sleep
            // through the channel update.
            let egui_ctx = cc.egui_ctx.clone();
            spawn_worker(path.clone(), cmd_rx, reply_tx.clone(), egui_ctx);
            Ok(Box::new(DbboardApp::new(cmd_tx, reply_rx)))
        }),
    )
}

fn spawn_worker(
    path: String,
    cmd_rx: mpsc::Receiver<Command>,
    reply_tx: mpsc::Sender<Reply>,
    egui_ctx: egui::Context,
) {
    thread::Builder::new()
        .name("dbboard-worker".into())
        .spawn(move || run_worker(&path, &cmd_rx, &reply_tx, &egui_ctx))
        .expect("spawn dbboard-worker thread");
}

fn run_worker(
    path: &str,
    cmd_rx: &mpsc::Receiver<Command>,
    reply_tx: &mpsc::Sender<Reply>,
    egui_ctx: &egui::Context,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            report_fatal(
                reply_tx,
                egui_ctx,
                &DbError::Connection(e.to_string()),
                cmd_rx,
            );
            return;
        }
    };

    let adapter = match rt.block_on(TursoAdapter::connect_local(path)) {
        Ok(a) => a,
        Err(e) => {
            report_fatal(reply_tx, egui_ctx, &e, cmd_rx);
            return;
        }
    };

    while let Ok(cmd) = cmd_rx.recv() {
        let reply = match cmd {
            Command::ListTables => Reply::Tables(rt.block_on(adapter.list_tables())),
            Command::Query(sql) => Reply::QueryResult(rt.block_on(adapter.query(&sql))),
        };
        if reply_tx.send(reply).is_err() {
            // UI side has hung up — no point continuing.
            break;
        }
        egui_ctx.request_repaint();
    }
}

/// Adapter never came up. Echo the connection error back through the
/// reply channel and continue draining commands with the same error
/// so the UI does not deadlock waiting for replies it will never get.
fn report_fatal(
    reply_tx: &mpsc::Sender<Reply>,
    egui_ctx: &egui::Context,
    err: &DbError,
    cmd_rx: &mpsc::Receiver<Command>,
) {
    let _ = reply_tx.send(Reply::Tables(DbResult::<_>::Err(err.clone())));
    egui_ctx.request_repaint();

    while let Ok(cmd) = cmd_rx.recv() {
        let reply = match cmd {
            Command::ListTables => Reply::Tables(Err(err.clone())),
            Command::Query(_) => Reply::QueryResult(Err(err.clone())),
        };
        if reply_tx.send(reply).is_err() {
            break;
        }
        egui_ctx.request_repaint();
    }
}
