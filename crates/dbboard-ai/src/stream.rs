//! Streaming surface for the `AiProvider` trait (ADR-0026).
//!
//! Decision 1 adds `stream_explain` / `stream_suggest_sql` to
//! `AiProvider` returning `AiResult<AiStream>`. The stream items are
//! normalized [`StreamEvent`]s rather than raw provider-specific
//! payloads, so the UI layer stays provider-independent — Anthropic's
//! `text_delta`, `message_delta`, `message_stop`, etc. are all mapped
//! into this enum at the provider boundary.
//!
//! Decision 3 enumerates the variants: `MessageStart` (carries the
//! initial input-token snapshot), `TextDelta` (incremental text to
//! append to the accumulated response), `Usage` (cumulative token
//! counters — the UI replaces its meter on each event, never sums),
//! `MessageStop` (end-of-stream marker with a stop reason), and
//! `Error` (a provider-emitted wire-level error that interrupts the
//! stream without tearing down the transport).

use futures_util::stream::BoxStream;

use crate::error::{AiError, AiResult};

/// Boxed, `Send`, `'static` stream of [`StreamEvent`]s returned by
/// the streaming variants of [`crate::AiProvider`].
///
/// The lifetime is `'static` so the stream can cross the
/// `dbboard-ui` worker channel without borrowing from the provider
/// — mirrors the `Arc<dyn AiProvider>` ownership model where the
/// provider itself is owned by an `Arc` rather than passed by
/// reference (ADR-0023 Decision 2 + ADR-0026 Decision 1).
pub type AiStream = BoxStream<'static, AiResult<StreamEvent>>;

/// Normalized streaming event surfaced by [`AiStream`].
///
/// `Clone` + `Eq` are required because the events flow through the
/// `dbboard-ui` worker channel (PR #43 baseline) and the panel's
/// state-machine tests assert on them by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// Initial message envelope. Carries the input-token count
    /// reported at request start (Anthropic surfaces this on
    /// `message_start.usage.input_tokens`).
    MessageStart { tokens_in: u32 },
    /// Incremental text chunk. The UI appends to its accumulated
    /// buffer; ordering within a single stream is guaranteed.
    TextDelta(String),
    /// Cumulative token usage snapshot. Anthropic's
    /// `message_delta.usage.output_tokens` is **cumulative** within
    /// a single message (ADR-0026 Decision 7), so the UI **replaces**
    /// its meter on each event rather than summing.
    Usage { tokens_in: u32, tokens_out: u32 },
    /// End-of-stream marker. Equivalent to Anthropic's
    /// `message_stop`. `stop_reason` is the last value observed on
    /// the preceding `message_delta` event sequence.
    MessageStop { stop_reason: StopReason },
    /// Provider-emitted wire-level error (e.g. Anthropic
    /// `overloaded_error` event). Distinct from a stream-level
    /// transport error, which surfaces as `Err` on the
    /// `AiResult<StreamEvent>` wrapper.
    Error(AiError),
}

/// Why the stream ended. Mirrors Anthropic's `stop_reason` field on
/// the final `message_delta` event, with an `Other(String)` escape
/// hatch so a future provider value (or an Anthropic addition) does
/// not break the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    Refusal,
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::{AiError, AiStream, StopReason, StreamEvent};

    #[test]
    fn stream_event_variants_clone_and_equate() {
        let cases = [
            StreamEvent::MessageStart { tokens_in: 42 },
            StreamEvent::TextDelta("hello".into()),
            StreamEvent::Usage {
                tokens_in: 1,
                tokens_out: 2,
            },
            StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
            },
            StreamEvent::Error(AiError::Cancelled),
        ];
        for evt in cases {
            assert_eq!(evt.clone(), evt);
        }
    }

    #[test]
    fn stream_event_text_delta_distinguishes_payload() {
        assert_ne!(
            StreamEvent::TextDelta("foo".into()),
            StreamEvent::TextDelta("bar".into())
        );
    }

    #[test]
    fn stream_event_error_wraps_each_ai_error_variant() {
        // The `Error` variant must accept every `AiError` shape so
        // a provider can surface any wire error mid-stream without
        // a type widening.
        for err in [
            AiError::Configuration("missing key".into()),
            AiError::Network("timeout".into()),
            AiError::Provider("rate_limit".into()),
            AiError::Quota("daily cap".into()),
            AiError::Cancelled,
        ] {
            let evt = StreamEvent::Error(err.clone());
            assert_eq!(evt, StreamEvent::Error(err));
        }
    }

    #[test]
    fn stop_reason_variants_clone_and_equate() {
        for reason in [
            StopReason::EndTurn,
            StopReason::MaxTokens,
            StopReason::StopSequence,
            StopReason::ToolUse,
            StopReason::Refusal,
            StopReason::Other("custom".into()),
        ] {
            assert_eq!(reason.clone(), reason);
        }
    }

    #[test]
    fn stop_reason_other_distinguishes_payloads() {
        assert_ne!(StopReason::Other("a".into()), StopReason::Other("b".into()));
    }

    #[test]
    fn ai_stream_is_send_and_static() {
        // Compile-time check: the worker channel ferries `AiStream`
        // values across tasks, so the alias must be `Send + 'static`.
        fn assert_send_static<T: Send + 'static>() {}
        assert_send_static::<AiStream>();
    }
}
