//! dbboard desktop binary entry point.
//!
//! Wires the egui UI (`dbboard-ui`) to a database adapter through a
//! pair of `std::sync::mpsc` channels. The adapter lives on a dedicated
//! worker thread that owns a single-threaded tokio runtime, so the UI
//! thread never blocks on database I/O. The architecture rule —
//! `dbboard-ui` depends only on `dbboard-core` — is preserved because
//! the UI sees the worker as opaque `Command`/`Reply` traffic.
//!
//! Which backend the worker drives is chosen from the environment at
//! startup:
//!
//! - If `DBBOARD_D1_ACCOUNT_ID`, `DBBOARD_D1_DATABASE_ID`, and
//!   `DBBOARD_D1_TOKEN` are all set, it connects to **Cloudflare D1**
//!   over the REST API (optionally overriding the API root with
//!   `DBBOARD_D1_BASE_URL`).
//! - Otherwise it opens a local **Turso/libSQL** database at
//!   `DBBOARD_TURSO_PATH`, defaulting to `":memory:"` so a fresh
//!   checkout runs without any configuration.

use std::sync::mpsc;
use std::thread;

use dbboard_core::{DbError, DbResult, QueryResult, TableInfo};
use dbboard_d1::{D1Adapter, D1Config};
use dbboard_turso::TursoAdapter;
use dbboard_ui::{Command, DbboardApp, Reply};

const TURSO_PATH_ENV: &str = "DBBOARD_TURSO_PATH";
const DEFAULT_TURSO_PATH: &str = ":memory:";

const D1_ACCOUNT_ID_ENV: &str = "DBBOARD_D1_ACCOUNT_ID";
const D1_DATABASE_ID_ENV: &str = "DBBOARD_D1_DATABASE_ID";
const D1_TOKEN_ENV: &str = "DBBOARD_D1_TOKEN";
const D1_BASE_URL_ENV: &str = "DBBOARD_D1_BASE_URL";

/// What the worker should connect to. Resolved from the environment on
/// the UI thread (cheap, no I/O) and handed to the worker, which does
/// the actual connecting inside its tokio runtime.
enum BackendConfig {
    Turso { path: String },
    D1(D1Config),
}

/// A connected adapter. The variants share the small command surface
/// the worker needs; statement dispatch is a plain `match`.
enum Backend {
    Turso(TursoAdapter),
    D1(D1Adapter),
}

impl Backend {
    async fn connect(config: BackendConfig) -> DbResult<Self> {
        match config {
            BackendConfig::Turso { path } => {
                Ok(Self::Turso(TursoAdapter::connect_local(&path).await?))
            }
            BackendConfig::D1(cfg) => {
                let adapter = D1Adapter::connect(cfg)?;
                // Verify connectivity up front so a bad token or id
                // surfaces as a connection error at startup, matching
                // how the Turso path fails fast on a bad file.
                adapter.ping().await?;
                Ok(Self::D1(adapter))
            }
        }
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        match self {
            Self::Turso(a) => a.list_tables().await,
            Self::D1(a) => a.list_tables().await,
        }
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        match self {
            Self::Turso(a) => a.query(sql).await,
            Self::D1(a) => a.query(sql).await,
        }
    }
}

fn backend_config_from_env() -> BackendConfig {
    // D1 wins only when fully configured; a partial D1 setup falls back
    // to Turso rather than failing, so a stray env var can't lock the
    // app out of its default local mode.
    if let (Ok(account_id), Ok(database_id), Ok(api_token)) = (
        std::env::var(D1_ACCOUNT_ID_ENV),
        std::env::var(D1_DATABASE_ID_ENV),
        std::env::var(D1_TOKEN_ENV),
    ) {
        return BackendConfig::D1(D1Config {
            account_id,
            database_id,
            api_token,
            base_url: std::env::var(D1_BASE_URL_ENV).ok(),
        });
    }

    let path = std::env::var(TURSO_PATH_ENV).unwrap_or_else(|_| DEFAULT_TURSO_PATH.into());
    BackendConfig::Turso { path }
}

fn main() -> Result<(), eframe::Error> {
    let config = backend_config_from_env();

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
            spawn_worker(config, cmd_rx, reply_tx, egui_ctx);
            Ok(Box::new(DbboardApp::new(cmd_tx, reply_rx)))
        }),
    )
}

fn spawn_worker(
    config: BackendConfig,
    cmd_rx: mpsc::Receiver<Command>,
    reply_tx: mpsc::Sender<Reply>,
    egui_ctx: egui::Context,
) {
    thread::Builder::new()
        .name("dbboard-worker".into())
        .spawn(move || run_worker(config, &cmd_rx, &reply_tx, &egui_ctx))
        .expect("spawn dbboard-worker thread");
}

fn run_worker(
    config: BackendConfig,
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

    let backend = match rt.block_on(Backend::connect(config)) {
        Ok(b) => b,
        Err(e) => {
            report_fatal(reply_tx, egui_ctx, &e, cmd_rx);
            return;
        }
    };

    while let Ok(cmd) = cmd_rx.recv() {
        let reply = match cmd {
            Command::ListTables => Reply::Tables(rt.block_on(backend.list_tables())),
            Command::Query(sql) => Reply::QueryResult(rt.block_on(backend.query(&sql))),
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
