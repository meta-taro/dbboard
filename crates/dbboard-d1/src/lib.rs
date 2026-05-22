//! Cloudflare D1 adapter for dbboard.
//!
//! Unlike the Turso adapter, D1 has no native driver reachable from a
//! desktop process: Cloudflare only exposes it to outside callers via
//! the REST API (the Workers binding is Worker-only). So this adapter
//! is an HTTP client of `POST /accounts/{account}/d1/database/{db}/raw`
//! that maps Cloudflare's JSON envelope onto the `dbboard-core` domain
//! types. It mirrors `TursoAdapter`'s method surface
//! (`connect` / `ping` / `list_tables` / `query`); the workspace-wide
//! adapter trait is still deferred to Phase 2 (see `docs/roadmap.md`).
//!
//! The `/raw` endpoint is used over `/query` because it preserves
//! column order and returns rows as positional arrays, which is what a
//! result table needs. It also returns the same shape for SELECT and
//! DML, so no statement routing is required.

use dbboard_core::{Column, DbError, DbResult, QueryResult, Row, TableInfo, Value};
use serde::Deserialize;
use serde_json::Value as JsonValue;

const DEFAULT_BASE_URL: &str = "https://api.cloudflare.com/client/v4";

/// Lists user tables, mirroring the Turso adapter's introspection.
const LIST_TABLES_SQL: &str = "SELECT name FROM sqlite_master \
     WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
     ORDER BY name";

/// Connection parameters for a single D1 database.
///
/// `api_token` is a secret: it is never logged, never placed in the
/// request URL, and never embedded in a [`DbError`] message.
pub struct D1Config {
    pub account_id: String,
    pub database_id: String,
    pub api_token: String,
    /// API root. `None` uses Cloudflare's production endpoint; tests
    /// and self-hosted gateways can point it elsewhere.
    pub base_url: Option<String>,
}

pub struct D1Adapter {
    client: reqwest::Client,
    raw_url: String,
    // Kept private and never surfaced in Debug or errors.
    token: String,
}

impl D1Adapter {
    /// Build an adapter for a D1 database. This does not perform any
    /// network I/O — call [`Self::ping`] to verify connectivity.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] when the API token is empty or
    /// when the HTTP client cannot be constructed (e.g. the TLS backend
    /// fails to initialise).
    pub fn connect(config: D1Config) -> DbResult<Self> {
        // Fail fast on a blank token (e.g. `DBBOARD_D1_TOKEN` exported
        // but empty) rather than sending a useless `Bearer ` header and
        // waiting for a 401.
        if config.api_token.trim().is_empty() {
            return Err(DbError::Connection("D1 API token is empty".to_string()));
        }

        let client = reqwest::Client::builder()
            // Pin rustls explicitly and refuse plaintext: the bearer
            // token must never travel over a non-TLS connection, even
            // if `base_url` is mistakenly set to an `http://` URL.
            .use_rustls_tls()
            .https_only(true)
            .build()
            .map_err(|e| DbError::Connection(e.to_string()))?;
        let base = config.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);
        let raw_url = build_raw_url(base, &config.account_id, &config.database_id);
        Ok(Self {
            client,
            raw_url,
            token: config.api_token,
        })
    }

    /// Cheap liveness probe: runs `SELECT 1` and discards the result.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] on transport/auth failure or
    /// [`DbError::Query`] when the probe statement is rejected.
    pub async fn ping(&self) -> DbResult<()> {
        self.post_raw("SELECT 1").await.map(|_| ())
    }

    /// List user tables (everything in `sqlite_master` that is not a
    /// `sqlite_*` internal table), ascending by name.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Schema`] when the introspection query fails.
    pub async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        // Re-tag the failure category: a failed introspection query is
        // a schema error to the rest of the system, not a user query.
        let envelope = self
            .post_raw(LIST_TABLES_SQL)
            .await
            .map_err(reclassify_schema)?;
        envelope_to_tables(envelope)
    }

    /// Execute a SQL statement against D1 and collect the result.
    ///
    /// `/raw` returns the same envelope for SELECT and DML, so there is
    /// no need to route by statement kind: rows come from
    /// `results.rows` and the affected count from `meta.changes`.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`], [`DbError::Query`], or
    /// [`DbError::TypeConversion`] depending on the failure mode.
    pub async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        let envelope = self.post_raw(sql).await?;
        envelope_to_query_result(envelope)
    }

    /// POST a single statement to the `/raw` endpoint and return the
    /// parsed, success-checked envelope.
    async fn post_raw(&self, sql: &str) -> DbResult<D1Envelope> {
        let response = self
            .client
            .post(&self.raw_url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "sql": sql, "params": [] }))
            .send()
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;

        let envelope: D1Envelope = serde_json::from_str(&body).map_err(|e| {
            // A non-JSON body (e.g. a Cloudflare HTML 5xx page) would
            // otherwise dump an unbounded string into the UI.
            DbError::Query(format!(
                "malformed D1 response (status {status}): {} [{}]",
                e,
                truncate(&body)
            ))
        })?;

        if envelope.success {
            Ok(envelope)
        } else {
            Err(error_from_response(status, &envelope.errors))
        }
    }
}

fn build_raw_url(base: &str, account_id: &str, database_id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/accounts/{account_id}/d1/database/{database_id}/raw")
}

/// Cloudflare's standard API envelope, narrowed to the fields we use.
#[derive(Debug, Deserialize)]
struct D1Envelope {
    success: bool,
    #[serde(default)]
    result: Vec<D1QueryResult>,
    #[serde(default)]
    errors: Vec<D1ApiError>,
}

#[derive(Debug, Deserialize)]
struct D1ApiError {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct D1QueryResult {
    #[serde(default)]
    results: D1Results,
    #[serde(default)]
    meta: D1Meta,
}

#[derive(Debug, Default, Deserialize)]
struct D1Results {
    #[serde(default)]
    columns: Vec<String>,
    #[serde(default)]
    rows: Vec<Vec<JsonValue>>,
}

#[derive(Debug, Default, Deserialize)]
struct D1Meta {
    /// Rows changed by a DML statement. 0 for SELECT.
    #[serde(default)]
    changes: u64,
}

/// Map the first statement result onto a [`QueryResult`]. D1's `/raw`
/// does not report per-column declared types, so [`Column::declared_type`]
/// is always `None` — the same convention SQLite expressions use.
fn envelope_to_query_result(envelope: D1Envelope) -> DbResult<QueryResult> {
    let first = envelope
        .result
        .into_iter()
        .next()
        .ok_or_else(|| DbError::Query("D1 returned no statement result".to_string()))?;

    let columns = first
        .results
        .columns
        .into_iter()
        .map(|name| Column {
            name,
            declared_type: None,
        })
        .collect();

    let mut rows = Vec::with_capacity(first.results.rows.len());
    for raw_row in first.results.rows {
        let mut values = Vec::with_capacity(raw_row.len());
        for cell in raw_row {
            values.push(convert_json_value(cell)?);
        }
        rows.push(Row::new(values));
    }

    Ok(QueryResult {
        columns,
        rows,
        rows_affected: first.meta.changes,
    })
}

/// Extract table names from a `SELECT name FROM sqlite_master` result.
fn envelope_to_tables(envelope: D1Envelope) -> DbResult<Vec<TableInfo>> {
    let result = envelope_to_query_result(envelope).map_err(reclassify_schema)?;
    result
        .rows
        .iter()
        .map(|row| match row.get(0) {
            Some(Value::Text(name)) => Ok(TableInfo::unqualified(name.clone())),
            other => Err(DbError::Schema(format!(
                "expected a text table name, got {other:?}"
            ))),
        })
        .collect()
}

/// Convert one JSON cell from a `/raw` row into a domain [`Value`].
///
/// SQLite storage classes map onto JSON as: null→Null, integers→Integer,
/// reals→Real, text→Text, and BLOBs as arrays of byte-valued integers.
/// Booleans are not a SQLite storage class but are mapped to `Integer`
/// (`0`/`1`) defensively.
fn convert_json_value(value: JsonValue) -> DbResult<Value> {
    match value {
        JsonValue::Null => Ok(Value::Null),
        JsonValue::Bool(b) => Ok(Value::Integer(i64::from(b))),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Real(f))
            } else {
                // Unreachable with serde_json's default (non-arbitrary-
                // precision) numbers, where `as_f64` always succeeds.
                // Kept as a defensive guard in case that ever changes.
                Err(DbError::TypeConversion(format!("number out of range: {n}")))
            }
        }
        JsonValue::String(s) => Ok(Value::Text(s)),
        JsonValue::Array(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                let byte = item
                    .as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| {
                        DbError::TypeConversion(format!(
                            "expected a byte (0-255) in BLOB array, got {item}"
                        ))
                    })?;
                bytes.push(byte);
            }
            Ok(Value::Blob(bytes))
        }
        JsonValue::Object(_) => Err(DbError::TypeConversion(
            "unexpected JSON object in result row".to_string(),
        )),
    }
}

/// Cap on error/body text surfaced into a [`DbError`], so a hostile or
/// malformed response cannot dump an unbounded string into the UI.
const MAX_ERROR_DETAIL: usize = 2048;

/// Turn an API failure into a domain error, classified by HTTP status:
///
/// - `401`/`403` → [`DbError::Connection`] (authentication).
/// - `429` and `5xx` → [`DbError::Connection`] (rate-limit / outage are
///   transient infrastructure failures, not a problem with the query).
/// - everything else → [`DbError::Query`] (e.g. a SQL error returned
///   with HTTP 400).
///
/// Cloudflare error messages are safe to surface; the API token is never
/// echoed back in them.
fn error_from_response(status: u16, errors: &[D1ApiError]) -> DbError {
    let detail = join_errors(errors);
    match status {
        401 | 403 => DbError::Connection(format!("authentication failed: {detail}")),
        429 | 500..=599 => {
            DbError::Connection(format!("D1 unavailable (status {status}): {detail}"))
        }
        _ => DbError::Query(detail),
    }
}

fn join_errors(errors: &[D1ApiError]) -> String {
    if errors.is_empty() {
        return "D1 request failed without an error message".to_string();
    }
    let joined = errors
        .iter()
        .map(|e| match e.code {
            Some(code) => format!("[{code}] {}", e.message),
            None => e.message.clone(),
        })
        .collect::<Vec<_>>()
        .join("; ");
    truncate(&joined)
}

/// Truncate `text` to [`MAX_ERROR_DETAIL`] bytes on a char boundary,
/// appending an ellipsis when shortened.
fn truncate(text: &str) -> String {
    if text.len() <= MAX_ERROR_DETAIL {
        return text.to_string();
    }
    let mut end = MAX_ERROR_DETAIL;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// Re-tag a `Query`/`Connection` failure raised during introspection as
/// a `Schema` error, leaving connection failures intact.
fn reclassify_schema(err: DbError) -> DbError {
    match err {
        DbError::Query(msg) | DbError::TypeConversion(msg) => DbError::Schema(msg),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_raw_url, convert_json_value, envelope_to_query_result, envelope_to_tables,
        error_from_response, reclassify_schema, D1ApiError, D1Envelope,
    };
    use dbboard_core::{DbError, Value};
    use serde_json::json;

    fn parse(body: serde_json::Value) -> D1Envelope {
        serde_json::from_value(body).expect("valid D1 envelope")
    }

    #[test]
    fn build_raw_url_targets_the_raw_endpoint() {
        assert_eq!(
            build_raw_url("https://api.cloudflare.com/client/v4", "acc", "db"),
            "https://api.cloudflare.com/client/v4/accounts/acc/d1/database/db/raw"
        );
    }

    #[test]
    fn build_raw_url_trims_a_trailing_slash() {
        assert_eq!(
            build_raw_url("https://example.test/", "a", "b"),
            "https://example.test/accounts/a/d1/database/b/raw"
        );
    }

    #[test]
    fn convert_null() {
        assert_eq!(convert_json_value(json!(null)).unwrap(), Value::Null);
    }

    #[test]
    fn convert_bool_becomes_integer() {
        assert_eq!(convert_json_value(json!(true)).unwrap(), Value::Integer(1));
        assert_eq!(convert_json_value(json!(false)).unwrap(), Value::Integer(0));
    }

    #[test]
    fn convert_integer_and_real() {
        assert_eq!(convert_json_value(json!(42)).unwrap(), Value::Integer(42));
        assert_eq!(convert_json_value(json!(-7)).unwrap(), Value::Integer(-7));
        assert_eq!(convert_json_value(json!(1.5)).unwrap(), Value::Real(1.5));
    }

    #[test]
    fn convert_string() {
        assert_eq!(
            convert_json_value(json!("hi")).unwrap(),
            Value::Text("hi".into())
        );
    }

    #[test]
    fn convert_byte_array_becomes_blob() {
        assert_eq!(
            convert_json_value(json!([1, 2, 255])).unwrap(),
            Value::Blob(vec![1, 2, 255])
        );
        assert_eq!(convert_json_value(json!([])).unwrap(), Value::Blob(vec![]));
    }

    #[test]
    fn convert_out_of_range_blob_byte_errors() {
        let err = convert_json_value(json!([256])).unwrap_err();
        assert!(matches!(err, DbError::TypeConversion(_)));
    }

    #[test]
    fn convert_object_errors() {
        let err = convert_json_value(json!({"a": 1})).unwrap_err();
        assert!(matches!(err, DbError::TypeConversion(_)));
    }

    #[test]
    fn query_result_maps_columns_rows_and_affected() {
        let envelope = parse(json!({
            "success": true,
            "result": [{
                "results": { "columns": ["id", "name"], "rows": [[1, "a"], [2, "b"]] },
                "meta": { "changes": 0 },
                "success": true
            }],
            "errors": []
        }));
        let result = envelope_to_query_result(envelope).unwrap();
        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0].name, "id");
        assert!(result.columns[0].declared_type.is_none());
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0].get(0), Some(&Value::Integer(1)));
        assert_eq!(result.rows[1].get(1), Some(&Value::Text("b".into())));
        assert_eq!(result.rows_affected, 0);
    }

    #[test]
    fn query_result_reads_affected_from_meta_changes() {
        let envelope = parse(json!({
            "success": true,
            "result": [{
                "results": { "columns": [], "rows": [] },
                "meta": { "changes": 3 },
                "success": true
            }],
            "errors": []
        }));
        let result = envelope_to_query_result(envelope).unwrap();
        assert!(result.columns.is_empty());
        assert!(result.rows.is_empty());
        assert_eq!(result.rows_affected, 3);
    }

    #[test]
    fn empty_result_array_is_a_query_error() {
        let envelope = parse(json!({ "success": true, "result": [], "errors": [] }));
        assert!(matches!(
            envelope_to_query_result(envelope),
            Err(DbError::Query(_))
        ));
    }

    #[test]
    fn tables_extracts_names_in_order() {
        let envelope = parse(json!({
            "success": true,
            "result": [{
                "results": { "columns": ["name"], "rows": [["users"], ["orders"]] },
                "meta": {},
                "success": true
            }],
            "errors": []
        }));
        let tables = envelope_to_tables(envelope).unwrap();
        let names: Vec<_> = tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["users", "orders"]);
        assert!(tables.iter().all(|t| t.schema.is_none()));
    }

    #[test]
    fn auth_status_maps_to_connection_error() {
        let errors = vec![D1ApiError {
            code: Some(10000),
            message: "Authentication error".into(),
        }];
        let err = error_from_response(403, &errors);
        match err {
            DbError::Connection(msg) => assert!(msg.contains("Authentication error")),
            other => panic!("expected Connection, got {other:?}"),
        }
    }

    #[test]
    fn sql_error_maps_to_query_error_with_message() {
        let errors = vec![D1ApiError {
            code: Some(7500),
            message: "no such table: ghost".into(),
        }];
        let err = error_from_response(200, &errors);
        match err {
            DbError::Query(msg) => assert!(msg.contains("no such table: ghost")),
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[test]
    fn rate_limit_and_server_errors_map_to_connection() {
        for status in [429, 500, 503] {
            let err = error_from_response(status, &[]);
            assert!(
                matches!(err, DbError::Connection(_)),
                "status {status} should be a connection error"
            );
        }
    }

    #[test]
    fn tables_rejects_non_text_name_cell() {
        let envelope = parse(json!({
            "success": true,
            "result": [{
                "results": { "columns": ["name"], "rows": [[42]] },
                "meta": {},
                "success": true
            }],
            "errors": []
        }));
        assert!(matches!(
            envelope_to_tables(envelope),
            Err(DbError::Schema(_))
        ));
    }

    #[test]
    fn reclassify_passes_connection_errors_through() {
        // A transport failure during introspection stays a connection
        // error; only query/type failures are re-tagged as schema.
        let passed = reclassify_schema(DbError::Connection("down".into()));
        assert!(matches!(passed, DbError::Connection(_)));
        let retagged = reclassify_schema(DbError::Query("boom".into()));
        assert!(matches!(retagged, DbError::Schema(_)));
    }

    #[test]
    fn blob_array_rejects_null_item() {
        let err = convert_json_value(json!([1, null, 2])).unwrap_err();
        assert!(matches!(err, DbError::TypeConversion(_)));
    }
}
