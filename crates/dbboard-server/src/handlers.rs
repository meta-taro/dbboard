//! axum request handlers. Each is a thin adapter from HTTP to the
//! shared `dyn DatabaseAdapter` behind [`AppState`]; all business logic
//! lives in the adapter implementations themselves.

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
    let tables = state.adapter.list_tables().await?;
    Ok(Json(TablesResponse { tables }))
}

pub(crate) async fn run_query(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResult>, ApiError> {
    let result = state.adapter.query(&req.sql).await?;
    Ok(Json(result))
}

/// Discovery endpoint (ADR-0012). Returns the adapter's stable id plus
/// the boolean capability flags so the UI can decide which optional
/// features to surface without probing each one individually.
pub(crate) async fn capabilities(State(state): State<AppState>) -> Json<CapabilitiesResponse> {
    Json(CapabilitiesResponse {
        id: state.adapter.id(),
        capabilities: state.adapter.capabilities(),
    })
}
