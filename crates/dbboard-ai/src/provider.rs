//! The `AiProvider` trait — the contract every concrete AI backend
//! implements (ADR-0023 Decision 2).
//!
//! Designed to mirror [`dbboard_core::DatabaseAdapter`]: a small
//! required surface (id, capabilities, two Stage 1 commands) that the
//! UI worker calls through `Arc<dyn AiProvider>`. `async-trait`
//! desugars the async methods to `Pin<Box<dyn Future>>` so the trait
//! is object-safe; the same constraint that applies to
//! `DatabaseAdapter` applies here.

use async_trait::async_trait;
use futures_util::stream;

use crate::capabilities::AiCapabilities;
use crate::error::AiResult;
use crate::request::{AiResponse, ExplainRequest, SuggestRequest};
use crate::stream::{AiStream, StopReason, StreamEvent};

#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Stable provider identifier (e.g. `"anthropic"`). Used for
    /// history labels and a future provider picker. Constant per
    /// provider, hence the `'static` bound.
    fn id(&self) -> &'static str;

    /// Capability flags advertised by this provider. Stage 1 providers
    /// return defaults (all-false); flags flip on as Stage 2
    /// capabilities are wired.
    fn capabilities(&self) -> AiCapabilities;

    /// Provider + model identity as a `(provider_id, model_id)` tuple
    /// (ADR-0027 Decision 4). `provider_id` is the same string as
    /// [`Self::id`] and stays `'static` because a provider's identifier
    /// is a compile-time constant. `model_id` is a borrowed string:
    /// concrete providers hold the model in a `String` field so a
    /// runtime-configured model (e.g. Anthropic's per-instance model)
    /// surfaces here without an allocation on every call.
    ///
    /// The default impl returns `("unknown", "")` so trait mocks that
    /// only care about `id()` / `capabilities()` compile unchanged.
    /// Any provider whose responses eventually land in `history.jsonl`
    /// SHOULD override this — the worker snapshots the tuple at task
    /// spawn time and stamps every terminal reply with it (spawn-time
    /// identity is the contract: the AI provider slot can swap
    /// mid-request via `AiProviderSwitcher`, but the user gets the
    /// identity they submitted against).
    fn identity(&self) -> (&'static str, &str) {
        ("unknown", "")
    }

    /// Explain a single SQL statement in natural language. The Stage 1
    /// surface intentionally does not pass a schema snapshot — an
    /// explanation of known SQL does not need the table list.
    async fn explain(&self, req: &ExplainRequest) -> AiResult<AiResponse>;

    /// Suggest a SQL statement from a natural-language prompt and the
    /// current `list_tables()` snapshot. Full DDL extraction is a
    /// Stage 2 concern (ADR-0023 §7).
    async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse>;

    /// Streaming variant of [`AiProvider::explain`] (ADR-0026
    /// Decision 1). The default body delegates to `explain` and
    /// wraps the atomic response as a 3-event stream
    /// (`TextDelta` + `Usage` + `MessageStop { EndTurn }`), so
    /// providers that do not implement real streaming still satisfy
    /// the contract — they just emit one chunk.
    ///
    /// Providers that advertise [`AiCapabilities::has_streaming`]
    /// `= true` MUST override this with a real streaming
    /// implementation (ADR-0026 Decision 8). The default impl exists
    /// to keep the trait additive: existing providers (and any
    /// future non-Anthropic backend) compile against the new
    /// surface without modification.
    async fn stream_explain(&self, req: &ExplainRequest) -> AiResult<AiStream> {
        let resp = self.explain(req).await?;
        Ok(Box::pin(stream::iter(default_chunks_from_response(resp))))
    }

    /// Streaming variant of [`AiProvider::suggest_sql`]. Same
    /// default-impl shape as [`AiProvider::stream_explain`].
    async fn stream_suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiStream> {
        let resp = self.suggest_sql(req).await?;
        Ok(Box::pin(stream::iter(default_chunks_from_response(resp))))
    }
}

/// Turn one atomic [`AiResponse`] into the 3-event sequence the
/// default streaming impls emit. Pulled out so both
/// `stream_explain` and `stream_suggest_sql` can call it without
/// duplicating the event list — the wrapper exists purely to keep
/// the trait body slim.
fn default_chunks_from_response(resp: AiResponse) -> Vec<AiResult<StreamEvent>> {
    vec![
        Ok(StreamEvent::TextDelta(resp.text)),
        Ok(StreamEvent::Usage {
            tokens_in: resp.tokens_in,
            tokens_out: resp.tokens_out,
        }),
        Ok(StreamEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use futures_util::StreamExt;

    use super::AiProvider;
    use crate::capabilities::AiCapabilities;
    use crate::error::{AiError, AiResult};
    use crate::request::{AiResponse, ExplainRequest, SuggestRequest};
    use crate::stream::{StopReason, StreamEvent};

    /// Minimal in-test provider used both for object-safety checking
    /// and for behavioural assertions on the trait surface.
    struct StubProvider {
        id: &'static str,
        caps: AiCapabilities,
    }

    #[async_trait]
    impl AiProvider for StubProvider {
        fn id(&self) -> &'static str {
            self.id
        }
        fn capabilities(&self) -> AiCapabilities {
            self.caps
        }
        async fn explain(&self, req: &ExplainRequest) -> AiResult<AiResponse> {
            Ok(AiResponse {
                text: format!("explained: {}", req.sql),
                tokens_in: u32::try_from(req.sql.len()).unwrap_or(u32::MAX),
                tokens_out: 1,
                provider: "unknown".into(),
                model: String::new(),
            })
        }
        async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse> {
            Ok(AiResponse {
                text: format!("suggested for: {}", req.prompt),
                tokens_in: u32::try_from(req.prompt.len()).unwrap_or(u32::MAX),
                tokens_out: u32::try_from(req.schema.len()).unwrap_or(u32::MAX),
                provider: "unknown".into(),
                model: String::new(),
            })
        }
    }

    /// Errors-only stub used to confirm that an `AiError` propagating
    /// out of an `Arc<dyn AiProvider>` keeps its variant.
    struct FailingProvider;

    #[async_trait]
    impl AiProvider for FailingProvider {
        fn id(&self) -> &'static str {
            "failing"
        }
        fn capabilities(&self) -> AiCapabilities {
            AiCapabilities::default()
        }
        async fn explain(&self, _req: &ExplainRequest) -> AiResult<AiResponse> {
            Err(AiError::Cancelled)
        }
        async fn suggest_sql(&self, _req: &SuggestRequest) -> AiResult<AiResponse> {
            Err(AiError::Quota("daily cap".into()))
        }
    }

    #[test]
    fn provider_is_object_safe_behind_arc_dyn() {
        // Compile-time check: if `AiProvider` were not object-safe,
        // this line would not type-check.
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            id: "stub",
            caps: AiCapabilities::default(),
        });
        assert_eq!(provider.id(), "stub");
        assert_eq!(provider.capabilities(), AiCapabilities::default());
    }

    #[test]
    fn identity_default_impl_returns_unknown_and_empty_model() {
        // ADR-0027 Decision 4 / §Implementation Slice b: providers that
        // do not override `identity()` (typically test mocks or a
        // future non-persisting provider) get `("unknown", "")`. The
        // worker propagates this straight through, so an unset identity
        // still round-trips as a valid record — no panics, no
        // history-write failures.
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            id: "stub",
            caps: AiCapabilities::default(),
        });
        let (p, m) = provider.identity();
        assert_eq!(p, "unknown");
        assert_eq!(m, "");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explain_propagates_request_payload_through_the_trait_call() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            id: "stub",
            caps: AiCapabilities::default(),
        });
        let resp = provider
            .explain(&ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: Some("postgres".into()),
            })
            .await
            .unwrap();
        assert_eq!(resp.text, "explained: SELECT 1");
        assert_eq!(resp.tokens_in, 8);
        assert_eq!(resp.tokens_out, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn suggest_sql_receives_the_schema_snapshot() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            id: "stub",
            caps: AiCapabilities::default(),
        });
        let resp = provider
            .suggest_sql(&SuggestRequest {
                prompt: "active users".into(),
                dialect: Some("postgres".into()),
                schema: vec![
                    dbboard_core::TableInfo::qualified("public", "users"),
                    dbboard_core::TableInfo::qualified("public", "sessions"),
                ],
            })
            .await
            .unwrap();
        assert_eq!(resp.text, "suggested for: active users");
        assert_eq!(resp.tokens_out, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_stream_explain_yields_three_events_delegating_to_atomic_explain() {
        // ADR-0026 Decision 3: the default streaming impl on a
        // provider that has not overridden `stream_explain` must
        // wrap the atomic response as a single TextDelta + Usage +
        // MessageStop sequence. `StubProvider` does not override
        // the streaming methods, so it gets the default body.
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            id: "stub",
            caps: AiCapabilities::default(),
        });
        let stream = provider
            .stream_explain(&ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: Some("postgres".into()),
            })
            .await
            .unwrap();
        let events: Vec<_> = stream.collect().await;
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0],
            Ok(StreamEvent::TextDelta(t)) if t == "explained: SELECT 1"
        ));
        assert!(matches!(
            events[1],
            Ok(StreamEvent::Usage {
                tokens_in: 8,
                tokens_out: 1
            })
        ));
        assert!(matches!(
            events[2],
            Ok(StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_stream_suggest_sql_yields_three_events_delegating_to_atomic_suggest() {
        let provider: Arc<dyn AiProvider> = Arc::new(StubProvider {
            id: "stub",
            caps: AiCapabilities::default(),
        });
        let stream = provider
            .stream_suggest_sql(&SuggestRequest {
                prompt: "active users".into(),
                dialect: None,
                schema: vec![dbboard_core::TableInfo::qualified("public", "users")],
            })
            .await
            .unwrap();
        let events: Vec<_> = stream.collect().await;
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0],
            Ok(StreamEvent::TextDelta(t)) if t == "suggested for: active users"
        ));
        // `StubProvider::suggest_sql` reports `tokens_out` = schema
        // length, so the cumulative Usage event reflects that.
        assert!(matches!(
            events[1],
            Ok(StreamEvent::Usage {
                tokens_in: 12,
                tokens_out: 1
            })
        ));
        assert!(matches!(
            events[2],
            Ok(StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_stream_explain_returns_outer_error_when_atomic_explain_fails() {
        // ADR-0026 Decision 5 / §Default impl: when the underlying
        // atomic call returns Err, the default streaming impl
        // short-circuits and propagates that error through the outer
        // `AiResult<AiStream>` — there is no inner `StreamEvent::Error`
        // chunk. Mid-stream errors live in the wire-protocol path
        // (Anthropic SSE `event: error`), not the default delegate.
        let provider: Arc<dyn AiProvider> = Arc::new(FailingProvider);
        // `AiStream` is a boxed trait object so `AiResult<AiStream>` is
        // not `Debug` on the Ok side — `let Err(_) = ... else` is the
        // idiomatic destructure that does not require Debug.
        let Err(err) = provider
            .stream_explain(&ExplainRequest {
                sql: String::new(),
                dialect: None,
            })
            .await
        else {
            panic!("expected stream_explain to short-circuit with Err");
        };
        assert!(matches!(err, AiError::Cancelled));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn default_stream_suggest_sql_returns_outer_error_when_atomic_suggest_fails() {
        let provider: Arc<dyn AiProvider> = Arc::new(FailingProvider);
        let Err(err) = provider
            .stream_suggest_sql(&SuggestRequest {
                prompt: String::new(),
                dialect: None,
                schema: Vec::new(),
            })
            .await
        else {
            panic!("expected stream_suggest_sql to short-circuit with Err");
        };
        assert!(matches!(err, AiError::Quota(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ai_error_propagates_through_an_arc_dyn_provider() {
        let provider: Arc<dyn AiProvider> = Arc::new(FailingProvider);
        let explain_err = provider
            .explain(&ExplainRequest {
                sql: String::new(),
                dialect: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(explain_err, AiError::Cancelled));

        let suggest_err = provider
            .suggest_sql(&SuggestRequest {
                prompt: String::new(),
                dialect: None,
                schema: Vec::new(),
            })
            .await
            .unwrap_err();
        assert!(matches!(suggest_err, AiError::Quota(_)));
    }
}
