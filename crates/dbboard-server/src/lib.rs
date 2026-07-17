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

use std::sync::{Arc, RwLock};

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use dbboard_core::{DatabaseAdapter, DbError};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use backend::connect_adapter;

pub use config::{
    backend_config_for_entry, backend_config_from_env, backend_config_from_env_and_store,
    resolved_connection_label, BackendConfig,
};

/// Maximum accepted `POST /query` body. 64 KiB comfortably holds any
/// hand-written SQL statement while bounding per-request memory.
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;

/// Shared application state handed to every request: the live adapter
/// handle behind an `Arc<RwLock<Arc<dyn DatabaseAdapter>>>` (ADR-0020).
/// Request handlers read the current `Arc<dyn DatabaseAdapter>` at the
/// start of the request via [`AppState::current_adapter`] and operate
/// on that captured `Arc` for the request's lifetime. A swap from
/// outside the request loop ([`swap_backend`]) takes effect on the
/// *next* request; queries already in flight finish against the
/// adapter they captured. See [`backend`] for why the adapter must
/// not be reconnected per request.
///
/// The lock is held only long enough to clone the inner `Arc`, so the
/// guard never crosses an `.await`; a write contends with a snapshot
/// read just for that hand-off and an awaiting query keeps no lock.
///
/// `#[non_exhaustive]` so it can only be obtained from [`connect`] —
/// callers receive it and hand it back to [`build_router`] but cannot
/// construct or destructure it, leaving the internals free to evolve.
#[derive(Clone)]
#[non_exhaustive]
pub struct AppState {
    pub(crate) adapter: Arc<RwLock<Arc<dyn DatabaseAdapter>>>,
}

impl AppState {
    /// Snapshot the current adapter into a stable `Arc` for one request.
    /// The returned handle is unaffected by subsequent [`swap_backend`]
    /// calls — this is the per-request capture ADR-0020 relies on. The
    /// read guard is dropped before this function returns, so no lock
    /// is held across the handler's `.await`.
    ///
    /// Public since ADR-0028 slice (c): the desktop binary implements
    /// `dbboard-ui`'s `SchemaSource` over this snapshot so the UI
    /// worker can fan out `describe_table` in-process — the HTTP
    /// contract shared with dbboard-web stays untouched.
    pub fn current_adapter(&self) -> Arc<dyn DatabaseAdapter> {
        // A poisoned lock means a prior writer panicked mid-swap. The
        // inner `Arc<dyn DatabaseAdapter>` is still a valid handle —
        // either the pre-swap adapter or the new one — so unwrap the
        // poison and keep serving.
        let guard = self
            .adapter
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(&guard)
    }
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
///
/// The shared [`AppState`] is exposed through [`RunningServer::state`]
/// so the desktop binary can swap the live adapter at runtime via
/// [`swap_backend`] (ADR-0020) without restarting the server.
pub struct RunningServer {
    pub port: u16,
    state: AppState,
    shutdown_tx: oneshot::Sender<()>,
    handle: JoinHandle<std::io::Result<()>>,
}

impl RunningServer {
    /// Snapshot the live [`AppState`] so the caller can hand it to
    /// [`swap_backend`] later. Returns a clone — `AppState` is `Clone`
    /// and internally `Arc`-shared, so the returned handle still points
    /// at the same shared adapter slot the router sees.
    #[must_use]
    pub fn state(&self) -> AppState {
        self.state.clone()
    }

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
    Ok(AppState {
        adapter: Arc::new(RwLock::new(adapter)),
    })
}

/// Build a fresh adapter without wrapping it in an [`AppState`]. The
/// desktop binary uses this to construct the *next* adapter while a
/// previous one is still live, then hands it to [`swap_backend`]
/// (ADR-0020). Tests use it for the same flow.
///
/// # Errors
///
/// Returns [`ServerError::Backend`] when the adapter cannot connect.
pub async fn build_adapter(config: BackendConfig) -> Result<Arc<dyn DatabaseAdapter>, ServerError> {
    Ok(connect_adapter(config).await?)
}

/// Atomically swap the live adapter behind `state` (ADR-0020). Requests
/// already in flight finish against the adapter they captured at the
/// start of the request; the *next* request sees `new`. The HTTP
/// contract (`docs/api-contract.md`) is unchanged — this is an
/// in-process wiring detail, not an HTTP endpoint.
pub fn swap_backend(state: &AppState, new: Arc<dyn DatabaseAdapter>) {
    // Same poison-handling rationale as `current_adapter`: if a prior
    // writer panicked, the inner handle is still valid, so unwrap the
    // poison and replace it.
    let mut guard = state
        .adapter
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = new;
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
    let router = build_router(state.clone());

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
        state,
        shutdown_tx,
        handle,
    })
}
