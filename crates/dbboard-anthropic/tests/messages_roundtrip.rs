//! Round-trip tests against a wiremock-backed mock Anthropic Messages
//! endpoint.
//!
//! These exercise the full `reqwest` send / receive path without
//! reaching the live Anthropic API, so they are part of the standard
//! `cargo test` run (no env-var gating). A live test that hits the
//! real endpoint behind `DBBOARD_ANTHROPIC_API_KEY` is deferred to a
//! follow-up issue.

use dbboard_ai::{AiError, AiProvider, ExplainRequest, SuggestRequest, TableInfo};
use dbboard_anthropic::{AnthropicConfig, AnthropicProvider};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider_for(server: &MockServer, model: &str) -> AnthropicProvider {
    AnthropicProvider::with_config(AnthropicConfig {
        api_key: "test-key".into(),
        model: model.into(),
        base_url: Some(server.uri()),
    })
    .expect("build provider against mock server")
}

fn ok_response(text: &str, input_tokens: u32, output_tokens: u32) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "content": [{"type": "text", "text": text}],
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens}
    }))
}

#[tokio::test]
async fn explain_round_trips_through_the_mock_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ok_response("selects one row", 12, 5))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-test");
    let resp = provider
        .explain(&ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: Some("postgres".into()),
        })
        .await
        .expect("explain ok");
    assert_eq!(resp.text, "selects one row");
    assert_eq!(resp.tokens_in, 12);
    assert_eq!(resp.tokens_out, 5);
}

#[tokio::test]
async fn suggest_sql_round_trips_through_the_mock_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ok_response("SELECT * FROM users", 9, 6))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-test");
    let resp = provider
        .suggest_sql(&SuggestRequest {
            prompt: "all users".into(),
            dialect: Some("postgres".into()),
            schema: vec![TableInfo::qualified("public", "users")],
            full_schema: None,
        })
        .await
        .expect("suggest ok");
    assert_eq!(resp.text, "SELECT * FROM users");
}

#[tokio::test]
async fn request_body_carries_model_system_and_user_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ok_response("ok", 1, 1))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-test");
    provider
        .explain(&ExplainRequest {
            sql: "SELECT pi()".into(),
            dialect: Some("postgres".into()),
        })
        .await
        .expect("explain ok");

    // wiremock records every received request; assert that the
    // serialised payload looks right rather than only that the mock
    // matched.
    let received = server
        .received_requests()
        .await
        .expect("wiremock records requests");
    assert_eq!(received.len(), 1);
    let body: Value = serde_json::from_slice(&received[0].body).expect("json body");
    assert_eq!(body["model"], "claude-test");
    let content = body["messages"][0]["content"]
        .as_str()
        .expect("string user content");
    assert!(content.contains("SELECT pi()"));
    assert!(content.contains("postgres"));
    assert!(body["system"]
        .as_str()
        .expect("string system prompt")
        .contains("SQL expert"));
}

#[tokio::test]
async fn rate_limit_response_becomes_a_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "type": "error",
            "error": {"type": "rate_limit_error", "message": "too many requests"}
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-test");
    let err = provider
        .explain(&ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: None,
        })
        .await
        .unwrap_err();
    let AiError::Provider(msg) = err else {
        panic!("expected Provider, got {err:?}");
    };
    assert!(msg.contains("429"));
    assert!(msg.contains("rate_limit_error"));
}

#[tokio::test]
async fn server_5xx_becomes_a_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream timeout"))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-test");
    let err = provider
        .explain(&ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: None,
        })
        .await
        .unwrap_err();
    let AiError::Provider(msg) = err else {
        panic!("expected Provider, got {err:?}");
    };
    assert!(msg.contains("503"));
}

#[tokio::test]
async fn authentication_failure_becomes_a_provider_error_not_configuration() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "invalid api key"}
        })))
        .mount(&server)
        .await;

    // Construction-time validation already passed (key is non-empty),
    // so per ADR-0023 §8 / issue 0005, the 401 surfaces as Provider
    // rather than re-raising as Configuration.
    let provider = provider_for(&server, "claude-test");
    let err = provider
        .explain(&ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: None,
        })
        .await
        .unwrap_err();
    let AiError::Provider(msg) = err else {
        panic!("expected Provider, got {err:?}");
    };
    assert!(msg.contains("authentication_error"));
}

#[tokio::test]
async fn malformed_success_body_becomes_a_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("not json at all")
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-test");
    let err = provider
        .explain(&ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: None,
        })
        .await
        .unwrap_err();
    let AiError::Provider(msg) = err else {
        panic!("expected Provider, got {err:?}");
    };
    assert!(msg.contains("malformed"));
}
