//! HTTP-level tests for the local backend.
//!
//! Most cases drive the router in-process with `tower`'s `oneshot`, so
//! they need no socket. One case exercises the real [`serve`] path:
//! bind a loopback port, round-trip over `reqwest`, then shut down.
//!
//! Every test connects Turso `:memory:` exactly once and reuses the
//! resulting state, because a fresh libSQL connection is its own empty
//! database — the `create_table_then_lists_it` case would silently
//! regress if the server ever reconnected per request.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dbboard_server::{
    build_adapter, build_router, connect, serve, swap_backend, AppState, BackendConfig,
};
use serde_json::{json, Value};
use tower::ServiceExt as _;

async fn memory_state() -> AppState {
    connect(BackendConfig::turso(":memory:"))
        .await
        .expect("connect in-memory Turso")
}

/// Drive one request through the router built from `state` (a clone, so
/// the shared backend connection is reused). Returns the status and the
/// parsed JSON body (`Value::Null` when the body is empty).
async fn request(state: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let response = build_router(state.clone())
        .oneshot(req)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    // axum's own extractor rejections (e.g. a malformed body) reply
    // with plain text, so fall back to Null rather than insisting on
    // JSON; the JSON assertions live in the individual tests.
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("build GET")
}

fn post_query(sql: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sql": sql }).to_string()))
        .expect("build POST /query")
}

#[tokio::test]
async fn health_returns_ok() {
    let state = memory_state().await;
    let (status, body) = request(&state, get("/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test]
async fn capabilities_reports_adapter_id_and_flags() {
    let state = memory_state().await;
    let (status, body) = request(&state, get("/capabilities")).await;
    assert_eq!(status, StatusCode::OK);
    // Turso ships no Supabase-style optional surfaces (ADR-0012), so
    // those flags are `false`; `has_describe_table` turned `true` with
    // ADR-0028 slice (b). `has_table_ddl` (ADR-0049) is still `false` here.
    // The restore flags `has_execute`/`has_atomic_restore` (ADR-0051) turned
    // `true` once libSQL grew per-statement + atomic execution — it has real
    // multi-statement transactions. The id is what `TursoAdapter::id` returns.
    assert_eq!(
        body,
        json!({
            "id": "turso",
            "capabilities": {
                "has_views": false,
                "has_functions": false,
                "has_auth": false,
                "has_storage": false,
                "has_realtime": false,
                "has_describe_table": true,
                "has_table_ddl": false,
                "has_execute": true,
                "has_atomic_restore": true,
                "has_foreign_keys": true,
            }
        })
    );
}

#[tokio::test]
async fn create_table_then_lists_it() {
    let state = memory_state().await;

    let (status, _) = request(&state, post_query("CREATE TABLE users (id INTEGER)")).await;
    assert_eq!(status, StatusCode::OK);

    // Same connection, so the table created above must be visible.
    let (status, body) = request(&state, get("/tables")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "tables": [{ "schema": null, "name": "users" }] })
    );
}

#[tokio::test]
async fn insert_reports_rows_affected() {
    let state = memory_state().await;
    request(&state, post_query("CREATE TABLE t (n INTEGER)")).await;

    let (status, body) = request(&state, post_query("INSERT INTO t (n) VALUES (1), (2)")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows_affected"], json!(2));
    assert_eq!(body["rows"], json!([]));
}

#[tokio::test]
async fn select_returns_typed_values() {
    let state = memory_state().await;
    let (status, body) = request(
        &state,
        post_query("SELECT 42 AS i, 'hi' AS t, NULL AS n, x'00ff' AS b"),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let columns: Vec<&str> = body["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(columns, ["i", "t", "n", "b"]);
    // Integer/text/null are native JSON; the blob rides in a tagged
    // base64 object. x'00ff' is bytes [0, 255] -> "AP8=".
    assert_eq!(body["rows"], json!([[42, "hi", null, { "$blob": "AP8=" }]]));
    assert_eq!(body["rows_affected"], json!(0));
}

#[tokio::test]
async fn empty_select_returns_ok_with_no_rows() {
    let state = memory_state().await;
    request(&state, post_query("CREATE TABLE empty (id INTEGER)")).await;

    let (status, body) = request(&state, post_query("SELECT * FROM empty")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"], json!([]));
}

#[tokio::test]
async fn invalid_sql_is_a_400_query_error() {
    let state = memory_state().await;
    let (status, body) = request(&state, post_query("SELECT FROM nope")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["category"], json!("query"));
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn missing_sql_field_is_unprocessable() {
    let state = memory_state().await;
    let req = Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"not_sql": 1}"#))
        .expect("build request");
    // Structurally valid JSON that doesn't fit QueryRequest: axum's Json
    // extractor reports 422 Unprocessable Entity.
    let (status, _) = request(&state, req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn non_json_content_type_is_unsupported_media_type() {
    let state = memory_state().await;
    let req = Request::builder()
        .method("POST")
        .uri("/query")
        .header("content-type", "text/plain")
        .body(Body::from("SELECT 1"))
        .expect("build request");
    let (status, _) = request(&state, req).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

/// ADR-0020 swap point: after `swap_backend` runs, the *next* request
/// must hit the new adapter. Two distinct in-memory libSQL databases —
/// each its own empty schema — make the swap observable: a table
/// created against adapter A must not be visible through adapter B.
#[tokio::test]
async fn swap_backend_routes_next_request_to_new_adapter() {
    let state = memory_state().await;

    // Create a table against the original adapter.
    let (status, _) = request(&state, post_query("CREATE TABLE in_original (n INTEGER)")).await;
    assert_eq!(status, StatusCode::OK);

    // Build a *fresh* in-memory Turso adapter — independent connection,
    // therefore independent (empty) schema.
    let fresh_adapter = build_adapter(BackendConfig::turso(":memory:"))
        .await
        .expect("build fresh adapter");

    // Swap the running state to point at the fresh adapter.
    swap_backend(&state, fresh_adapter);

    // The next /tables request must see the fresh adapter's empty
    // schema, not the original adapter's `in_original` table.
    let (status, body) = request(&state, get("/tables")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "tables": [] }));
}

/// ADR-0020: the desktop binary owns a `RunningServer` and reaches the
/// live `AppState` through it. After a swap issued via that state, the
/// loopback server's next response must reflect the new adapter.
#[tokio::test]
async fn running_server_state_lets_swap_take_effect_over_loopback() {
    let server = serve(BackendConfig::turso(":memory:"))
        .await
        .expect("server starts");
    let base = format!("http://127.0.0.1:{}", server.port);
    let client = reqwest::Client::new();

    // Plant a table through the original adapter.
    let create: Value = client
        .post(format!("{base}/query"))
        .json(&json!({ "sql": "CREATE TABLE in_original (n INTEGER)" }))
        .send()
        .await
        .expect("create request")
        .json()
        .await
        .expect("create body");
    assert_eq!(create["rows_affected"], json!(0));

    // Build a fresh adapter and swap it in via the exposed AppState.
    let fresh = build_adapter(BackendConfig::turso(":memory:"))
        .await
        .expect("build fresh adapter");
    swap_backend(&server.state(), fresh);

    // The next /tables call over the same loopback socket must see the
    // fresh adapter's empty schema, not the original `in_original` row.
    let tables: Value = client
        .get(format!("{base}/tables"))
        .send()
        .await
        .expect("tables request")
        .json()
        .await
        .expect("tables body");
    assert_eq!(tables, json!({ "tables": [] }));

    server.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn real_serve_round_trips_over_loopback() {
    let server = serve(BackendConfig::turso(":memory:"))
        .await
        .expect("server starts");
    let base = format!("http://127.0.0.1:{}", server.port);
    let client = reqwest::Client::new();

    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("health request")
        .json()
        .await
        .expect("health body");
    assert_eq!(health, json!({ "status": "ok" }));

    let result: Value = client
        .post(format!("{base}/query"))
        .json(&json!({ "sql": "SELECT 1 AS one" }))
        .send()
        .await
        .expect("query request")
        .json()
        .await
        .expect("query body");
    assert_eq!(result["rows"], json!([[1]]));

    server.shutdown().await.expect("clean shutdown");
}
