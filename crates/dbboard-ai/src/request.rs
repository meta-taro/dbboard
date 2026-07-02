//! Request and response value types for the Stage 1 AI surface
//! (ADR-0023 Decision 7).
//!
//! The Stage 1 commands are `explain` (SQL → natural-language
//! explanation) and `suggest_sql` (prompt + schema snapshot → SQL).
//! Both return [`AiResponse`].
//!
//! `dialect` is an optional hint (`"postgres"`, `"sqlite"`, `"d1-sql"`,
//! …) derived from the active adapter's `id()` so the provider can
//! tailor its output. `schema` on `SuggestRequest` carries the current
//! `list_tables()` result; `full_schema` (ADR-0028 Decision 8) carries
//! the per-table `describe_table` results when the UI's
//! "Include column details" toggle is on.

use dbboard_core::{TableInfo, TableSchema};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainRequest {
    pub sql: String,
    pub dialect: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestRequest {
    pub prompt: String,
    pub dialect: Option<String>,
    pub schema: Vec<TableInfo>,
    /// Full per-table column + primary-key descriptions (ADR-0028
    /// Decision 8). `None` (or `Some` with an empty vec) means the
    /// caller only has table names — providers fall back to rendering
    /// [`Self::schema`]. When non-empty, providers prefer this over
    /// `schema`. Additive: existing callers pass `None` and behave
    /// exactly as before.
    pub full_schema: Option<Vec<TableSchema>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiResponse {
    pub text: String,
    /// Input token count reported by the provider. The Stage 1 UI does
    /// not display this, but it is recorded for future cost-meter and
    /// `AiError::Quota` wiring.
    pub tokens_in: u32,
    pub tokens_out: u32,
    /// Stable provider identifier that produced this response
    /// (ADR-0027). Mirrors [`super::AiProvider::identity`]'s first
    /// tuple element. Populated so callers holding only an `AiResponse`
    /// can stamp history without a second trait call. The `dbboard-ui`
    /// worker uses its spawn-time `identity()` snapshot rather than
    /// this field when composing terminal replies (spawn-time identity
    /// is the contract, per ADR-0027 §Implementation Slice b), but the
    /// value here is the same for the atomic path.
    pub provider: String,
    /// Model identifier that produced this response (e.g.
    /// `"claude-sonnet-4-6"`). Mirrors `identity()`'s second tuple
    /// element. See [`Self::provider`] for the rationale.
    pub model: String,
}

#[cfg(test)]
mod tests {
    use super::{AiResponse, ExplainRequest, SuggestRequest};
    use dbboard_core::{ColumnInfo, TableInfo, TableSchema};

    #[test]
    fn explain_request_holds_sql_and_optional_dialect() {
        let with_hint = ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: Some("postgres".into()),
        };
        let without_hint = ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: None,
        };
        assert_eq!(with_hint.sql, "SELECT 1");
        assert_eq!(with_hint.dialect.as_deref(), Some("postgres"));
        assert!(without_hint.dialect.is_none());
    }

    #[test]
    fn suggest_request_carries_a_schema_snapshot() {
        let req = SuggestRequest {
            prompt: "monthly active users".into(),
            dialect: Some("postgres".into()),
            schema: vec![
                TableInfo::qualified("public", "users"),
                TableInfo::qualified("public", "sessions"),
            ],
            full_schema: None,
        };
        assert_eq!(req.schema.len(), 2);
        assert_eq!(req.schema[0].name, "users");
        assert_eq!(req.schema[1].schema.as_deref(), Some("public"));
        assert!(req.full_schema.is_none());
    }

    #[test]
    fn suggest_request_carries_optional_full_schema_descriptions() {
        // ADR-0028 Decision 8: `full_schema` rides alongside the
        // names-only `schema` list; both may be present and the
        // provider prefers `full_schema` when non-empty.
        let table = TableInfo::qualified("public", "users");
        let req = SuggestRequest {
            prompt: "monthly active users".into(),
            dialect: Some("postgres".into()),
            schema: vec![table.clone()],
            full_schema: Some(vec![TableSchema {
                table,
                columns: vec![ColumnInfo {
                    name: "id".into(),
                    declared_type: Some("integer".into()),
                    nullable: false,
                    primary_key: true,
                    ordinal: 1,
                    default_value: Some("nextval('users_id_seq'::regclass)".into()),
                }],
                primary_key: vec!["id".into()],
            }]),
        };
        let full = req.full_schema.as_ref().expect("full schema present");
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].columns[0].name, "id");
        assert_eq!(full[0].primary_key, vec!["id".to_owned()]);
        // Clone still works for channel dispatch with the new field.
        assert_eq!(req.clone(), req);
    }

    #[test]
    fn ai_response_carries_text_and_token_counts() {
        let resp = AiResponse {
            text: "this query selects one row".into(),
            tokens_in: 42,
            tokens_out: 7,
            provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
        };
        assert_eq!(resp.text, "this query selects one row");
        assert_eq!(resp.tokens_in, 42);
        assert_eq!(resp.tokens_out, 7);
    }

    #[test]
    fn ai_response_carries_provider_and_model_identity() {
        let resp = AiResponse {
            text: "hi".into(),
            tokens_in: 0,
            tokens_out: 0,
            provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
        };
        assert_eq!(resp.provider, "anthropic");
        assert_eq!(resp.model, "claude-sonnet-4-6");
    }

    #[test]
    fn request_and_response_clone_for_dispatching_through_channels() {
        let req = ExplainRequest {
            sql: "SELECT 1".into(),
            dialect: None,
        };
        let req2 = req.clone();
        assert_eq!(req, req2);

        let resp = AiResponse {
            text: "ok".into(),
            tokens_in: 1,
            tokens_out: 1,
            provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
        };
        let resp2 = resp.clone();
        assert_eq!(resp, resp2);
    }
}
