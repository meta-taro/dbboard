//! axum request handlers. Each is a thin adapter from HTTP to the
//! shared `dyn DatabaseAdapter` behind [`AppState`]; all business logic
//! lives in the adapter implementations themselves.
//!
//! Each handler snapshots the live adapter through
//! [`AppState::current_adapter`] before doing any work, so a
//! mid-request swap (ADR-0020) cannot pull the adapter out from under
//! a query in flight.

use axum::extract::State;
use axum::Json;
use dbboard_core::QueryResult;

use crate::dto::{ApiError, CapabilitiesResponse, HealthResponse, QueryRequest, TablesResponse};
use crate::AppState;

/// Liveness probe. Does not touch the database — answering means the
/// HTTP server is up and the adapter was connected at startup.
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

pub(crate) async fn list_tables(
    State(state): State<AppState>,
) -> Result<Json<TablesResponse>, ApiError> {
    let adapter = state.current_adapter();
    let tables = adapter.list_tables().await?;
    Ok(Json(TablesResponse { tables }))
}

pub(crate) async fn run_query(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResult>, ApiError> {
    let adapter = state.current_adapter();
    let result = adapter.query(&req.sql).await?;
    Ok(Json(result))
}

/// Discovery endpoint (ADR-0012). Returns the adapter's stable id plus
/// the boolean capability flags so the UI can decide which optional
/// features to surface without probing each one individually.
pub(crate) async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesResponse> {
    let adapter = state.current_adapter();
    Json(CapabilitiesResponse {
        id: adapter.id(),
        capabilities: adapter.capabilities(),
    })
}
