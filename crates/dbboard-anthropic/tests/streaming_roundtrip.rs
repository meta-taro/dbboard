//! Wiremock-driven SSE round-trip coverage for the Anthropic
//! `stream_explain` / `stream_suggest_sql` paths (ADR-0026 Slice b).
//!
//! These tests exercise the parser end-to-end:
//!
//! 1. Build an [`AnthropicProvider`] pointed at a local wiremock
//!    server (the construction-time `https_only` guard has a
//!    `is_localhost` exception that allows plaintext loopback for
//!    exactly this purpose).
//! 2. Mount a `POST /v1/messages` mock that returns an
//!    `text/event-stream` body containing a canned Anthropic SSE
//!    sequence.
//! 3. Open the stream via `stream_explain(...)`, collect every
//!    event, and assert on the normalized chunk sequence emitted
//!    through the [`AiStream`] surface.
//!
//! No live Anthropic endpoint is touched. No `DBBOARD_ANTHROPIC_API_KEY`
//! is required.

use dbboard_ai::{AiError, AiProvider, AiStream, ExplainRequest, StopReason, StreamEvent};
use dbboard_anthropic::{AnthropicConfig, AnthropicProvider};
use futures_util::StreamExt;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SSE_CONTENT_TYPE: &str = "text/event-stream";

fn provider_for(server: &MockServer, model: &str) -> AnthropicProvider {
    AnthropicProvider::with_config(AnthropicConfig {
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

/// Canonical Anthropic SSE happy path: `message_start` → 3 text
/// deltas inside a single content block → `message_delta` with
/// `stop_reason` + cumulative `output_tokens` → `message_stop`.
const HAPPY_PATH_SSE: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"claude-x\",\"stop_reason\":null,\"usage\":{\"input_tokens\":7,\"output_tokens\":0}}}\n",
    "\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
    "\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n",
    "\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\", \"}}\n",
    "\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"world.\"}}\n",
    "\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
    "\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":11}}\n",
    "\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n",
    "\n",
);

#[tokio::test]
async fn happy_path_yields_start_deltas_usage_and_stop_in_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(HAPPY_PATH_SSE, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
    let stream = provider
        .stream_explain(&explain_req())
        .await
        .expect("open SSE stream");
    let events = collect_events(stream).await;

    // 1 MessageStart + 3 TextDelta + 1 Usage + 1 MessageStop = 6.
    let oks: Vec<StreamEvent> = events
        .iter()
        .map(|e| e.as_ref().expect("no transport errors").clone())
        .collect();
    assert_eq!(oks.len(), 6, "unexpected event count: {oks:?}");

    assert_eq!(oks[0], StreamEvent::MessageStart { tokens_in: 7 });
    assert_eq!(oks[1], StreamEvent::TextDelta("Hello".into()));
    assert_eq!(oks[2], StreamEvent::TextDelta(", ".into()));
    assert_eq!(oks[3], StreamEvent::TextDelta("world.".into()));
    assert_eq!(
        oks[4],
        StreamEvent::Usage {
            tokens_in: 7,
            tokens_out: 11,
        }
    );
    assert_eq!(
        oks[5],
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn ping_and_content_block_lifecycle_events_are_filtered() {
    // `ping`, `content_block_start`, `content_block_stop`, and a
    // non-text content delta (`input_json_delta`) are all noise per
    // ADR-0026 Decision 3 — the parser must drop them and surface
    // only the canonical MessageStart/TextDelta/Usage/MessageStop
    // sequence.
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":2}}}\n",
        "\n",
        "event: ping\n",
        "data: {\"type\":\"ping\"}\n",
        "\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"k\\\":\"}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n",
        "\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
        "\n",
        "event: ping\n",
        "data: {\"type\":\"ping\"}\n",
        "\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    let events = collect_events(stream).await;

    let oks: Vec<StreamEvent> = events
        .iter()
        .map(|e| e.as_ref().expect("no transport errors").clone())
        .collect();
    // MessageStart + TextDelta("hi") + MessageStop(EndTurn default)
    assert_eq!(oks.len(), 3, "filtered sequence was {oks:?}");
    assert_eq!(oks[0], StreamEvent::MessageStart { tokens_in: 2 });
    assert_eq!(oks[1], StreamEvent::TextDelta("hi".into()));
    assert_eq!(
        oks[2],
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn unknown_stop_reason_surfaces_as_other() {
    // ADR-0026 Decision 3: `StopReason::Other(String)` is the future-
    // proofing escape hatch so a new Anthropic stop reason (or one
    // from a future provider) does not crash the stream.
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n",
        "\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"future_reason_v9\"},\"usage\":{\"output_tokens\":1}}\n",
        "\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
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
async fn mid_stream_error_event_surfaces_as_stream_event_error_not_outer_err() {
    // Anthropic sends a fully-formed SSE `error` event mid-stream
    // (e.g. `overloaded_error`). The connection itself is still 200
    // and `text/event-stream`, so the **outer** Result must stay
    // `Ok(stream)` and the failure surfaces as
    // `Ok(StreamEvent::Error(_))` — ADR-0026 Decision 6.
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n",
        "\n",
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"please retry\"}}\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    let events = collect_events(stream).await;

    // 1 MessageStart + 1 Error + 1 defensive MessageStop = 3.
    assert_eq!(events.len(), 3, "events: {events:?}");
    matches::assert(&events[0], |e| {
        matches!(e, Ok(StreamEvent::MessageStart { .. }))
    });
    let error_event = events[1].as_ref().expect("error event is Ok-wrapped");
    match error_event {
        StreamEvent::Error(AiError::Provider(msg)) => {
            assert!(msg.contains("overloaded_error"));
            assert!(msg.contains("please retry"));
        }
        other => panic!("expected StreamEvent::Error(Provider), got {other:?}"),
    }
}

#[tokio::test]
async fn stream_ending_without_message_stop_still_emits_defensive_stop() {
    // Server closes the connection after the deltas without sending
    // a `message_stop` (Anthropic does not promise this, but a
    // misbehaving proxy or a hung-up connection can produce it). The
    // parser must still terminate cleanly with a `MessageStop` so the
    // worker sees a final chunk.
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3}}}\n",
        "\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"early\"}}\n",
        "\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, SSE_CONTENT_TYPE))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    let events = collect_events(stream).await;

    let oks: Vec<StreamEvent> = events
        .iter()
        .map(|e| e.as_ref().expect("no transport errors").clone())
        .collect();
    assert_eq!(oks.len(), 3, "events: {oks:?}");
    assert_eq!(oks[0], StreamEvent::MessageStart { tokens_in: 3 });
    assert_eq!(oks[1], StreamEvent::TextDelta("early".into()));
    assert_eq!(
        oks[2],
        StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }
    );
}

#[tokio::test]
async fn non_2xx_response_surfaces_on_the_first_stream_item_as_provider_error() {
    // `reqwest-eventsource` opens the connection lazily on the first
    // `next()`; a 401 / 5xx therefore surfaces as the FIRST stream
    // item, **not** as the outer `Err` from `stream_explain(...)`.
    // The worker layer (Slice c) must handle this asymmetry.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
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
    // Regression: a 400 from the Messages API carries the *reason* in
    // its JSON body (e.g. "credit balance too low", an invalid model).
    // The streaming error path used to drop that body and surface only
    // `status 400`, forcing users to guess. The detail must reach the
    // error message the same way the non-streaming path already does.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "Your credit balance is too low to access the Claude API."
            }
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
    let mut stream = provider
        .stream_explain(&explain_req())
        .await
        .expect("open returns Ok even though the upstream will reject");
    let first = stream.next().await.expect("at least one item");
    match first {
        Err(AiError::Provider(msg)) => {
            assert!(msg.contains("400"), "status code must survive: {msg}");
            assert!(
                msg.contains("credit balance is too low"),
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
async fn streaming_request_body_carries_stream_true_and_keeps_user_content() {
    // Smoke test the body shape: when `stream_explain` runs the
    // outgoing JSON must carry `"stream": true` AND the same
    // user-content / system shape used by the non-streaming path.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        // The body matcher we have available is the generic
        // `body_json_string_contains` via wiremock-rs's
        // `BodyContainsMatcher`. We assert post-hoc via the request
        // log instead, which is what wiremock 0.6 exposes cleanly.
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                "event: message_start\n",
                "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n",
                "\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n",
                "\n",
            ),
            SSE_CONTENT_TYPE,
        ))
        .mount(&server)
        .await;

    let provider = provider_for(&server, "claude-x");
    let stream = provider.stream_explain(&explain_req()).await.unwrap();
    // Drain so the request actually goes out.
    let _ = collect_events(stream).await;

    let requests = server.received_requests().await.expect("wiremock log");
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("json body");
    assert_eq!(body["stream"], serde_json::Value::Bool(true));
    assert_eq!(body["model"], "claude-x");
    let content = body["messages"][0]["content"].as_str().unwrap();
    assert!(content.contains("SELECT 1"));
}

mod matches {
    use std::fmt::Debug;

    /// Helper that gives a clean failure message when an
    /// `assert!(matches!(x, P))` would normally print only "false".
    pub fn assert<T: Debug>(value: &T, predicate: impl Fn(&T) -> bool) {
        assert!(predicate(value), "predicate failed on {value:?}");
    }
}
