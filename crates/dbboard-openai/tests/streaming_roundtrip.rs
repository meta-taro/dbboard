//! Wiremock-driven SSE round-trip coverage for the `OpenAI`
//! `stream_explain` / `stream_suggest_sql` paths (ADR-0052).
//!
//! These exercise the parser end-to-end against a canned Chat
//! Completions SSE body (data-only frames + a `[DONE]` sentinel). No
//! live `OpenAI` endpoint is touched and no API key is required — the
//! construction-time `https_only` guard has an `is_localhost` exception
//! that allows plaintext loopback for exactly this purpose.

use dbboard_ai::{AiError, AiProvider, AiStream, ExplainRequest, StopReason, StreamEvent};
use dbboard_openai::{OpenAiConfig, OpenAiProvider};
use futures_util::StreamExt;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SSE_CONTENT_TYPE: &str = "text/event-stream";

fn provider_for(server: &MockServer, model: &str) -> OpenAiProvider {
    OpenAiProvider::with_config(OpenAiConfig {
        api_key: "test-key".into(),
        model: model.into(),
        base_url: Some(server.uri()),
    })
    .expect("construct provider for wiremock target")
}

fn explain_req() -> ExplainRequest {
    ExplainRequest {
        sql: "SELECT 1".into(),
        dialect: None,
    }
}

async fn collect_events(stream: AiStream) -> Vec<Result<StreamEvent, AiError>> {
    stream.collect::<Vec<_>>().await
}

/// Canonical Chat Completions SSE happy path: a role-only opener → 3
/// content deltas → a finish-reason frame → the `include_usage` frame →
/// the `[DONE]` sentinel.
const HAPPY_PATH_SSE: &str = concat!(
    "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n",
    "\n",
    "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n",
    "\n",
    "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\", \"},\"finish_reason\":null}]}\n",
    "\n",
    "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"world.\"},\"finish_reason\":null}]}\n",
    "\n",
    "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
    "\n",
    "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":11,\"total_tokens\":18}}\n",
    "\n",
    "data: [DONE]\n",
    "\n",
);

#[tokio::test]
async fn happy_path_yields_deltas_usage_and_stop_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(HAPPY_PATH_SSE, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let stream = provider
        .stream_explain(&explain_req())
        .await
        .expect("open SSE stream");
    let events = collect_events(stream).await;

    // 3 TextDelta + 1 Usage + 1 MessageStop = 5. Unlike Anthropic there
    // is no leading MessageStart — OpenAI does not report input tokens
    // up front (ADR-0052).
    let oks: Vec<StreamEvent> = events
        .iter()
        .map(|e| e.as_ref().expect("no transport errors").clone())
        .collect();
    assert_eq!(oks.len(), 5, "unexpected event count: {oks:?}");

    assert_eq!(oks[0], StreamEvent::TextDelta("Hello".into()));
    assert_eq!(oks[1], StreamEvent::TextDelta(", ".into()));
    assert_eq!(oks[2], StreamEvent::TextDelta("world.".into()));
    assert_eq!(
        oks[3],
        StreamEvent::Usage {
            tokens_in: 7,
            tokens_out: 11,
        }
    );
    assert_eq!(
        oks[4],
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn role_only_and_empty_content_frames_are_filtered() {
    // The leading role-only frame and any empty-string content carry
    // nothing to append — the parser must drop them and surface only
    // the real TextDelta plus the terminal MessageStop.
    let sse = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n",
        "\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"},\"finish_reason\":null}]}\n",
        "\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n",
        "\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        "\n",
        "data: [DONE]\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    let events = collect_events(stream).await;

    let oks: Vec<StreamEvent> = events
        .iter()
        .map(|e| e.as_ref().expect("no transport errors").clone())
        .collect();
    // TextDelta("hi") + MessageStop(EndTurn). No usage frame here.
    assert_eq!(oks.len(), 2, "filtered sequence was {oks:?}");
    assert_eq!(oks[0], StreamEvent::TextDelta("hi".into()));
    assert_eq!(
        oks[1],
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn length_finish_reason_maps_to_max_tokens() {
    let sse = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"trunc\"},\"finish_reason\":null}]}\n",
        "\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n",
        "\n",
        "data: [DONE]\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let events = collect_events(provider.stream_explain(&explain_req()).await.unwrap()).await;
    let last = events.last().expect("at least one event").as_ref().unwrap();
    assert_eq!(
        last,
        &StreamEvent::MessageStop {
            stop_reason: StopReason::MaxTokens,
        }
    );
}

#[tokio::test]
async fn unknown_finish_reason_surfaces_as_other() {
    let sse = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n",
        "\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"future_reason_v9\"}]}\n",
        "\n",
        "data: [DONE]\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let events = collect_events(provider.stream_explain(&explain_req()).await.unwrap()).await;
    let last = events.last().expect("at least one event").as_ref().unwrap();
    assert_eq!(
        last,
        &StreamEvent::MessageStop {
            stop_reason: StopReason::Other("future_reason_v9".into()),
        }
    );
}

#[tokio::test]
async fn stream_ending_without_done_sentinel_still_emits_defensive_stop() {
    // Server closes the connection after a delta without sending the
    // `[DONE]` sentinel (a hung-up connection or a misbehaving proxy).
    // The parser must still terminate cleanly with a MessageStop.
    let sse = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"early\"},\"finish_reason\":null}]}\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    let events = collect_events(stream).await;

    let oks: Vec<StreamEvent> = events
        .iter()
        .map(|e| e.as_ref().expect("no transport errors").clone())
        .collect();
    assert_eq!(oks.len(), 2, "events: {oks:?}");
    assert_eq!(oks[0], StreamEvent::TextDelta("early".into()));
    assert_eq!(
        oks[1],
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn non_2xx_response_surfaces_on_the_first_stream_item_as_provider_error() {
    // `reqwest-eventsource` opens the connection lazily on the first
    // `next()`; a 401 / 5xx therefore surfaces as the FIRST stream item,
    // not as the outer `Err` from `stream_explain(...)`.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let mut stream = provider
        .stream_explain(&explain_req())
        .await
        .expect("open returns Ok even though the upstream will reject");
    let first = stream.next().await.expect("at least one item");
    match first {
        Err(AiError::Provider(msg)) => {
            assert!(msg.contains("503"), "msg = {msg}");
        }
        other => panic!("expected Err(Provider(503…)), got {other:?}"),
    }
}

#[tokio::test]
async fn non_2xx_response_surfaces_the_api_error_body_not_just_the_status() {
    // A 400 from the API carries the reason in its JSON body (e.g. an
    // unknown model). The streaming error path must surface that detail
    // the same way the non-streaming path does.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "type": "invalid_request_error",
                "message": "The model `gpt-nope` does not exist",
                "code": "model_not_found"
            }
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-nope");
    let mut stream = provider
        .stream_explain(&explain_req())
        .await
        .expect("open returns Ok even though the upstream will reject");
    let first = stream.next().await.expect("at least one item");
    match first {
        Err(AiError::Provider(msg)) => {
            assert!(msg.contains("400"), "status code must survive: {msg}");
            assert!(
                msg.contains("does not exist"),
                "the API error message must reach the user: {msg}"
            );
            assert!(
                msg.contains("invalid_request_error"),
                "the API error kind must reach the user: {msg}"
            );
        }
        other => panic!("expected Err(Provider(400…)), got {other:?}"),
    }
}

#[tokio::test]
async fn streaming_request_body_carries_stream_true_and_include_usage() {
    // Smoke test the body shape: `stream_explain` must send
    // `"stream": true` AND `stream_options.include_usage = true` so the
    // final usage frame is emitted, plus the same system+user message
    // shape used by the non-streaming path.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n",
                "\n",
                "data: [DONE]\n",
                "\n",
            ),
            SSE_CONTENT_TYPE,
        ))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "gpt-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    // Drain so the request actually goes out.
    let _ = collect_events(stream).await;

    let requests = server.received_requests().await.expect("wiremock log");
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("json body");
    assert_eq!(body["stream"], serde_json::Value::Bool(true));
    assert_eq!(
        body["stream_options"]["include_usage"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(body["model"], "gpt-x");
    assert_eq!(body["messages"][0]["role"], "system");
    let content = body["messages"][1]["content"].as_str().unwrap();
    assert!(content.contains("SELECT 1"));
}
