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

use crate::capabilities::AiCapabilities;
use crate::error::AiResult;
use crate::request::{AiResponse, ExplainRequest, SuggestRequest};

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

    /// Explain a single SQL statement in natural language. The Stage 1
    /// surface intentionally does not pass a schema snapshot — an
    /// explanation of known SQL does not need the table list.
    async fn explain(&self, req: &ExplainRequest) -> AiResult<AiResponse>;

    /// Suggest a SQL statement from a natural-language prompt and the
    /// current `list_tables()` snapshot. Full DDL extraction is a
    /// Stage 2 concern (ADR-0023 §7).
    async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::AiProvider;
    use crate::capabilities::AiCapabilities;
    use crate::error::{AiError, AiResult};
    use crate::request::{AiResponse, ExplainRequest, SuggestRequest};

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
            })
        }
        async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse> {
            Ok(AiResponse {
                text: format!("suggested for: {}", req.prompt),
                tokens_in: u32::try_from(req.prompt.len()).unwrap_or(u32::MAX),
                tokens_out: u32::try_from(req.schema.len()).unwrap_or(u32::MAX),
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
