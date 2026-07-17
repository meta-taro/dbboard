//! Pure mapping between UI [`Command`]s/[`Reply`]s and the HTTP wire.
//!
//! Nothing here touches the network: [`request_for`] decides *which*
//! request a command becomes, and [`reply_for_tables`] /
//! [`reply_for_query`] turn a `(status, body)` pair back into a
//! [`Reply`]. The actual `reqwest` calls live in [`crate::worker`], so
//! this module stays trivially testable without a live server.
//!
//! The JSON shapes mirror `docs/api-contract.md` (shared with the
//! dbboard-web sibling): success bodies decode into core types, while a
//! non-2xx response carries `{"error":{"category","message"}}`.

use dbboard_core::{DbError, DbResult, QueryResult, TableInfo};
use serde::Deserialize;

use crate::{Command, Reply};

/// The HTTP shape a [`Command`] maps to. The worker reads this to pick
/// the verb, path, and body without re-matching the command itself.
pub(crate) enum HttpRequest {
    /// `GET /tables`
    GetTables,
    /// `POST /query` with `{"sql": ...}`
    PostQuery(String),
}

/// `GET /tables` success body: `{"tables":[{"schema":null,"name":"users"}]}`.
#[derive(Deserialize)]
struct TablesBody {
    tables: Vec<TableInfo>,
}

/// Error envelope shared with dbboard-web: `{"error":{"category","message"}}`.
#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    category: String,
    message: String,
}

/// Decide which HTTP request a command becomes.
///
/// `SwitchConnection` (ADR-0020) and `AiExplain` / `AiSuggest`
/// (ADR-0023) are intentionally *not* HTTP requests — the worker
/// dispatches each in-process before calling `request_for`. Reaching
/// any panic arm would indicate a bug in the worker's dispatch order
/// rather than user input.
pub(crate) fn request_for(command: &Command) -> HttpRequest {
    match command {
        Command::ListTables => HttpRequest::GetTables,
        Command::Query(sql) => HttpRequest::PostQuery(sql.clone()),
        Command::SwitchConnection { .. } => {
            unreachable!("SwitchConnection is handled in the worker before request_for")
        }
        Command::AiExplain { .. }
        | Command::AiSuggest { .. }
        | Command::AiExplainStream { .. }
        | Command::AiSuggestStream { .. }
        | Command::CancelAiRequest => {
            unreachable!("AI commands are routed to the provider before request_for")
        }
        Command::SwitchAiProvider { .. } => {
            unreachable!("SwitchAiProvider is handled by AiProviderSwitcher before request_for")
        }
        Command::PrefetchSchema { .. } => {
            unreachable!("PrefetchSchema is handled via SchemaSource before request_for")
        }
        Command::DescribeTable { .. } => {
            unreachable!("DescribeTable is handled via SchemaSource before request_for")
        }
    }
}

/// Map a `GET /tables` response into a [`Reply::Tables`].
pub(crate) fn reply_for_tables(status: u16, body: &str) -> Reply {
    Reply::Tables(decode(status, body, |raw: TablesBody| raw.tables))
}

/// Map a `POST /query` response into a [`Reply::QueryResult`].
pub(crate) fn reply_for_query(status: u16, body: &str) -> Reply {
    Reply::QueryResult(decode(status, body, |result: QueryResult| result))
}

/// Shared decode path: on a 2xx, deserialize `T` and project it with
/// `extract`; otherwise read the error envelope. A success body that
/// fails to parse is a contract violation we surface as a connection
/// error rather than silently dropping.
fn decode<T, U>(status: u16, body: &str, extract: impl FnOnce(T) -> U) -> DbResult<U>
where
    T: for<'de> Deserialize<'de>,
{
    if is_success(status) {
        serde_json::from_str::<T>(body)
            .map(extract)
            .map_err(|e| DbError::Connection(format!("malformed response body: {e}")))
    } else {
        Err(error_from_body(status, body))
    }
}

fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Reconstruct the domain error from a non-2xx body. A body that is not
/// the expected envelope still yields a usable error keyed off the
/// status code, so an unexpected response never strands the UI.
fn error_from_body(status: u16, body: &str) -> DbError {
    match serde_json::from_str::<ErrorEnvelope>(body) {
        Ok(envelope) => DbError::from_parts(&envelope.error.category, envelope.error.message),
        Err(_) => DbError::Connection(format!("server returned HTTP {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{reply_for_query, reply_for_tables, request_for, HttpRequest};
    use crate::{Command, Reply};
    use dbboard_core::DbError;

    #[test]
    fn list_tables_maps_to_a_get() {
        assert!(matches!(
            request_for(&Command::ListTables),
            HttpRequest::GetTables
        ));
    }

    #[test]
    fn query_maps_to_a_post_carrying_the_sql() {
        assert!(matches!(
            request_for(&Command::Query("SELECT 1".into())),
            HttpRequest::PostQuery(sql) if sql == "SELECT 1"
        ));
    }

    #[test]
    fn ok_query_body_decodes_into_a_result() {
        let body =
            r#"{"columns":[{"name":"one","declared_type":null}],"rows":[[1]],"rows_affected":0}"#;
        match reply_for_query(200, body) {
            Reply::QueryResult(Ok(result)) => {
                assert_eq!(result.columns[0].name, "one");
                assert_eq!(result.rows.len(), 1);
            }
            other => panic!("expected Ok query result, got {other:?}"),
        }
    }

    #[test]
    fn query_error_envelope_becomes_a_query_error() {
        let body = r#"{"error":{"category":"query","message":"syntax error"}}"#;
        match reply_for_query(400, body) {
            Reply::QueryResult(Err(DbError::Query(msg))) => assert_eq!(msg, "syntax error"),
            other => panic!("expected Err(Query), got {other:?}"),
        }
    }

    #[test]
    fn malformed_success_body_is_a_connection_error() {
        match reply_for_query(200, "not json at all") {
            Reply::QueryResult(Err(DbError::Connection(_))) => {}
            other => panic!("expected Err(Connection), got {other:?}"),
        }
    }

    #[test]
    fn ok_tables_body_decodes_into_table_list() {
        let body = r#"{"tables":[{"schema":null,"name":"users"}]}"#;
        match reply_for_tables(200, body) {
            Reply::Tables(Ok(tables)) => {
                assert_eq!(tables.len(), 1);
                assert_eq!(tables[0].name, "users");
            }
            other => panic!("expected Ok tables, got {other:?}"),
        }
    }

    #[test]
    fn tables_connection_envelope_becomes_a_connection_error() {
        let body = r#"{"error":{"category":"connection","message":"db down"}}"#;
        match reply_for_tables(502, body) {
            Reply::Tables(Err(DbError::Connection(msg))) => assert_eq!(msg, "db down"),
            other => panic!("expected Err(Connection), got {other:?}"),
        }
    }

    #[test]
    fn non_envelope_error_body_falls_back_to_connection_error() {
        // A bare status line with no JSON envelope (e.g. a proxy error)
        // must still surface as a usable error, keyed off the status.
        match reply_for_tables(503, "Service Unavailable") {
            Reply::Tables(Err(DbError::Connection(msg))) => assert!(msg.contains("503")),
            other => panic!("expected Err(Connection) mentioning 503, got {other:?}"),
        }
    }
}
