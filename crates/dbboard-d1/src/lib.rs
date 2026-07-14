//! Cloudflare D1 adapter for dbboard.
//!
//! Unlike the Turso adapter, D1 has no native driver reachable from a
//! desktop process: Cloudflare only exposes it to outside callers via
//! the REST API (the Workers binding is Worker-only). So this adapter
//! is an HTTP client of `POST /accounts/{account}/d1/database/{db}/raw`
//! that maps Cloudflare's JSON envelope onto the `dbboard-core` domain
//! types. Implements the workspace-wide [`DatabaseAdapter`] contract
//! (ADR-0012); Phase 2 advertises no optional capabilities.
//!
//! The `/raw` endpoint is used over `/query` because it preserves
//! column order and returns rows as positional arrays, which is what a
//! result table needs. It also returns the same shape for SELECT and
//! DML, so no statement routing is required.

use async_trait::async_trait;
use dbboard_core::{
    too_many_rows_error, Capabilities, Column, ColumnInfo, DatabaseAdapter, DbError, DbResult,
    QueryResult, Row, TableInfo, TableSchema, Value, MAX_RESULT_ROWS,
};
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

    /// POST a single statement to the `/raw` endpoint and return the
    /// parsed, success-checked envelope.
    async fn post_raw(&self, sql: &str) -> DbResult<D1Envelope> {
        // reqwest's `Error::Display` echoes the full request URL — which
        // for D1 carries the account and database IDs — back into the
        // message. `without_url` strips it before we surface anything.
        let response = self
            .client
            .post(&self.raw_url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "sql": sql, "params": [] }))
            .send()
            .await
            .map_err(transport_error)?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(transport_error)?;

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

#[async_trait]
impl DatabaseAdapter for D1Adapter {
    fn id(&self) -> &'static str {
        "d1"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        self.post_raw("SELECT 1").await.map(|_| ())
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        // Re-tag the failure category: a failed introspection query is
        // a schema error to the rest of the system, not a user query.
        let envelope = self
            .post_raw(LIST_TABLES_SQL)
            .await
            .map_err(reclassify_schema)?;
        envelope_to_tables(envelope)
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        // `/raw` returns the same envelope for SELECT and DML, so the
        // statement kind doesn't need to be routed: rows come from
        // `results.rows` and the affected count from `meta.changes`.
        let envelope = self.post_raw(sql).await?;
        envelope_to_query_result(envelope)
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        // Same PRAGMA as the Turso adapter, over the /raw envelope.
        // PRAGMA arguments cannot be bound as parameters, so the name is
        // embedded with single quotes doubled (SQLite string-literal
        // escaping).
        let escaped = table.name.replace('\'', "''");
        let envelope = self
            .post_raw(&format!("PRAGMA table_info('{escaped}')"))
            .await?;
        envelope_to_table_schema(table, envelope)
    }
}

fn build_raw_url(base: &str, account_id: &str, database_id: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/accounts/{account_id}/d1/database/{database_id}/raw")
}

/// Turn a reqwest transport failure into a [`DbError::Connection`] with
/// the request URL stripped (so the account/database IDs do not leak
/// into the HTTP error envelope or any log line).
fn transport_error(err: reqwest::Error) -> DbError {
    DbError::Connection(truncate(&err.without_url().to_string()))
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

    // Refuse to expand a result set past the workspace-wide cap. D1's
    // envelope hands us the row array in full, so the check is upfront.
    if first.results.rows.len() > MAX_RESULT_ROWS {
        return Err(too_many_rows_error());
    }

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

/// Map a `PRAGMA table_info` envelope (`cid, name, type, notnull,
/// dflt_value, pk`) onto a [`TableSchema`]. Columns are located by name
/// rather than position so a reordered envelope still maps correctly.
fn envelope_to_table_schema(table: &TableInfo, envelope: D1Envelope) -> DbResult<TableSchema> {
    let result = envelope_to_query_result(envelope)?;

    // PRAGMA table_info returns zero rows for a missing table rather
    // than an engine error, so synthesise SQLite's own message shape to
    // satisfy the ADR-0028 "missing table is DbError::Query" rule.
    if result.rows.is_empty() {
        return Err(DbError::Query(format!("no such table: {}", table.name)));
    }

    let position = |field: &str| -> DbResult<usize> {
        result
            .columns
            .iter()
            .position(|c| c.name == field)
            .ok_or_else(|| {
                DbError::Schema(format!(
                    "PRAGMA table_info result is missing the '{field}' column"
                ))
            })
    };
    let cid_at = position("cid")?;
    let name_at = position("name")?;
    let type_at = position("type")?;
    let notnull_at = position("notnull")?;
    let dflt_at = position("dflt_value")?;
    let pk_at = position("pk")?;

    let mut columns = Vec::with_capacity(result.rows.len());
    // (pk position, column name) — collected out of order, sorted below.
    let mut pk_parts: Vec<(i64, String)> = Vec::new();
    for row in &result.rows {
        let cid = pragma_int(row, cid_at, "cid")?;
        let name = pragma_text(row, name_at, "name")?;
        let declared = pragma_text(row, type_at, "type")?;
        let notnull = pragma_int(row, notnull_at, "notnull")?;
        let default_value = match row.get(dflt_at) {
            Some(Value::Null) | None => None,
            Some(Value::Text(s)) => Some(s.clone()),
            Some(other) => Some(other.to_string()),
        };
        let pk = pragma_int(row, pk_at, "pk")?;

        if pk > 0 {
            pk_parts.push((pk, name.clone()));
        }
        let ordinal = u32::try_from(cid)
            .map_err(|_| DbError::TypeConversion(format!("negative PRAGMA cid: {cid}")))?
            + 1; // cid is 0-based; ColumnInfo::ordinal is 1-based (ADR-0028).

        columns.push(ColumnInfo {
            name,
            // Typeless SQLite columns report an empty string.
            declared_type: (!declared.is_empty()).then_some(declared),
            nullable: notnull == 0,
            primary_key: pk > 0,
            ordinal,
            default_value,
            // SQLite has no column-comment concept (ADR-0037).
            comment: None,
        });
    }

    pk_parts.sort_by_key(|&(key_position, _)| key_position);
    Ok(TableSchema {
        table: table.clone(),
        columns,
        primary_key: pk_parts.into_iter().map(|(_, name)| name).collect(),
    })
}

fn pragma_int(row: &Row, at: usize, field: &str) -> DbResult<i64> {
    match row.get(at) {
        Some(Value::Integer(n)) => Ok(*n),
        other => Err(DbError::TypeConversion(format!(
            "expected an integer '{field}' in a PRAGMA table_info row, got {other:?}"
        ))),
    }
}

fn pragma_text(row: &Row, at: usize, field: &str) -> DbResult<String> {
    match row.get(at) {
        Some(Value::Text(s)) => Ok(s.clone()),
        other => Err(DbError::TypeConversion(format!(
            "expected a text '{field}' in a PRAGMA table_info row, got {other:?}"
        ))),
    }
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
        build_raw_url, convert_json_value, envelope_to_query_result, envelope_to_table_schema,
        envelope_to_tables, error_from_response, reclassify_schema, D1Adapter, D1ApiError,
        D1Config, D1Envelope,
    };
    use dbboard_core::{DatabaseAdapter, DbError, TableInfo, Value};
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

    fn pragma_envelope(rows: &serde_json::Value) -> D1Envelope {
        parse(json!({
            "success": true,
            "result": [{
                "results": {
                    "columns": ["cid", "name", "type", "notnull", "dflt_value", "pk"],
                    "rows": rows
                },
                "meta": { "changes": 0 },
                "success": true
            }],
            "errors": []
        }))
    }

    #[test]
    fn connect_advertises_describe_table_capability() {
        let adapter = D1Adapter::connect(D1Config {
            account_id: "acc".into(),
            database_id: "db".into(),
            api_token: "token".into(),
            base_url: None,
        })
        .expect("build adapter");
        assert!(adapter.capabilities().has_describe_table);
    }

    #[test]
    fn table_schema_maps_pragma_rows_with_composite_pk_in_key_order() {
        // Declaration order: sku, order_id, line_no — but the composite
        // key order is (order_id=1, line_no=2), which must win.
        let envelope = pragma_envelope(&json!([
            [0, "sku", "TEXT", 1, "'unknown'", 0],
            [1, "order_id", "INTEGER", 0, null, 1],
            [2, "line_no", "INTEGER", 0, null, 2]
        ]));
        let table = TableInfo::unqualified("order_items");
        let schema = envelope_to_table_schema(&table, envelope).expect("map schema");

        assert_eq!(schema.table, table);
        let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["sku", "order_id", "line_no"]);
        // cid is 0-based; ordinal is normalised to 1-based.
        let ordinals: Vec<u32> = schema.columns.iter().map(|c| c.ordinal).collect();
        assert_eq!(ordinals, vec![1, 2, 3]);

        let sku = &schema.columns[0];
        assert!(!sku.nullable);
        assert!(!sku.primary_key);
        assert_eq!(sku.default_value.as_deref(), Some("'unknown'"));

        assert!(schema.columns[1].nullable);
        assert!(schema.columns[1].primary_key);
        assert_eq!(schema.columns[1].default_value, None);

        assert_eq!(
            schema.primary_key,
            vec!["order_id".to_owned(), "line_no".to_owned()]
        );
    }

    #[test]
    fn table_schema_maps_empty_declared_type_to_none() {
        // Typeless SQLite columns report "" from PRAGMA table_info.
        let envelope = pragma_envelope(&json!([[0, "anything", "", 0, null, 0]]));
        let table = TableInfo::unqualified("loose");
        let schema = envelope_to_table_schema(&table, envelope).expect("map schema");
        assert_eq!(schema.columns[0].declared_type, None);
        assert!(schema.primary_key.is_empty());
    }

    #[test]
    fn table_schema_for_missing_table_is_a_query_error() {
        // PRAGMA table_info on a missing table returns zero rows rather
        // than an engine error; the adapter synthesises SQLite's message.
        let envelope = pragma_envelope(&json!([]));
        let err = envelope_to_table_schema(&TableInfo::unqualified("ghost"), envelope)
            .expect_err("missing table should fail");
        let DbError::Query(msg) = err else {
            panic!("expected DbError::Query, got {err:?}");
        };
        assert!(
            msg.contains("no such table") && msg.contains("ghost"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn blob_array_rejects_null_item() {
        let err = convert_json_value(json!([1, null, 2])).unwrap_err();
        assert!(matches!(err, DbError::TypeConversion(_)));
    }

    /// Exactly at the cap — `MAX_RESULT_ROWS` rows in the envelope must
    /// round-trip successfully.
    #[test]
    fn envelope_at_the_row_cap_succeeds() {
        use dbboard_core::MAX_RESULT_ROWS;
        let rows: Vec<serde_json::Value> = (0..MAX_RESULT_ROWS).map(|i| json!([i])).collect();
        let envelope = parse(json!({
            "success": true,
            "result": [{
                "results": { "columns": ["n"], "rows": rows },
                "meta": { "changes": 0 },
                "success": true
            }],
            "errors": []
        }));
        let result = envelope_to_query_result(envelope).expect("at cap should succeed");
        assert_eq!(result.rows.len(), MAX_RESULT_ROWS);
    }

    /// One past the cap must short-circuit before any cell is converted.
    /// The error message must reference the cap so the UI can guide the
    /// user to add a `LIMIT` clause.
    #[test]
    fn envelope_over_the_row_cap_is_a_query_error() {
        use dbboard_core::MAX_RESULT_ROWS;
        let rows: Vec<serde_json::Value> = (0..=MAX_RESULT_ROWS).map(|i| json!([i])).collect();
        let envelope = parse(json!({
            "success": true,
            "result": [{
                "results": { "columns": ["n"], "rows": rows },
                "meta": { "changes": 0 },
                "success": true
            }],
            "errors": []
        }));
        match envelope_to_query_result(envelope) {
            Err(DbError::Query(msg)) => assert!(
                msg.contains(&MAX_RESULT_ROWS.to_string()),
                "error should mention the cap, got: {msg}"
            ),
            other => panic!("expected Query error, got {other:?}"),
        }
    }
}
