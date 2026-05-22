//! Background HTTP worker bridging the synchronous egui UI to the
//! loopback server.
//!
//! egui runs the UI on one thread and expects `update` to return
//! promptly, so blocking network calls cannot happen there. This worker
//! owns a dedicated thread with its own single-threaded tokio runtime,
//! drains [`Command`]s off the channel, performs the matching HTTP call
//! with `reqwest`, and posts a [`Reply`] back — waking the UI thread via
//! [`egui::Context::request_repaint`] so it drains the reply promptly.
//!
//! A per-request transport failure (server unreachable) maps to a
//! `Connection` error reply, so the UI shows it rather than deadlocking.
//! [`report_fatal`] covers the rarer case where the worker cannot even
//! build its runtime or HTTP client: it answers every command with that
//! error so the UI still makes progress.

use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use dbboard_core::DbError;
use eframe::egui;

use crate::client::{self, HttpRequest};
use crate::{Command, Reply};

/// Spawn the worker thread. `base_url` is the loopback server root the
/// binary just started (e.g. `http://127.0.0.1:54123`).
pub(crate) fn spawn_worker(
    base_url: String,
    cmd_rx: Receiver<Command>,
    reply_tx: Sender<Reply>,
    egui_ctx: egui::Context,
) {
    thread::Builder::new()
        .name("dbboard-http-worker".into())
        .spawn(move || run_worker(&base_url, &cmd_rx, &reply_tx, &egui_ctx))
        .expect("spawn dbboard-http-worker thread");
}

fn run_worker(
    base_url: &str,
    cmd_rx: &Receiver<Command>,
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            return report_fatal(
                reply_tx,
                egui_ctx,
                &DbError::Connection(e.to_string()),
                cmd_rx,
            )
        }
    };
    let http = match reqwest::Client::builder().build() {
        Ok(client) => client,
        Err(e) => {
            return report_fatal(
                reply_tx,
                egui_ctx,
                &DbError::Connection(e.to_string()),
                cmd_rx,
            )
        }
    };

    while let Ok(cmd) = cmd_rx.recv() {
        let request = client::request_for(&cmd);
        let reply = rt.block_on(execute(&http, base_url, &request));
        if reply_tx.send(reply).is_err() {
            break; // UI side hung up — nothing left to answer.
        }
        egui_ctx.request_repaint();
    }
}

async fn execute(http: &reqwest::Client, base_url: &str, request: &HttpRequest) -> Reply {
    match request {
        HttpRequest::GetTables => {
            let response = http.get(format!("{base_url}/tables")).send().await;
            match read(response).await {
                Ok((status, body)) => client::reply_for_tables(status, &body),
                Err(e) => Reply::Tables(Err(e)),
            }
        }
        HttpRequest::PostQuery(sql) => {
            let response = http
                .post(format!("{base_url}/query"))
                .json(&serde_json::json!({ "sql": sql }))
                .send()
                .await;
            match read(response).await {
                Ok((status, body)) => client::reply_for_query(status, &body),
                Err(e) => Reply::QueryResult(Err(e)),
            }
        }
    }
}

/// Collapse a `reqwest` send result into `(status, body)`, turning any
/// transport-level failure into a `Connection` error.
async fn read(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<(u16, String), DbError> {
    let response = response.map_err(transport_error)?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(transport_error)?;
    Ok((status, body))
}

fn transport_error(err: reqwest::Error) -> DbError {
    // `without_url` strips the request URL from the message; it carries
    // no secrets here, but keeping errors URL-free is the safe default.
    DbError::Connection(format!("request failed: {}", err.without_url()))
}

/// The worker could not start its runtime or HTTP client. Echo the error
/// back and keep answering every command with it, so the UI surfaces the
/// failure instead of waiting forever for replies that will never come.
fn report_fatal(
    reply_tx: &Sender<Reply>,
    egui_ctx: &egui::Context,
    err: &DbError,
    cmd_rx: &Receiver<Command>,
) {
    let _ = reply_tx.send(Reply::Tables(Err(err.clone())));
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
