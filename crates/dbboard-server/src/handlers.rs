//! axum request handlers. Each is a thin adapter from HTTP to the
//! shared [`Backend`](crate::backend::Backend); all business logic lives
//! in the adapters behind it.

use axum::extract::State;
use axum::Json;
use dbboard_core::QueryResult;

use crate::dto::{ApiError, HealthResponse, QueryRequest, TablesResponse};
use crate::AppState;

/// Liveness probe. Does not touch the database — answering means the
/// HTTP server is up and the backend was connected at startup.
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

pub(crate) async fn list_tables(
    State(state): State<AppState>,
) -> Result<Json<TablesResponse>, ApiError> {
    let tables = state.backend.list_tables().await?;
    Ok(Json(TablesResponse { tables }))
}

pub(crate) async fn run_query(
    State(state): State<AppState>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResult>, ApiError> {
    let result = state.backend.query(&req.sql).await?;
    Ok(Json(result))
}
