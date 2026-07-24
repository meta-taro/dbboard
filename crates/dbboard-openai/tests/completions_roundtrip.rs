//! Round-trip tests against a wiremock-backed mock `OpenAI` Chat
//! Completions endpoint.
//!
//! These exercise the full `reqwest` send / receive path without
//! reaching the live `OpenAI` API, so they are part of the standard
//! `cargo test` run (no env-var gating). A live test that hits the real
//! endpoint behind an env-provided key is deferred to a follow-up.

use dbboard_ai::{AiError, AiProvider, ExplainRequest, SuggestRequest, TableInfo};
use dbboard_openai::{OpenAiConfig, OpenAiProvider};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider_for(server: &MockServer, model: &str) -> OpenAiProvider {
    OpenAiProvider::with_config(OpenAiConfig {
        api_key: "test-key".into(),
        model: model.into(),
        base_url: Some(server.uri()),
    })
    .expect("build provider against mock server")
}

fn ok_response(text: &str, prompt_tokens: u32, completion_tokens: u32) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    }))
}

#[tokio::test]
async fn explain_round_trips_through_the_mock_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        // OpenAI authenticates with a bearer token, not x-api-key.
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ok_response("selects one row", 12, 5))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-test");
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
    assert_eq!(resp.provider, "openai");
    assert_eq!(resp.model, "gpt-test");
}

#[tokio::test]
async fn suggest_sql_round_trips_through_the_mock_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ok_response("SELECT * FROM users", 9, 6))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-test");
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
async fn request_body_carries_model_and_system_plus_user_messages() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ok_response("ok", 1, 1))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-test");
    provider
        .explain(&ExplainRequest {
            sql: "SELECT pi()".into(),
            dialect: Some("postgres".into()),
        })
        .await
        .expect("explain ok");

    let received = server
        .received_requests()
        .await
        .expect("wiremock records requests");
    assert_eq!(received.len(), 1);
    let body: Value = serde_json::from_slice(&received[0].body).expect("json body");
    assert_eq!(body["model"], "gpt-test");
    // System prompt is the first message; there is no top-level `system`
    // field on the Chat Completions surface.
    assert!(body.get("system").is_none());
    assert_eq!(body["messages"][0]["role"], "system");
    assert!(body["messages"][0]["content"]
        .as_str()
        .expect("string system prompt")
        .contains("SQL expert"));
    assert_eq!(body["messages"][1]["role"], "user");
    let content = body["messages"][1]["content"]
        .as_str()
        .expect("string user content");
    assert!(content.contains("SELECT pi()"));
    assert!(content.contains("postgres"));
    // No max_tokens cap is sent (ADR-0052).
    assert!(body.get("max_tokens").is_none());
}

#[tokio::test]
async fn quota_response_becomes_a_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": {
                "type": "insufficient_quota",
                "message": "You exceeded your current quota",
                "code": "insufficient_quota"
            }
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-test");
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
    assert!(msg.contains("insufficient_quota"));
}

#[tokio::test]
async fn server_5xx_becomes_a_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream timeout"))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-test");
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
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": {
                "type": "invalid_request_error",
                "message": "Incorrect API key provided",
                "code": "invalid_api_key"
            }
        })))
        .mount(&server)
        .await;

    // Construction-time validation already passed (key is non-empty),
    // so per ADR-0023 §8 / issue 0005, the 401 surfaces as Provider
    // rather than re-raising as Configuration.
    let provider = provider_for(&server, "gpt-test");
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
    assert!(msg.contains("invalid_request_error"));
}

#[tokio::test]
async fn malformed_success_body_becomes_a_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("not json at all")
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-test");
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
