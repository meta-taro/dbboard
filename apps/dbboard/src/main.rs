//! dbboard desktop binary entry point.
//!
//! The binary boots an in-process loopback HTTP server (`dbboard-server`)
//! and points the egui UI (`dbboard-ui`) at it. The server owns the
//! database adapter and resolves which backend to connect from the
//! environment (see [`dbboard_server::backend_config_from_env`]); the UI
//! is a pure HTTP client. This keeps the desktop app on the same API
//! contract as the dbboard-web sibling (ADR-0009).
//!
//! Two runtimes coexist without nesting: this `main` owns a multi-thread
//! tokio runtime that drives the server, while the UI's HTTP worker runs
//! a separate current-thread runtime on its own thread. The UI thread
//! itself never blocks on I/O.

use dbboard_server::{backend_config_from_env, serve};
use dbboard_ui::DbboardApp;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server runtime lives for the whole process. Connecting here
    // (before the window opens) preserves the fail-fast contract: a bad
    // connection string aborts startup instead of failing on first use.
    let rt = tokio::runtime::Runtime::new()?;
    let server = rt.block_on(serve(backend_config_from_env()))?;
    let base_url = format!("http://127.0.0.1:{}", server.port);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "dbboard",
        native_options,
        Box::new(move |cc| Ok(Box::new(DbboardApp::connect(base_url, cc.egui_ctx.clone())))),
    );

    // The UI has exited; stop the server before reporting how it went.
    rt.block_on(server.shutdown())?;
    result.map_err(Into::into)
}
