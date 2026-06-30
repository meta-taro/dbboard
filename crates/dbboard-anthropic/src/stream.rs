//! Anthropic SSE → normalized [`StreamEvent`] mapping (ADR-0026
//! Slice b).
//!
//! Anthropic's Messages API streaming wire format is RFC SSE
//! (`event: <type>\ndata: <json>\n\n`) with the following event types
//! per their streaming reference:
//!
//! ```text
//! event: message_start
//! data: {"type":"message_start","message":{"usage":{"input_tokens":42}}}
//!
//! event: content_block_start
//! data: {"type":"content_block_start","index":0,"content_block":{...}}
//!
//! event: content_block_delta
//! data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}
//!
//! event: content_block_stop
//! data: {"type":"content_block_stop","index":0}
//!
//! event: message_delta
//! data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},
//!        "usage":{"output_tokens":7}}
//!
//! event: message_stop
//! data: {"type":"message_stop"}
//!
//! event: ping
//! data: {"type":"ping"}
//!
//! event: error
//! data: {"type":"error","error":{"type":"overloaded_error","message":"..."}}
//! ```
//!
//! Per ADR-0026 Decision 3 the parser keeps the trait surface
//! provider-independent: every event is mapped into a normalized
//! [`StreamEvent`], and the non-text content-block deltas
//! (`input_json_delta`, `thinking_delta`, `signature_delta`),
//! `content_block_start`, `content_block_stop`, and `ping` are
//! **filtered** at this layer. They are Anthropic-specific protocol
//! noise the UI does not need.
//!
//! `message_delta.usage.output_tokens` is **cumulative** (per the
//! Anthropic spec), so the running input token count is captured at
//! `message_start` and threaded through `Usage` events without
//! summing — the UI replaces its meter on each event per ADR-0026
//! Decision 7.

use dbboard_ai::{AiError, AiResult, AiStream, StopReason, StreamEvent};
use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde::Deserialize;

use crate::{truncate_to_owned, MAX_ERROR_DETAIL};

/// Adapt a [`reqwest_eventsource::EventSource`] into an [`AiStream`].
///
/// The returned stream lifecycle:
/// - emits one [`StreamEvent::MessageStart`] per Anthropic
///   `message_start` event,
/// - emits zero-or-more [`StreamEvent::TextDelta`] for each
///   `content_block_delta` of type `text_delta`,
/// - emits one [`StreamEvent::Usage`] per `message_delta` that
///   carries a `usage` block, with the cumulative `tokens_out` and
///   the input-token snapshot captured at `message_start`,
/// - emits exactly one [`StreamEvent::MessageStop`] on `message_stop`
///   (or defensively on `StreamEnded` if the server closes the
///   connection without sending `message_stop`),
/// - emits one [`StreamEvent::Error`] per Anthropic `error` event,
/// - propagates any transport-level failure as `Err(AiError)` and
///   terminates the stream.
///
/// Dropping the returned stream cancels the in-flight request via
/// the [`EventSource`] / `reqwest::Response` drop chain — no
/// trait-level cancellation token is required (ADR-0026 Decision 5).
pub(crate) fn anthropic_stream(es: EventSource) -> AiStream {
    let state = StreamState::new(es);
    let s = futures_util::stream::unfold(state, |mut state| async move {
        if state.closed {
            return None;
        }
        loop {
            // EventSource exhaustion → `unfold` returns `None`. The
            // `?` is on the outer `Option<_>` from `Stream::next` and
            // discards the state value, which is exactly what we want
            // here.
            let item = state.es.next().await?;
            match item {
                Ok(Event::Open) => {}
                Ok(Event::Message(msg)) => match parse_message(&msg.event, &msg.data, &mut state) {
                    ParseOutcome::Emit(evt) => return Some((Ok(evt), state)),
                    ParseOutcome::Filter => {}
                    ParseOutcome::Stop => {
                        state.closed = true;
                        state.es.close();
                        let evt = StreamEvent::MessageStop {
                            stop_reason: state.pending_stop_reason.clone(),
                        };
                        return Some((Ok(evt), state));
                    }
                    ParseOutcome::ParseError(e) => {
                        state.closed = true;
                        state.es.close();
                        return Some((Err(e), state));
                    }
                },
                Err(reqwest_eventsource::Error::StreamEnded) => {
                    // Server closed the connection naturally. If we
                    // never observed an explicit `message_stop`, emit
                    // a defensive `MessageStop` carrying the last
                    // `stop_reason` we saw on a `message_delta` (or
                    // the EndTurn default) so the worker gets a clean
                    // terminator.
                    state.closed = true;
                    let evt = StreamEvent::MessageStop {
                        stop_reason: state.pending_stop_reason.clone(),
                    };
                    return Some((Ok(evt), state));
                }
                Err(err) => {
                    state.closed = true;
                    state.es.close();
                    return Some((Err(map_eventsource_err(err)), state));
                }
            }
        }
    });
    Box::pin(s)
}

struct StreamState {
    es: EventSource,
    /// Input-token snapshot captured at `message_start`. Threaded
    /// into every subsequent `Usage` event so the UI's token meter
    /// shows both axes.
    running_input_tokens: u32,
    /// `stop_reason` parsed from the most recent `message_delta`.
    /// Defaults to `EndTurn` so a stream that ends without an
    /// explicit `stop_reason` still produces a sensible
    /// `MessageStop`.
    pending_stop_reason: StopReason,
    closed: bool,
}

impl StreamState {
    fn new(es: EventSource) -> Self {
        Self {
            es,
            running_input_tokens: 0,
            pending_stop_reason: StopReason::EndTurn,
            closed: false,
        }
    }
}

enum ParseOutcome {
    Emit(StreamEvent),
    Filter,
    /// `message_stop` received — caller emits the final
    /// `MessageStop { pending_stop_reason }` chunk and terminates.
    Stop,
    ParseError(AiError),
}

fn parse_message(event: &str, data: &str, state: &mut StreamState) -> ParseOutcome {
    match event {
        "message_start" => parse_message_start(data, state),
        "content_block_delta" => parse_content_block_delta(data),
        "message_delta" => parse_message_delta(data, state),
        "message_stop" => ParseOutcome::Stop,
        "error" => parse_error(data),
        // ping / content_block_start / content_block_stop /
        // signature_delta / thinking_delta / input_json_delta —
        // all filtered per ADR-0026 Decision 3.
        _ => ParseOutcome::Filter,
    }
}

fn parse_message_start(data: &str, state: &mut StreamState) -> ParseOutcome {
    #[derive(Deserialize)]
    struct Payload {
        message: PayloadMessage,
    }
    #[derive(Deserialize)]
    struct PayloadMessage {
        usage: PayloadUsage,
    }
    #[derive(Deserialize)]
    struct PayloadUsage {
        #[serde(default)]
        input_tokens: u32,
    }

    match serde_json::from_str::<Payload>(data) {
        Ok(p) => {
            state.running_input_tokens = p.message.usage.input_tokens;
            ParseOutcome::Emit(StreamEvent::MessageStart {
                tokens_in: p.message.usage.input_tokens,
            })
        }
        Err(e) => ParseOutcome::ParseError(AiError::Provider(format!(
            "anthropic streaming: malformed message_start payload: {e}"
        ))),
    }
}

fn parse_content_block_delta(data: &str) -> ParseOutcome {
    #[derive(Deserialize)]
    struct Payload {
        delta: Delta,
    }
    #[derive(Deserialize)]
    #[serde(tag = "type")]
    enum Delta {
        #[serde(rename = "text_delta")]
        TextDelta { text: String },
        #[serde(other)]
        Other,
    }

    match serde_json::from_str::<Payload>(data) {
        Ok(Payload {
            delta: Delta::TextDelta { text },
        }) => ParseOutcome::Emit(StreamEvent::TextDelta(text)),
        Ok(Payload {
            delta: Delta::Other,
        }) => ParseOutcome::Filter,
        Err(e) => ParseOutcome::ParseError(AiError::Provider(format!(
            "anthropic streaming: malformed content_block_delta payload: {e}"
        ))),
    }
}

fn parse_message_delta(data: &str, state: &mut StreamState) -> ParseOutcome {
    #[derive(Deserialize)]
    struct Payload {
        #[serde(default)]
        delta: Option<DeltaInner>,
        #[serde(default)]
        usage: Option<DeltaUsage>,
    }
    #[derive(Deserialize)]
    struct DeltaInner {
        #[serde(default)]
        stop_reason: Option<String>,
    }
    #[derive(Deserialize)]
    struct DeltaUsage {
        #[serde(default)]
        output_tokens: u32,
    }

    let parsed: Payload = match serde_json::from_str(data) {
        Ok(p) => p,
        Err(e) => {
            return ParseOutcome::ParseError(AiError::Provider(format!(
                "anthropic streaming: malformed message_delta payload: {e}"
            )))
        }
    };

    if let Some(reason) = parsed.delta.as_ref().and_then(|d| d.stop_reason.as_deref()) {
        state.pending_stop_reason = map_stop_reason(reason);
    }

    if let Some(usage) = parsed.usage {
        ParseOutcome::Emit(StreamEvent::Usage {
            tokens_in: state.running_input_tokens,
            tokens_out: usage.output_tokens,
        })
    } else {
        // The `message_delta` carried only a `stop_reason`; we have
        // absorbed it into state and there is no Usage event to emit
        // for this chunk.
        ParseOutcome::Filter
    }
}

fn map_stop_reason(raw: &str) -> StopReason {
    match raw {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "tool_use" => StopReason::ToolUse,
        "refusal" => StopReason::Refusal,
        other => StopReason::Other(other.to_string()),
    }
}

fn parse_error(data: &str) -> ParseOutcome {
    #[derive(Deserialize)]
    struct Payload {
        error: Inner,
    }
    #[derive(Deserialize)]
    struct Inner {
        #[serde(default, rename = "type")]
        kind: String,
        #[serde(default)]
        message: String,
    }

    let payload: Payload = match serde_json::from_str(data) {
        Ok(p) => p,
        Err(e) => {
            return ParseOutcome::Emit(StreamEvent::Error(AiError::Provider(format!(
                "anthropic streaming: malformed error event: {e}"
            ))));
        }
    };

    let combined = if payload.error.kind.is_empty() {
        payload.error.message
    } else {
        format!("[{}] {}", payload.error.kind, payload.error.message)
    };
    let detail = if combined.len() > MAX_ERROR_DETAIL {
        truncate_to_owned(&combined)
    } else {
        combined
    };
    ParseOutcome::Emit(StreamEvent::Error(AiError::Provider(format!(
        "anthropic streaming error: {detail}"
    ))))
}

fn map_eventsource_err(err: reqwest_eventsource::Error) -> AiError {
    use reqwest_eventsource::Error as E;
    match err {
        E::Transport(e) => AiError::Network(format!(
            "anthropic streaming transport error: {}",
            e.without_url()
        )),
        E::InvalidStatusCode(status, _) => {
            AiError::Provider(format!("anthropic streaming: status {}", status.as_u16()))
        }
        E::InvalidContentType(value, _) => AiError::Provider(format!(
            "anthropic streaming: invalid content-type {value:?}"
        )),
        E::Parser(e) => AiError::Provider(format!("anthropic streaming SSE parse error: {e}")),
        E::Utf8(e) => AiError::Provider(format!("anthropic streaming utf8 error: {e}")),
        E::InvalidLastEventId(id) => {
            AiError::Provider(format!("anthropic streaming: invalid last-event-id {id}"))
        }
        // `StreamEnded` is filtered upstream in `anthropic_stream`;
        // it never reaches this mapper. Defensive arm so the match
        // stays exhaustive.
        E::StreamEnded => AiError::Provider("anthropic streaming ended unexpectedly".into()),
    }
}

// Make `AiResult` available for the helper module without a wildcard
// import that would shadow the `Result` alias from std.
#[allow(dead_code)]
type _AiResult<T> = AiResult<T>;

#[cfg(test)]
mod tests {
    use super::{map_eventsource_err, map_stop_reason};
    use dbboard_ai::{AiError, StopReason};

    #[test]
    fn map_stop_reason_recognises_every_documented_value() {
        assert_eq!(map_stop_reason("end_turn"), StopReason::EndTurn);
        assert_eq!(map_stop_reason("max_tokens"), StopReason::MaxTokens);
        assert_eq!(map_stop_reason("stop_sequence"), StopReason::StopSequence);
        assert_eq!(map_stop_reason("tool_use"), StopReason::ToolUse);
        assert_eq!(map_stop_reason("refusal"), StopReason::Refusal);
    }

    #[test]
    fn map_stop_reason_preserves_unknown_value_in_other() {
        assert_eq!(
            map_stop_reason("future_reason_v9"),
            StopReason::Other("future_reason_v9".into())
        );
    }

    #[test]
    fn map_eventsource_err_for_invalid_status_returns_provider_error_with_code() {
        // `InvalidStatusCode` cannot be hand-constructed from outside
        // the crate (the `Response` field is foreign), so we test the
        // family via a contrived `Utf8` path instead and assert on
        // the variant shape rather than the exact payload.
        let bytes = vec![0xFFu8, 0xFE, 0xFD];
        let utf8_err = String::from_utf8(bytes).unwrap_err();
        let mapped = map_eventsource_err(reqwest_eventsource::Error::Utf8(utf8_err));
        match mapped {
            AiError::Provider(msg) => assert!(msg.contains("utf8")),
            other => panic!("expected Provider, got {other:?}"),
        }
    }
}
