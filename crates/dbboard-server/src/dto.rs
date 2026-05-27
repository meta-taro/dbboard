//! HTTP request/response envelopes and the error-to-status mapping.
//!
//! The wire shapes here are the canonical API contract
//! (`docs/api-contract.md`), mirrored by the dbboard-web sibling.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use dbboard_core::{Capabilities, DbError, TableInfo};
use serde::{Deserialize, Serialize};

/// `POST /query` request body.
#[derive(Debug, Deserialize)]
pub(crate) struct QueryRequest {
    pub sql: String,
}

/// `GET /health` response body.
#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub status: &'static str,
}

/// `GET /tables` response body.
#[derive(Debug, Serialize)]
pub(crate) struct TablesResponse {
    pub tables: Vec<TableInfo>,
}

/// `GET /capabilities` response body. `id` identifies the connected
/// adapter (e.g. `"turso"`, `"d1"`, `"postgres"`); `capabilities` is the
/// flat per-feature flag struct from `dbboard-core` (ADR-0012).
#[derive(Debug, Serialize)]
pub(crate) struct CapabilitiesResponse {
    pub id: &'static str,
    pub capabilities: Capabilities,
}

/// Error response wrapper: `{"error":{"category":"...","message":"..."}}`.
#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    category: &'a str,
    message: &'a str,
}

/// Local newtype so we can `impl IntoResponse` for a foreign error type
/// (the orphan rule forbids implementing it on `DbError` directly).
pub(crate) struct ApiError(pub DbError);

impl From<DbError> for ApiError {
    fn from(err: DbError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = status_for(&self.0);
        let body = Json(ErrorEnvelope {
            error: ErrorBody {
                category: self.0.category(),
                message: self.0.message(),
            },
        });
        (status, body).into_response()
    }
}

/// Map a domain error onto an HTTP status. A bad SQL statement is the
/// caller's fault (`400`); a type the adapter cannot represent is
/// semantically invalid (`422`); connection and schema failures are the
/// upstream database's fault from the UI's perspective (`502`); a
/// capability the adapter does not implement is treated as a missing
/// resource (`404`, per ADR-0012) so the UI can hide the feature
/// cleanly instead of surfacing it as a SQL error.
fn status_for(err: &DbError) -> StatusCode {
    match err {
        DbError::Query(_) => StatusCode::BAD_REQUEST,
        DbError::TypeConversion(_) => StatusCode::UNPROCESSABLE_ENTITY,
        DbError::Connection(_) | DbError::Schema(_) => StatusCode::BAD_GATEWAY,
        DbError::Capability(_) => StatusCode::NOT_FOUND,
    }
}

#[cfg(test)]
mod tests {
    use super::status_for;
    use axum::http::StatusCode;
    use dbboard_core::DbError;

    #[test]
    fn query_errors_are_client_errors() {
        assert_eq!(
            status_for(&DbError::Query(String::new())),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn type_conversion_is_unprocessable() {
        assert_eq!(
            status_for(&DbError::TypeConversion(String::new())),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn connection_and_schema_are_bad_gateway() {
        assert_eq!(
            status_for(&DbError::Connection(String::new())),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status_for(&DbError::Schema(String::new())),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn capability_unavailable_is_not_found() {
        assert_eq!(
            status_for(&DbError::Capability(String::new())),
            StatusCode::NOT_FOUND
        );
    }
}
