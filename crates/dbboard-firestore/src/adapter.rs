//! The `DatabaseAdapter` implementation.
//!
//! Two things here are Firestore-specific enough to be worth stating up front.
//!
//! **The query text is JSON, not SQL.** `query` takes a Firestore
//! `StructuredQuery` — the same object Google's REST docs show — so
//! [`FirestoreAdapter::query_read_only`] must override the trait default,
//! which would otherwise hand the JSON to the SQL classifier and refuse every
//! query. There is nothing to classify: read-only is structural here
//! (ADR-0091 §3), enforced by [`ReadEndpoint`] having no write variant.
//!
//! **A collection has no schema**, so `describe_table` samples and says so —
//! see [`crate::sample`].

use async_trait::async_trait;
use dbboard_core::{
    Capabilities, DatabaseAdapter, DbError, DbResult, QueryResult, TableInfo, TableSchema,
    MAX_RESULT_ROWS,
};
use serde_json::{json, Value as JsonValue};

use crate::auth::Auth;
use crate::credentials::ServiceAccount;
use crate::document;
use crate::endpoint::ReadEndpoint;
use crate::sample;

/// Google's production endpoint, used when a connection names no other.
pub const DEFAULT_BASE_URL: &str = "https://firestore.googleapis.com/v1";

/// A project with no `database_id` uses Firestore's implicit default database,
/// which is spelled `(default)` — literally, parentheses included.
const DEFAULT_DATABASE_ID: &str = "(default)";

/// Collections requested per `:listCollectionIds` page. Firestore caps the
/// page size server-side; this only bounds one round trip, and
/// [`FirestoreAdapter::list_tables`] follows the page token to the end.
const COLLECTION_PAGE_SIZE: usize = 300;

/// Documents read to infer a collection's shape. Large enough that an
/// occasional field shows up, small enough that describing a table stays one
/// cheap request against a collection of any size.
const SCHEMA_SAMPLE_SIZE: usize = 100;

/// How the adapter proves it may read the database.
#[derive(Debug, Clone)]
pub enum FirestoreCredentials {
    /// The JSON key file of a service account, verbatim.
    ServiceAccountJson(String),
    /// The local emulator, which accepts the fixed token `owner` and issues
    /// none of its own.
    Emulator,
}

#[derive(Debug, Clone)]
pub struct FirestoreConfig {
    pub project_id: String,
    /// The named database, or `None` for the project's default one.
    pub database_id: Option<String>,
    pub credentials: FirestoreCredentials,
    /// API root, for the emulator or a test server. `None` means
    /// [`DEFAULT_BASE_URL`].
    pub base_url: Option<String>,
}

pub struct FirestoreAdapter {
    http: reqwest::Client,
    auth: Auth,
    base_url: String,
    documents_root: String,
}

/// Hand-written so the credential never reaches a log line: `auth` holds a
/// signing key and, once a query has run, a live access token.
impl std::fmt::Debug for FirestoreAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirestoreAdapter")
            .field("base_url", &self.base_url)
            .field("documents_root", &self.documents_root)
            .finish_non_exhaustive()
    }
}

/// A request that did not come back with a usable body.
///
/// Kept separate from [`DbError`] because the same failure means different
/// things depending on what asked: a refusal during `ping` is a connection
/// problem, while a 400 during `query` is the caller's query being wrong.
struct HttpFailure {
    status: Option<reqwest::StatusCode>,
    detail: String,
}

impl HttpFailure {
    fn message(&self) -> String {
        match self.status {
            Some(status) => format!("Firestore returned HTTP {status}: {}", self.detail),
            None => self.detail.clone(),
        }
    }

    /// Whether the server said the *request* was malformed, as opposed to
    /// refusing it or being unreachable. Only `400 INVALID_ARGUMENT` says
    /// that; 403/404/429 are all about the connection or the target, not the
    /// query text.
    fn blames_the_request(&self) -> bool {
        self.status == Some(reqwest::StatusCode::BAD_REQUEST)
    }
}

impl FirestoreAdapter {
    /// Build an adapter from `config`. Performs no I/O — call
    /// [`ping`](DatabaseAdapter::ping) to find out whether the credentials
    /// actually work.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] if the service-account key does not
    /// parse, if it would be sent over plain HTTP, or if the HTTP client
    /// cannot be built.
    pub fn connect(config: FirestoreConfig) -> DbResult<Self> {
        let base_url = config
            .base_url
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();
        let plain_http = base_url.starts_with("http://");

        // An access token is a bearer credential for the whole database, so it
        // is never worth sending in clear text — and a mistyped base URL is
        // not a good enough reason to make an exception. The emulator is
        // exempt because it issues no credential to leak.
        if plain_http
            && matches!(
                config.credentials,
                FirestoreCredentials::ServiceAccountJson(_)
            )
        {
            return Err(DbError::Connection(format!(
                "refusing to send a service-account token over plain http to {base_url}; use https, or connect to the emulator"
            )));
        }

        let http = reqwest::Client::builder()
            // Also blocks an https → http redirect, which would defeat the
            // check above.
            .https_only(!plain_http)
            .build()
            .map_err(|e| DbError::Connection(format!("could not build an HTTP client: {e}")))?;

        let auth = match config.credentials {
            FirestoreCredentials::ServiceAccountJson(raw) => {
                Auth::service_account(ServiceAccount::from_json(&raw)?, http.clone())
            }
            FirestoreCredentials::Emulator => Auth::Emulator,
        };

        let database = config
            .database_id
            .unwrap_or_else(|| DEFAULT_DATABASE_ID.to_owned());
        let documents_root = format!(
            "projects/{}/databases/{database}/documents",
            config.project_id
        );

        Ok(Self {
            http,
            auth,
            base_url,
            documents_root,
        })
    }

    /// Send one read request and parse the JSON body.
    async fn send(
        &self,
        endpoint: &ReadEndpoint,
        body: Option<JsonValue>,
        query: &[(&str, String)],
    ) -> Result<JsonValue, HttpFailure> {
        debug_assert!(endpoint.is_read(), "only read endpoints are reachable");

        let bearer = self.auth.bearer().await.map_err(|e| HttpFailure {
            status: None,
            detail: e.message().to_owned(),
        })?;

        let url = format!("{}/{}", self.base_url, endpoint.path(&self.documents_root));
        let mut request = self
            .http
            .request(endpoint.method(), &url)
            .header(reqwest::header::AUTHORIZATION, bearer)
            .query(query);
        if let Some(body) = body {
            request = request.json(&body);
        }

        let response = request.send().await.map_err(|e| HttpFailure {
            status: None,
            // `without_url` keeps the project and database out of the message;
            // the caller already knows which connection it used.
            detail: format!("could not reach Firestore: {}", e.without_url()),
        })?;

        let status = response.status();
        let text = response.text().await.map_err(|e| HttpFailure {
            status: Some(status),
            detail: format!("the response body could not be read: {}", e.without_url()),
        })?;

        if !status.is_success() {
            return Err(HttpFailure {
                status: Some(status),
                detail: text.trim().to_owned(),
            });
        }

        // An empty 200 is how Firestore answers "nothing here" for some
        // endpoints; treat it as an empty object rather than a parse failure.
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).map_err(|e| HttpFailure {
            status: Some(status),
            detail: format!("the response was not valid JSON: {e}"),
        })
    }

    /// Wrap a bare `StructuredQuery` in the envelope `:runQuery` expects.
    ///
    /// Both forms are accepted so a request copied from Google's docs works
    /// unedited and so does the shorter object on its own.
    fn run_query_body(sql: &str) -> DbResult<JsonValue> {
        let parsed: JsonValue = serde_json::from_str(sql).map_err(|e| {
            DbError::Query(format!(
                "a Firestore query is a StructuredQuery in JSON, not SQL; this did not parse as JSON: {e}"
            ))
        })?;
        let Some(object) = parsed.as_object() else {
            return Err(DbError::Query(
                "a Firestore query must be a JSON object holding a StructuredQuery".to_owned(),
            ));
        };
        if object.contains_key("structuredQuery") {
            return Ok(parsed);
        }
        Ok(json!({ "structuredQuery": parsed }))
    }

    /// The documents in a `:runQuery` response.
    ///
    /// The stream interleaves bookkeeping entries (`readTime`,
    /// `skippedResults`, `done`) with the results; those carry no `document`
    /// key and are not empty documents, so they are skipped rather than
    /// turned into blank rows.
    fn documents_of(response: &JsonValue) -> Vec<JsonValue> {
        response
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| entry.get("document").cloned())
            .collect()
    }

    async fn run_query(&self, sql: &str) -> DbResult<Vec<JsonValue>> {
        let body = Self::run_query_body(sql)?;
        let response = self
            .send(&ReadEndpoint::RunQuery, Some(body), &[])
            .await
            .map_err(|f| {
                if f.blames_the_request() {
                    DbError::Query(f.message())
                } else {
                    DbError::Connection(f.message())
                }
            })?;
        Ok(Self::documents_of(&response))
    }
}

#[async_trait]
impl DatabaseAdapter for FirestoreAdapter {
    fn id(&self) -> &'static str {
        "firestore"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            // Everything else stays off. Firestore has no views, functions, or
            // foreign keys to introspect, and this adapter cannot write at all
            // — advertising `execute` and then refusing it is worse than never
            // offering it.
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        self.send(
            &ReadEndpoint::ListCollectionIds,
            Some(json!({ "pageSize": 1 })),
            &[],
        )
        .await
        .map(|_| ())
        .map_err(|f| DbError::Connection(f.message()))
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        let mut tables = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut body = json!({ "pageSize": COLLECTION_PAGE_SIZE });
            if let Some(token) = &page_token {
                body["pageToken"] = json!(token);
            }
            let response = self
                .send(&ReadEndpoint::ListCollectionIds, Some(body), &[])
                .await
                .map_err(|f| DbError::Schema(f.message()))?;

            // Firestore omits `collectionIds` entirely for an empty database
            // rather than sending an empty array.
            if let Some(ids) = response.get("collectionIds").and_then(JsonValue::as_array) {
                tables.extend(
                    ids.iter()
                        .filter_map(JsonValue::as_str)
                        .map(TableInfo::unqualified),
                );
            }

            match response.get("nextPageToken").and_then(JsonValue::as_str) {
                Some(token) if !token.is_empty() => page_token = Some(token.to_owned()),
                // A truncated list would hide collections with no sign that
                // anything was missing, so the loop only ends when Firestore
                // says there is no more.
                _ => return Ok(tables),
            }
        }
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        let documents = self.run_query(sql).await?;
        if documents.len() > MAX_RESULT_ROWS {
            return Err(dbboard_core::too_many_rows_error());
        }
        document::to_result(&documents, &self.documents_root)
    }

    /// Read-only, without a classifier.
    ///
    /// The trait default parses `sql` as Postgres. A `StructuredQuery` is
    /// JSON, so that default would fail closed on *every* Firestore query —
    /// and only on the MCP surface, where it would be hardest to notice. It is
    /// also redundant: this crate can build no write URL (ADR-0091 §3), so the
    /// read-only guarantee does not depend on reading the query at all.
    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        let documents = self.run_query(sql).await?;
        // Truncate rather than error — the caller asked for a bounded read.
        let capped = &documents[..documents.len().min(max_rows)];
        document::to_result(capped, &self.documents_root)
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        let response = self
            .send(
                &ReadEndpoint::ListDocuments {
                    collection: table.name.clone(),
                },
                None,
                &[("pageSize", SCHEMA_SAMPLE_SIZE.to_string())],
            )
            .await
            .map_err(|f| DbError::Schema(f.message()))?;

        // As with `collectionIds`, an empty collection omits the key. That is
        // a real answer — the collection exists and holds nothing — so it
        // describes as the one column every document has.
        let documents = response
            .get("documents")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(sample::infer_schema(table, &documents))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::NAME_COLUMN;
    use dbboard_core::Value;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn emulator_config(base_url: &str) -> FirestoreConfig {
        FirestoreConfig {
            project_id: "demo".to_string(),
            database_id: None,
            credentials: FirestoreCredentials::Emulator,
            base_url: Some(base_url.to_string()),
        }
    }

    fn connect_to(server: &MockServer) -> FirestoreAdapter {
        FirestoreAdapter::connect(emulator_config(&server.uri())).unwrap()
    }

    fn document(id: &str, fields: &serde_json::Value) -> serde_json::Value {
        json!({
            "name": format!("projects/demo/databases/(default)/documents/users/{id}"),
            "fields": fields,
        })
    }

    // --- construction -----------------------------------------------------

    #[test]
    fn the_adapter_id_is_stable() {
        let adapter = FirestoreAdapter::connect(emulator_config("http://localhost:8080")).unwrap();
        assert_eq!(adapter.id(), "firestore");
    }

    /// A service-account access token is a bearer credential for the whole
    /// database. Sending it over plain HTTP hands it to anything on the path,
    /// and a typo in a base URL is not a good enough reason to allow it.
    #[test]
    fn a_service_account_refuses_a_plain_http_endpoint() {
        let config = FirestoreConfig {
            project_id: "demo".to_string(),
            database_id: None,
            credentials: FirestoreCredentials::ServiceAccountJson("{}".to_string()),
            base_url: Some("http://firestore.internal/v1".to_string()),
        };
        let err = FirestoreAdapter::connect(config).unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
        assert!(
            err.message().contains("http"),
            "the message should name the problem: {}",
            err.message()
        );
    }

    /// The emulator is the exception: it speaks plain HTTP on localhost and
    /// there is no credential to leak.
    #[test]
    fn the_emulator_may_speak_plain_http() {
        assert!(FirestoreAdapter::connect(emulator_config("http://127.0.0.1:8080")).is_ok());
    }

    #[test]
    fn a_missing_base_url_points_at_google() {
        let config = FirestoreConfig {
            project_id: "demo".to_string(),
            database_id: None,
            credentials: FirestoreCredentials::Emulator,
            base_url: None,
        };
        let adapter = FirestoreAdapter::connect(config).unwrap();
        assert_eq!(adapter.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn an_unnamed_database_is_the_default_one() {
        let adapter = FirestoreAdapter::connect(emulator_config("http://x")).unwrap();
        assert_eq!(
            adapter.documents_root,
            "projects/demo/databases/(default)/documents"
        );
    }

    #[test]
    fn a_named_database_appears_in_the_documents_root() {
        let mut config = emulator_config("http://x");
        config.database_id = Some("analytics".to_string());
        let adapter = FirestoreAdapter::connect(config).unwrap();
        assert_eq!(
            adapter.documents_root,
            "projects/demo/databases/analytics/documents"
        );
    }

    // --- ping -------------------------------------------------------------

    #[tokio::test]
    async fn ping_asks_for_the_collection_list() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/projects/demo/databases/(default)/documents:listCollectionIds",
            ))
            .and(header("authorization", "Bearer owner"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;

        connect_to(&server).ping().await.unwrap();
    }

    #[tokio::test]
    async fn ping_surfaces_a_refusal_as_a_connection_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("permission denied"))
            .mount(&server)
            .await;

        let err = connect_to(&server).ping().await.unwrap_err();
        assert!(matches!(err, DbError::Connection(_)));
        assert!(err.message().contains("403"), "{}", err.message());
    }

    // --- list_tables ------------------------------------------------------

    #[tokio::test]
    async fn list_tables_reports_the_collection_ids() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/projects/demo/databases/(default)/documents:listCollectionIds",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "collectionIds": ["users", "orders"] })),
            )
            .mount(&server)
            .await;

        let tables = connect_to(&server).list_tables().await.unwrap();
        assert_eq!(
            tables,
            vec![
                TableInfo::unqualified("users"),
                TableInfo::unqualified("orders"),
            ]
        );
    }

    /// Firestore omits the key entirely rather than sending an empty array.
    #[tokio::test]
    async fn a_database_with_no_collections_lists_no_tables() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;

        assert!(connect_to(&server).list_tables().await.unwrap().is_empty());
    }

    /// A truncated collection list would silently hide tables — the one
    /// failure mode of `list_tables` a user cannot detect from the result.
    #[tokio::test]
    async fn list_tables_follows_the_page_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_json(json!({ "pageSize": COLLECTION_PAGE_SIZE })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "collectionIds": ["users"], "nextPageToken": "more" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_json(
                json!({ "pageSize": COLLECTION_PAGE_SIZE, "pageToken": "more" }),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "collectionIds": ["orders"] })),
            )
            .mount(&server)
            .await;

        let tables = connect_to(&server).list_tables().await.unwrap();
        assert_eq!(
            tables,
            vec![
                TableInfo::unqualified("users"),
                TableInfo::unqualified("orders"),
            ]
        );
    }

    // --- query ------------------------------------------------------------

    const BARE_QUERY: &str = r#"{"from":[{"collectionId":"users"}]}"#;

    #[tokio::test]
    async fn a_bare_structured_query_is_wrapped_for_the_api() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/projects/demo/databases/(default)/documents:runQuery",
            ))
            .and(body_json(json!({
                "structuredQuery": { "from": [{ "collectionId": "users" }] }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;

        connect_to(&server).query(BARE_QUERY).await.unwrap();
    }

    /// The REST body and the query object differ by one wrapper key. Accepting
    /// both means a request copied from Google's docs works unedited, and so
    /// does the shorter form.
    #[tokio::test]
    async fn an_already_wrapped_query_is_sent_unchanged() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_json(json!({
                "structuredQuery": { "from": [{ "collectionId": "users" }] },
                "readTime": "2026-01-01T00:00:00Z",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;

        let sent = r#"{"structuredQuery":{"from":[{"collectionId":"users"}]},"readTime":"2026-01-01T00:00:00Z"}"#;
        connect_to(&server).query(sent).await.unwrap();
    }

    #[tokio::test]
    async fn query_returns_the_documents_as_rows() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "document": document("a", &json!({ "email": { "stringValue": "a@dbboard.example.com" } })) },
                { "document": document("b", &json!({ "email": { "stringValue": "b@dbboard.example.com" } })) },
            ])))
            .mount(&server)
            .await;

        let result = connect_to(&server).query(BARE_QUERY).await.unwrap();
        assert_eq!(result.rows.len(), 2);
        let names: Vec<&str> = result.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec![NAME_COLUMN, "email"]);
        assert_eq!(
            result.rows[0].values()[0],
            Value::Text("users/a".to_string())
        );
    }

    /// `:runQuery` interleaves bookkeeping entries (`readTime`, `skippedResults`,
    /// `done`) with the results. They are not empty documents — they are not
    /// documents.
    #[tokio::test]
    async fn entries_that_carry_no_document_are_skipped() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "readTime": "2026-01-01T00:00:00Z", "skippedResults": 3 },
                { "document": document("a", &json!({})) },
                { "done": true },
            ])))
            .mount(&server)
            .await;

        assert_eq!(
            connect_to(&server)
                .query(BARE_QUERY)
                .await
                .unwrap()
                .rows
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn a_query_that_is_not_json_is_refused_before_any_request_is_made() {
        let server = MockServer::start().await;
        // No mock is mounted: a request reaching the server would 404, but the
        // point is that none is made.
        let err = connect_to(&server)
            .query("SELECT * FROM users")
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        assert!(
            err.message().to_lowercase().contains("json"),
            "the message should say what the query text must be: {}",
            err.message()
        );
    }

    #[tokio::test]
    async fn a_query_that_is_json_but_not_an_object_is_refused() {
        let server = MockServer::start().await;
        let err = connect_to(&server).query("[1, 2, 3]").await.unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
    }

    #[tokio::test]
    async fn a_result_set_over_the_workspace_cap_is_refused_rather_than_truncated() {
        let server = MockServer::start().await;
        let entries: Vec<serde_json::Value> = (0..=MAX_RESULT_ROWS)
            .map(|i| json!({ "document": document(&i.to_string(), &json!({})) }))
            .collect();
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(entries))
            .mount(&server)
            .await;

        let err = connect_to(&server).query(BARE_QUERY).await.unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        assert!(err.message().contains(&MAX_RESULT_ROWS.to_string()));
    }

    // --- query_read_only --------------------------------------------------

    /// The trait's default `query_read_only` runs the SQL classifier over the
    /// query text. A `StructuredQuery` is JSON, not SQL, so the default would
    /// fail closed and refuse *every* Firestore query — silently, and only on
    /// the MCP surface. Read-only here is structural (ADR-0091 §3): the crate
    /// builds no write URL at all, so there is nothing left to classify.
    #[tokio::test]
    async fn a_structured_query_is_not_put_through_the_sql_classifier() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "document": document("a", &json!({})) },
            ])))
            .mount(&server)
            .await;

        let result = connect_to(&server)
            .query_read_only(BARE_QUERY, 10)
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 1);
    }

    #[tokio::test]
    async fn the_read_only_path_truncates_instead_of_erroring() {
        let server = MockServer::start().await;
        let entries: Vec<serde_json::Value> = (0..5)
            .map(|i| json!({ "document": document(&i.to_string(), &json!({})) }))
            .collect();
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(entries))
            .mount(&server)
            .await;

        let result = connect_to(&server)
            .query_read_only(BARE_QUERY, 2)
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 2);
    }

    // --- describe_table ---------------------------------------------------

    #[tokio::test]
    async fn describe_table_samples_the_collection() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/projects/demo/databases/(default)/documents/users"))
            .and(query_param("pageSize", SCHEMA_SAMPLE_SIZE.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "documents": [
                    document("a", &json!({ "email": { "stringValue": "a@dbboard.example.com" } })),
                    document("b", &json!({ "email": { "stringValue": "b@dbboard.example.com" } })),
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let schema = connect_to(&server)
            .describe_table(&TableInfo::unqualified("users"))
            .await
            .unwrap();
        assert_eq!(schema.columns[1].name, "email");
        assert_eq!(
            schema.columns[1].declared_type.as_deref(),
            Some("string (2/2 sampled)"),
            "the description has to show it is an inference"
        );
    }

    #[tokio::test]
    async fn describing_an_empty_collection_is_not_a_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;

        let schema = connect_to(&server)
            .describe_table(&TableInfo::unqualified("users"))
            .await
            .unwrap();
        assert_eq!(schema.columns.len(), 1);
    }

    // --- capabilities -----------------------------------------------------

    /// The workspace invariant: a flag is set exactly when the accessor
    /// behind it is present.
    #[test]
    fn capabilities_agree_with_the_implemented_surface() {
        let adapter = FirestoreAdapter::connect(emulator_config("http://x")).unwrap();
        let caps = adapter.capabilities();
        assert_eq!(caps.has_views, adapter.views().is_some());
        assert_eq!(caps.has_functions, adapter.functions().is_some());
        assert_eq!(caps.has_auth, adapter.auth().is_some());
        assert_eq!(caps.has_storage, adapter.storage().is_some());
        assert_eq!(caps.has_realtime, adapter.realtime().is_some());
    }

    /// Firestore is read-only here by construction, so the write-side flags
    /// must stay off — an adapter that advertises `execute` and then refuses
    /// it is worse than one that never offered.
    #[test]
    fn the_write_capabilities_stay_off() {
        let adapter = FirestoreAdapter::connect(emulator_config("http://x")).unwrap();
        let caps = adapter.capabilities();
        assert!(caps.has_describe_table);
        assert!(!caps.has_execute);
        assert!(!caps.has_atomic_restore);
        assert!(!caps.has_table_ddl);
        assert!(!caps.has_foreign_keys);
    }

    #[tokio::test]
    async fn writing_is_not_merely_refused_at_runtime_it_is_unavailable() {
        let server = MockServer::start().await;
        let adapter = connect_to(&server);
        assert!(matches!(
            adapter.execute("anything").await.unwrap_err(),
            DbError::Capability(_)
        ));
        assert!(matches!(
            adapter
                .execute_in_transaction(&["anything".to_string()])
                .await
                .unwrap_err(),
            DbError::Capability(_)
        ));
    }
}
