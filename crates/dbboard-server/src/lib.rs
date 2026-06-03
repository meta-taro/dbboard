//! Local in-process HTTP backend for dbboard (Phase 1.5 / ADR-0009).
//!
//! The desktop binary boots this server on a loopback socket and the
//! egui UI talks to it over HTTP, using the same contract as the
//! dbboard-web sibling (`docs/api-contract.md`). The server owns the
//! connected adapter; the UI never links an adapter directly.
//!
//! The server binds `127.0.0.1:0` so the OS assigns a free port, which
//! the caller reads back via [`RunningServer::port`]. Binding to
//! loopback keeps the database reachable only from this machine.
//!
//! # Security model
//!
//! The endpoints are **unauthenticated by design**: this is a
//! single-user, loopback-only server whose port is known only to the
//! process that spawned it. That trade-off holds only as long as those
//! assumptions do — if the port is ever persisted across restarts or
//! the bind address widened beyond loopback, a per-launch secret
//! (e.g. an `X-DBBoard-Token` header) must be added first.

mod backend;
mod config;
mod dto;
mod handlers;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use dbboard_core::{DatabaseAdapter, DbError};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use backend::connect_adapter;

pub use config::{backend_config_from_env, backend_config_from_env_and_store, BackendConfig};

/// Maximum accepted `POST /query` body. 64 KiB comfortably holds any
/// hand-written SQL statement while bounding per-request memory.
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;

/// Shared application state handed to every request: the single
/// connected adapter behind an `Arc<dyn DatabaseAdapter>` (see
/// [`backend`] for why it must not be reconnected per request).
///
/// `#[non_exhaustive]` so it can only be obtained from [`connect`] —
/// callers receive it and hand it back to [`build_router`] but cannot
/// construct or destructure it, leaving the internals free to evolve.
#[derive(Clone)]
#[non_exhaustive]
pub struct AppState {
    pub(crate) adapter: Arc<dyn DatabaseAdapter>,
}

/// Failure modes when standing up the server.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The adapter failed to connect at startup (fail-fast contract).
    #[error(transparent)]
    Backend(#[from] DbError),
    /// Binding the loopback socket or serving failed.
    #[error("server I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The server task panicked or was cancelled.
    #[error("server task failed: {0}")]
    Join(String),
}

/// A running server and the handle needed to stop it. The port is the
/// OS-assigned loopback port the UI connects to.
pub struct RunningServer {
    pub port: u16,
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<std::io::Result<()>>,
}

impl RunningServer {
    /// Signal graceful shutdown and wait for the server task to finish.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError`] if the server task panicked or the final
    /// serve future returned an I/O error.
    pub async fn shutdown(self) -> Result<(), ServerError> {
        // A closed receiver just means the server already stopped.
        let _ = self.shutdown_tx.send(());
        // Three distinct outcomes, kept explicit rather than chained
        // through `??`: the task may panic (Join), the serve future may
        // return an I/O error (Io), or it may exit cleanly.
        match self.handle.await {
            Err(join_err) => Err(ServerError::Join(join_err.to_string())),
            Ok(Err(io_err)) => Err(ServerError::Io(io_err)),
            Ok(Ok(())) => Ok(()),
        }
    }
}

/// Connect the backend and wrap it in shareable [`AppState`].
///
/// # Errors
///
/// Returns [`ServerError::Backend`] when the adapter cannot connect.
/// This is the only variant `connect` can produce — socket binding
/// (and hence [`ServerError::Io`]) happens later, in [`serve`].
pub async fn connect(config: BackendConfig) -> Result<AppState, ServerError> {
    let adapter = connect_adapter(config).await?;
    Ok(AppState { adapter })
}

/// Build the axum router for a connected backend. Exposed so tests can
/// drive it with `tower::ServiceExt::oneshot`, without binding a socket.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/capabilities", get(handlers::capabilities))
        .route("/tables", get(handlers::list_tables))
        .route("/query", post(handlers::run_query))
        // A SQL editor sends short statements; cap the body well below
        // axum's 2 MB default so a runaway or hostile request can't
        // balloon memory before a handler ever runs.
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

/// Connect the backend, bind a loopback socket, and spawn the server.
///
/// Returns once the adapter is connected and the listener is bound, so
/// a bad connection string fails fast here rather than on the first
/// request.
///
/// Must be called from within a tokio runtime; the server task is
/// spawned onto it. A `current_thread` runtime works (request futures
/// then share that one thread) but a multi-thread runtime is preferred
/// when the caller owns a dedicated one, as the desktop binary does.
///
/// # Errors
///
/// Returns [`ServerError`] if the adapter cannot connect or the loopback
/// socket cannot be bound.
pub async fn serve(config: BackendConfig) -> Result<RunningServer, ServerError> {
    let state = connect(config).await?;
    let router = build_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    Ok(RunningServer {
        port,
        shutdown_tx,
        handle,
    })
}
