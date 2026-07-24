//! `OpenAI` Chat Completions SSE → normalized [`StreamEvent`] mapping
//! (ADR-0052).
//!
//! Unlike Anthropic's typed SSE (`event: <type>\ndata: <json>`), the
//! Chat Completions stream is **data-only** — every frame is a bare
//! `data: <json>` line with no `event:` field (so `reqwest-eventsource`
//! reports the default `"message"` type), and a literal `data: [DONE]`
//! sentinel terminates the stream:
//!
//! ```text
//! data: {"id":"...","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}
//!
//! data: {"id":"...","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
//!
//! data: {"id":"...","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}
//!
//! data: {"id":"...","choices":[],"usage":{"prompt_tokens":7,"completion_tokens":11}}
//!
//! data: [DONE]
//! ```
//!
//! Mapping decisions:
//!
//! - **No `MessageStart`.** `OpenAI` does not report the input-token
//!   count up front — it only arrives on the final `usage` frame
//!   (requested via `stream_options.include_usage`). The worker already
//!   tolerates a stream without a `MessageStart` (the trait's default
//!   atomic-wrapper stream emits none either), so this provider simply
//!   omits it rather than fabricating a zero.
//! - **`delta.content`** → [`StreamEvent::TextDelta`]. The leading
//!   role-only frame (and any empty-string content) is filtered.
//! - **`finish_reason`** is absorbed into state and surfaced on the
//!   terminal [`StreamEvent::MessageStop`], mirroring how the Anthropic
//!   parser threads `message_delta.stop_reason`.
//! - **Final `usage` frame** → one [`StreamEvent::Usage`] carrying
//!   `prompt_tokens` / `completion_tokens` directly (no threading — the
//!   frame is self-contained).
//! - **`[DONE]`** → terminal [`StreamEvent::MessageStop`]; a natural
//!   `StreamEnded` before `[DONE]` produces the same defensive stop.

use dbboard_ai::{AiError, AiStream, StopReason, StreamEvent};
use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use serde::Deserialize;

use crate::{body_error_detail, MAX_ERROR_DETAIL};

/// `OpenAI`'s `[DONE]` stream terminator (sent as the `data` payload of
/// the final SSE frame, before the connection closes).
const DONE_SENTINEL: &str = "[DONE]";

/// Adapt a [`reqwest_eventsource::EventSource`] into an [`AiStream`].
///
/// The returned stream lifecycle:
/// - emits zero-or-more [`StreamEvent::TextDelta`] for each chunk that
///   carries `choices[0].delta.content`,
/// - emits one [`StreamEvent::Usage`] for the final choices-empty frame
///   that carries `usage` (present because the request sets
///   `stream_options.include_usage`),
/// - emits exactly one [`StreamEvent::MessageStop`] on the `[DONE]`
///   sentinel (or defensively on `StreamEnded` if the server closes the
///   connection without sending `[DONE]`), carrying the last
///   `finish_reason` observed,
/// - propagates any transport-level failure as `Err(AiError)` and
///   terminates the stream.
///
/// Dropping the returned stream cancels the in-flight request via the
/// [`EventSource`] / `reqwest::Response` drop chain — no trait-level
/// cancellation token is required (ADR-0026 Decision 5).
pub(crate) fn openai_stream(es: EventSource) -> AiStream {
    let state = StreamState::new(es);
    let s = futures_util::stream::unfold(state, |mut state| async move {
        if state.closed {
            return None;
        }
        loop {
            // EventSource exhaustion → `unfold` returns `None`. The `?`
            // is on the outer `Option<_>` from `Stream::next` and
            // discards the state value, which is exactly what we want.
            let item = state.es.next().await?;
            match item {
                Ok(Event::Open) => {}
                Ok(Event::Message(msg)) => match parse_message(&msg.data, &mut state) {
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
                    // Server closed the connection without a `[DONE]`
                    // sentinel. Emit a defensive `MessageStop` carrying
                    // the last `finish_reason` we saw (or the EndTurn
                    // default) so the worker gets a clean terminator.
                    state.closed = true;
                    let evt = StreamEvent::MessageStop {
                        stop_reason: state.pending_stop_reason.clone(),
                    };
                    return Some((Ok(evt), state));
                }
                Err(err) => {
                    state.closed = true;
                    // Read any error-response body *before* closing the
                    // EventSource: a non-2xx open carries the reason
                    // (invalid model, quota, …) in the response body,
                    // and dropping the connection would discard it.
                    let mapped = map_stream_error(err).await;
                    state.es.close();
                    return Some((Err(mapped), state));
                }
            }
        }
    });
    Box::pin(s)
}

struct StreamState {
    es: EventSource,
    /// `finish_reason` parsed from the most recent chunk. Defaults to
    /// `EndTurn` so a stream that ends without an explicit
    /// `finish_reason` still produces a sensible `MessageStop`.
    pending_stop_reason: StopReason,
    closed: bool,
}

impl StreamState {
    fn new(es: EventSource) -> Self {
        Self {
            es,
            pending_stop_reason: StopReason::EndTurn,
            closed: false,
        }
    }
}

enum ParseOutcome {
    Emit(StreamEvent),
    Filter,
    /// `[DONE]` sentinel received — caller emits the final
    /// `MessageStop { pending_stop_reason }` chunk and terminates.
    Stop,
    ParseError(AiError),
}

fn parse_message(data: &str, state: &mut StreamState) -> ParseOutcome {
    if data.trim() == DONE_SENTINEL {
        return ParseOutcome::Stop;
    }
    parse_chunk(data, state)
}

fn parse_chunk(data: &str, state: &mut StreamState) -> ParseOutcome {
    #[derive(Deserialize)]
    struct Chunk {
        #[serde(default)]
        choices: Vec<ChunkChoice>,
        #[serde(default)]
        usage: Option<ChunkUsage>,
    }
    #[derive(Deserialize)]
    struct ChunkChoice {
        #[serde(default)]
        delta: Delta,
        #[serde(default)]
        finish_reason: Option<String>,
    }
    #[derive(Default, Deserialize)]
    struct Delta {
        #[serde(default)]
        content: Option<String>,
    }
    #[derive(Deserialize)]
    struct ChunkUsage {
        #[serde(default)]
        prompt_tokens: u32,
        #[serde(default)]
        completion_tokens: u32,
    }

    let chunk: Chunk = match serde_json::from_str(data) {
        Ok(c) => c,
        Err(e) => {
            return ParseOutcome::ParseError(AiError::Provider(format!(
                "openai streaming: malformed chunk payload: {e}"
            )))
        }
    };

    // Absorb the finish reason (if any) into state before deciding what
    // to emit; content and finish_reason arrive on separate frames, so
    // this never competes with a TextDelta emission for the same chunk.
    if let Some(reason) = chunk
        .choices
        .first()
        .and_then(|c| c.finish_reason.as_deref())
    {
        state.pending_stop_reason = map_finish_reason(reason);
    }

    // A content delta is the common case. Empty-string content (and the
    // leading role-only frame) carry nothing for the UI to append.
    if let Some(text) = chunk
        .choices
        .into_iter()
        .find_map(|c| c.delta.content.filter(|t| !t.is_empty()))
    {
        return ParseOutcome::Emit(StreamEvent::TextDelta(text));
    }

    // The final choices-empty frame carries usage (include_usage=true).
    if let Some(usage) = chunk.usage {
        return ParseOutcome::Emit(StreamEvent::Usage {
            tokens_in: usage.prompt_tokens,
            tokens_out: usage.completion_tokens,
        });
    }

    // Role-only frame, or a finish-only frame whose stop reason we just
    // absorbed — nothing to surface for this chunk.
    ParseOutcome::Filter
}

fn map_finish_reason(raw: &str) -> StopReason {
    match raw {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "content_filter" => StopReason::Refusal,
        other => StopReason::Other(other.to_string()),
    }
}

/// Async error mapper for the stream open/read path. Only
/// `InvalidStatusCode` needs `await` — the API returns the failure
/// reason in the response body, which we consume here — so everything
/// else delegates to the synchronous [`map_eventsource_err`].
async fn map_stream_error(err: reqwest_eventsource::Error) -> AiError {
    use reqwest_eventsource::Error as E;
    match err {
        E::InvalidStatusCode(status, response) => {
            let code = status.as_u16();
            let detail = match response.bytes().await {
                Ok(bytes) => body_error_detail(&bytes),
                // Body read failed (connection dropped mid-response):
                // still surface the status so the user isn't left with
                // a silent failure.
                Err(_) => "no response body".to_string(),
            };
            AiError::Provider(format!("openai streaming error (status {code}): {detail}"))
        }
        other => map_eventsource_err(other),
    }
}

fn map_eventsource_err(err: reqwest_eventsource::Error) -> AiError {
    use reqwest_eventsource::Error as E;
    match err {
        E::Transport(e) => AiError::Network(format!(
            "openai streaming transport error: {}",
            e.without_url()
        )),
        // Handled by the async `map_stream_error`, which reads the
        // response body. This arm stays for match exhaustiveness and as
        // a body-less fallback if ever reached synchronously.
        E::InvalidStatusCode(status, _) => {
            AiError::Provider(format!("openai streaming: status {}", status.as_u16()))
        }
        E::InvalidContentType(value, _) => {
            AiError::Provider(format!("openai streaming: invalid content-type {value:?}"))
        }
        E::Parser(e) => AiError::Provider(format!("openai streaming SSE parse error: {e}")),
        E::Utf8(e) => AiError::Provider(format!("openai streaming utf8 error: {e}")),
        E::InvalidLastEventId(id) => {
            AiError::Provider(format!("openai streaming: invalid last-event-id {id}"))
        }
        // `StreamEnded` is filtered upstream in `openai_stream`; it
        // never reaches this mapper. Defensive arm so the match stays
        // exhaustive.
        E::StreamEnded => AiError::Provider("openai streaming ended unexpectedly".into()),
    }
}

// Keep the crate-level error-detail cap referenced from this module so a
// future direct use here does not drift from the atomic path's cap.
#[allow(dead_code)]
const _: usize = MAX_ERROR_DETAIL;

#[cfg(test)]
mod tests {
    use super::{map_eventsource_err, map_finish_reason};
    use dbboard_ai::{AiError, StopReason};

    #[test]
    fn map_finish_reason_recognises_every_documented_value() {
        assert_eq!(map_finish_reason("stop"), StopReason::EndTurn);
        assert_eq!(map_finish_reason("length"), StopReason::MaxTokens);
        assert_eq!(map_finish_reason("tool_calls"), StopReason::ToolUse);
        assert_eq!(map_finish_reason("function_call"), StopReason::ToolUse);
        assert_eq!(map_finish_reason("content_filter"), StopReason::Refusal);
    }

    #[test]
    fn map_finish_reason_preserves_unknown_value_in_other() {
        assert_eq!(
            map_finish_reason("future_reason_v9"),
            StopReason::Other("future_reason_v9".into())
        );
    }

    #[test]
    fn map_eventsource_err_wraps_utf8_as_provider_error() {
        // `InvalidStatusCode` cannot be hand-constructed from outside
        // the crate (the `Response` field is foreign), so we exercise a
        // sibling variant and assert on the mapped shape instead.
        let bytes = vec![0xFFu8, 0xFE, 0xFD];
        let utf8_err = String::from_utf8(bytes).unwrap_err();
        let mapped = map_eventsource_err(reqwest_eventsource::Error::Utf8(utf8_err));
        match mapped {
            AiError::Provider(msg) => assert!(msg.contains("utf8")),
            other => panic!("expected Provider, got {other:?}"),
        }
    }
}
