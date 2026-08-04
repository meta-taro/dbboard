//! `OpenAI` Chat Completions API provider for the `dbboard-ai` trait
//! (ADR-0052).
//!
//! Second concrete provider after `dbboard-anthropic`. Same surface
//! (`explain`, `suggest_sql`, plus their streaming variants) routed
//! through `POST /v1/chat/completions`. It mirrors the Anthropic
//! provider's shape deliberately so the two stay easy to diff, but the
//! wire protocol differs on three axes:
//!
//! - **Auth**: `Authorization: Bearer <key>` rather than Anthropic's
//!   `x-api-key` + `anthropic-version` header pair.
//! - **System prompt**: a `{"role":"system"}` entry at the head of the
//!   `messages` array, not a separate top-level `system` field.
//! - **Usage**: `usage.prompt_tokens` / `usage.completion_tokens`
//!   rather than `input_tokens` / `output_tokens`.
//!
//! Dependency rule (ADR-0023 Decision 1 / ADR-0052): this crate depends
//! on `dbboard-ai` plus `reqwest` / `serde` / `serde_json`. It never
//! depends on `dbboard-core` directly; `TableInfo` is consumed through
//! `dbboard-ai`'s re-export.
//!
//! No `max_tokens` is sent. gpt-4o accepts `max_tokens`, but the
//! newer o-series and gpt-5 models reject it in favour of
//! `max_completion_tokens`; omitting the cap keeps *any* model id the
//! user types working against the API's own default. The system prompts
//! ask for concise output, so an unbounded completion is not a runaway
//! risk here (ADR-0052).

use std::fmt::Write as _;

use async_trait::async_trait;
use dbboard_ai::{
    AiCapabilities, AiError, AiProvider, AiResponse, AiResult, AiStream, ExplainRequest,
    SuggestRequest, TableInfo, TableSchema,
};
use serde::{Deserialize, Serialize};

mod stream;

const PROVIDER_ID: &str = "openai";
const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const DEFAULT_MODEL: &str = "gpt-4o";

/// Cap on error/body text surfaced into an [`AiError`], so a hostile or
/// runaway response cannot dump an unbounded string into the UI.
const MAX_ERROR_DETAIL: usize = 2048;

const EXPLAIN_SYSTEM_PROMPT: &str = "You are a SQL expert. Explain the given SQL statement \
    in plain English. Be concise (1-3 sentences) and do not restate the SQL verbatim.";

const SUGGEST_SYSTEM_PROMPT: &str =
    "You are a SQL expert. Generate a single SQL statement that answers the user's request, \
    using only the tables listed. Reply with the SQL only — no commentary, no markdown fences.";

/// Configuration for [`OpenAiProvider`]. `base_url` is `None` in
/// production; tests override it to point at a wiremock server.
#[derive(Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

/// `OpenAI` Chat Completions API client implementing [`AiProvider`].
///
/// The API key is held privately and never appears in `Debug` output,
/// log lines, or error messages.
pub struct OpenAiProvider {
    client: reqwest::Client,
    completions_url: String,
    model: String,
    // Kept private and never surfaced in Debug or errors.
    api_key: String,
}

impl OpenAiProvider {
    /// Build a provider with an explicit model id against the
    /// production `OpenAI` endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`AiError::Configuration`] when `api_key` or `model` is
    /// empty (or whitespace only), or when the HTTP client fails to
    /// initialise (e.g. the TLS backend cannot start).
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> AiResult<Self> {
        Self::with_config(OpenAiConfig {
            api_key: api_key.into(),
            model: model.into(),
            base_url: None,
        })
    }

    /// Build a provider against the [`DEFAULT_MODEL`] constant
    /// (`gpt-4o`). Used when the stored entry leaves the model field
    /// empty; any typed model id overrides it via [`Self::new`].
    ///
    /// # Errors
    ///
    /// Same as [`Self::new`].
    pub fn with_default_model(api_key: impl Into<String>) -> AiResult<Self> {
        Self::new(api_key, DEFAULT_MODEL)
    }

    /// Build a provider from a full [`OpenAiConfig`].
    ///
    /// # Errors
    ///
    /// See [`Self::new`].
    pub fn with_config(config: OpenAiConfig) -> AiResult<Self> {
        if config.api_key.trim().is_empty() {
            return Err(AiError::Configuration("openai api key is empty".into()));
        }
        if config.model.trim().is_empty() {
            return Err(AiError::Configuration("openai model id is empty".into()));
        }

        let base = config.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);
        // Pin rustls explicitly and refuse plaintext: the API key must
        // never travel over a non-TLS connection. The localhost
        // exception exists so wiremock-backed tests (which can only
        // bind plaintext loopback) still reach the client.
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(!is_localhost(base))
            .build()
            .map_err(|e| AiError::Configuration(e.to_string()))?;
        let completions_url = build_completions_url(base);
        Ok(Self {
            client,
            completions_url,
            model: config.model,
            api_key: config.api_key,
        })
    }

    async fn call_completions(&self, body: ChatRequest) -> AiResult<AiResponse> {
        let response = self
            .client
            .post(&self.completions_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(transport_error)?;

        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(transport_error)?;

        if (200..300).contains(&status) {
            let parsed: ChatResponse = serde_json::from_slice(&bytes).map_err(|e| {
                AiError::Provider(format!(
                    "malformed openai response (status {status}): {e} [{}]",
                    truncate_to_owned(&String::from_utf8_lossy(&bytes))
                ))
            })?;
            parsed_to_response(parsed, &self.model)
        } else {
            Err(error_from_status_and_body(status, &bytes))
        }
    }

    /// Open an SSE stream against `POST /v1/chat/completions` with
    /// `"stream": true` and `stream_options.include_usage = true` (so
    /// the token counters arrive on a final choices-empty frame). Per
    /// ADR-0026 Decision 4 the request uses [`reqwest_eventsource`] with
    /// `RetryPolicy::Never` so a token-billed POST is never silently
    /// retried.
    ///
    /// The pre-stream error path returns `Err(AiError)` on the outer
    /// future (config / transport setup); wire-level errors after the
    /// connection opens surface as `Ok(StreamEvent::Error)` chunks
    /// inside the stream.
    fn open_stream(&self, body: &ChatRequest) -> AiResult<AiStream> {
        use reqwest_eventsource::{retry, RequestBuilderExt};

        let request = self
            .client
            .post(&self.completions_url)
            .bearer_auth(&self.api_key)
            .json(body);

        let mut es = request.eventsource().map_err(|e| {
            AiError::Configuration(format!("openai streaming: cannot prepare request: {e}"))
        })?;
        // Never retry a token-billed POST — CLAUDE.md / ADR-0026
        // Decision 4. The default policy is exponential back-off,
        // which would silently double-charge on transient 5xx.
        es.set_retry_policy(Box::new(retry::Never));

        Ok(stream::openai_stream(es))
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> AiCapabilities {
        // Full streaming parity with Anthropic (ADR-0052). Function
        // calling stays off — it is explicitly out of scope.
        AiCapabilities {
            has_streaming: true,
            has_function_calling: false,
        }
    }

    // ADR-0027 Decision 4: `(provider_id, model_id)` for history
    // stamping. `PROVIDER_ID` is a compile-time constant; `model` is a
    // per-instance `String` (constructor-time), so we hand back a
    // borrow into the provider. See `dbboard-anthropic` for the full
    // rationale on the worker's spawn-time snapshot.
    fn identity(&self) -> (&'static str, &str) {
        (PROVIDER_ID, &self.model)
    }

    async fn explain(&self, req: &ExplainRequest) -> AiResult<AiResponse> {
        self.call_completions(build_explain_request(&self.model, req))
            .await
    }

    async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse> {
        self.call_completions(build_suggest_request(&self.model, req))
            .await
    }

    async fn stream_explain(&self, req: &ExplainRequest) -> AiResult<AiStream> {
        let mut body = build_explain_request(&self.model, req);
        enable_streaming(&mut body);
        self.open_stream(&body)
    }

    async fn stream_suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiStream> {
        let mut body = build_suggest_request(&self.model, req);
        enable_streaming(&mut body);
        self.open_stream(&body)
    }
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `finish_non_exhaustive` lets clippy's
        // `missing_fields_in_debug` lint pass without forcing us to
        // surface the `reqwest::Client` (whose own Debug carries no
        // useful information for this struct).
        f.debug_struct("OpenAiProvider")
            .field("completions_url", &self.completions_url)
            .field("model", &self.model)
            // Never expose the API key, even if a future change starts
            // logging this struct or wiring it into an error envelope.
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

// --- wire types -----------------------------------------------------------

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<RequestMessage>,
    /// `true` when the request opens an SSE stream. Skipped from
    /// serialization when `false` so non-streaming requests produce a
    /// clean body (preserving the round-trip tests).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    /// Only present on streaming requests: asks the API to append a
    /// final choices-empty frame carrying `usage`, so the token meter
    /// can be filled from the stream instead of a second call.
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct RequestMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Debug, Default, Deserialize)]
struct Choice {
    #[serde(default)]
    message: ChoiceMessage,
}

#[derive(Debug, Default, Deserialize)]
struct ChoiceMessage {
    // `content` is `null` on a tool-call-only choice; Stage 1 only
    // consumes text, so a missing/`null` content is treated as empty.
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
}

// --- helpers --------------------------------------------------------------

fn build_completions_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/v1/chat/completions")
}

/// Flip a prepared request into streaming mode: `stream: true` plus
/// `stream_options.include_usage` so the final usage frame is emitted.
fn enable_streaming(body: &mut ChatRequest) {
    body.stream = true;
    body.stream_options = Some(StreamOptions {
        include_usage: true,
    });
}

fn build_explain_request(model: &str, req: &ExplainRequest) -> ChatRequest {
    let user_text = match req.dialect.as_deref() {
        Some(dialect) => format!("SQL ({dialect}):\n{}", req.sql),
        None => format!("SQL:\n{}", req.sql),
    };
    ChatRequest {
        model: model.to_string(),
        messages: vec![
            RequestMessage {
                role: "system",
                content: EXPLAIN_SYSTEM_PROMPT.into(),
            },
            RequestMessage {
                role: "user",
                content: user_text,
            },
        ],
        stream: false,
        stream_options: None,
    }
}

fn build_suggest_request(model: &str, req: &SuggestRequest) -> ChatRequest {
    let mut user_text = String::from("Tables:\n");
    match req.full_schema.as_deref() {
        // ADR-0028 Decision 8: prefer the full per-table descriptions
        // when the caller prefetched them; an empty vec means the
        // prefetch produced nothing usable, so fall back to names.
        Some(full) if !full.is_empty() => {
            for schema in full {
                render_table_schema(&mut user_text, schema);
            }
        }
        _ if req.schema.is_empty() => {
            user_text.push_str("(no tables introspected)\n");
        }
        _ => {
            for table in &req.schema {
                // `writeln!` to String is infallible; the discard is to
                // satisfy `#[must_use]` on `Result` without a noisy
                // `.unwrap()` chain.
                let _ = writeln!(user_text, "- {}", qualify(table));
            }
        }
    }
    if let Some(dialect) = req.dialect.as_deref() {
        let _ = writeln!(user_text, "\nDialect: {dialect}");
    }
    user_text.push_str("\nRequest: ");
    user_text.push_str(&req.prompt);

    ChatRequest {
        model: model.to_string(),
        messages: vec![
            RequestMessage {
                role: "system",
                content: SUGGEST_SYSTEM_PROMPT.into(),
            },
            RequestMessage {
                role: "user",
                content: user_text,
            },
        ],
        stream: false,
        stream_options: None,
    }
}

/// Compact `CREATE TABLE`-ish rendering of one [`TableSchema`]
/// (ADR-0028 Decision 8). This is a prompt hint, not valid DDL —
/// fidelity beats parseability, so `declared_type` / `default_value`
/// are passed through as the engine's raw text and a missing type is
/// simply omitted rather than guessed.
fn render_table_schema(out: &mut String, schema: &TableSchema) {
    let mut lines: Vec<String> = schema
        .columns
        .iter()
        .map(|column| {
            let mut line = format!("  {}", column.name);
            if let Some(declared) = column.declared_type.as_deref() {
                let _ = write!(line, " {declared}");
            }
            if !column.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = column.default_value.as_deref() {
                let _ = write!(line, " DEFAULT {default}");
            }
            line
        })
        .collect();
    if !schema.primary_key.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", schema.primary_key.join(", ")));
    }
    let _ = writeln!(
        out,
        "CREATE TABLE {} (\n{}\n);",
        qualify(&schema.table),
        lines.join(",\n")
    );
}

fn qualify(table: &TableInfo) -> String {
    match &table.schema {
        Some(schema) => format!("{schema}.{}", table.name),
        None => table.name.clone(),
    }
}

fn parsed_to_response(parsed: ChatResponse, model: &str) -> AiResult<AiResponse> {
    let mut combined = String::new();
    for choice in parsed.choices {
        if let Some(text) = choice.message.content {
            combined.push_str(&text);
        }
    }
    if combined.is_empty() {
        return Err(AiError::Provider(
            "openai returned no text content in response".into(),
        ));
    }
    Ok(AiResponse {
        text: combined,
        tokens_in: parsed.usage.prompt_tokens,
        tokens_out: parsed.usage.completion_tokens,
        // ADR-0027 Decision 4: stamp the atomic response with the
        // model that produced it. The provider id is the crate-level
        // constant so out-of-band callers can pipe an `AiResponse`
        // straight into a history record without a second trait call.
        provider: PROVIDER_ID.to_string(),
        model: model.to_string(),
    })
}

/// Map an HTTP error response to an [`AiError::Provider`]. Per ADR-0023
/// §8 + issue 0005 acceptance: every 4xx/5xx surfaces as `Provider`;
/// the Stage 1 design trusts construction-time validation, so a 401
/// from a runtime-rejected key is still a `Provider` error rather than
/// re-raising as `Configuration`.
fn error_from_status_and_body(status: u16, body: &[u8]) -> AiError {
    AiError::Provider(format!(
        "openai api error (status {status}): {}",
        body_error_detail(body)
    ))
}

/// Extract a human-readable reason from an API error response body,
/// preferring the structured `{ "error": { … } }` envelope and falling
/// back to the raw (truncated) text when the body is not JSON. Shared by
/// the atomic and streaming error paths so both surface the same detail
/// (e.g. "You exceeded your current quota") instead of a bare status
/// code.
pub(crate) fn body_error_detail(body: &[u8]) -> String {
    parse_error_envelope(body).unwrap_or_else(|| truncate_to_owned(&String::from_utf8_lossy(body)))
}

fn parse_error_envelope(body: &[u8]) -> Option<String> {
    let envelope: ErrorEnvelope = serde_json::from_slice(body).ok()?;
    let api_err = envelope.error?;
    let combined = if api_err.kind.is_empty() {
        api_err.message
    } else {
        format!("[{}] {}", api_err.kind, api_err.message)
    };
    Some(truncate_to_owned(&combined))
}

/// Map a reqwest transport failure onto [`AiError::Network`], scrubbing
/// the request URL out of the error message so it cannot leak into
/// future log lines.
fn transport_error(err: reqwest::Error) -> AiError {
    // `is_timeout` borrows; `without_url` moves. Capture the timeout
    // flag first so the scrubbed message can still be built afterwards.
    let timed_out = err.is_timeout();
    let scrubbed = err.without_url().to_string();
    if timed_out {
        AiError::Network(format!("openai request timed out: {scrubbed}"))
    } else {
        AiError::Network(format!("openai transport error: {scrubbed}"))
    }
}

fn truncate_to_owned(text: &str) -> String {
    if text.len() <= MAX_ERROR_DETAIL {
        return text.to_string();
    }
    let mut end = MAX_ERROR_DETAIL;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn is_localhost(base: &str) -> bool {
    // wiremock binds plaintext loopback; allow that one case so tests
    // can drive the client without surrendering the production
    // `https_only` guard for arbitrary URLs.
    base.starts_with("http://127.0.0.1")
        || base.starts_with("http://localhost")
        || base.starts_with("http://[::1]")
}

#[cfg(test)]
mod tests {
    use super::{
        build_completions_url, build_explain_request, build_suggest_request,
        error_from_status_and_body, is_localhost, parsed_to_response, qualify, truncate_to_owned,
        ChatResponse, OpenAiProvider, DEFAULT_BASE_URL, DEFAULT_MODEL, MAX_ERROR_DETAIL,
        PROVIDER_ID,
    };
    use dbboard_ai::{AiError, AiProvider, ExplainRequest, SuggestRequest, TableInfo};
    use serde_json::json;

    #[test]
    fn provider_id_constant_is_openai() {
        assert_eq!(PROVIDER_ID, "openai");
    }

    #[test]
    fn default_model_is_gpt_4o() {
        assert_eq!(DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn with_default_model_constructs_against_the_default_model() {
        let provider = OpenAiProvider::with_default_model("test-key").expect("construct");
        assert_eq!(provider.model, DEFAULT_MODEL);
        assert_eq!(provider.id(), "openai");
        // Full streaming parity with Anthropic (ADR-0052); function
        // calling stays off (out of scope).
        let caps = provider.capabilities();
        assert!(caps.has_streaming);
        assert!(!caps.has_function_calling);
    }

    #[test]
    fn identity_returns_provider_id_and_configured_model() {
        // ADR-0027 Decision 4: `identity()` is the source of truth the
        // `dbboard-ui` worker snapshots at task-spawn time and stamps on
        // every terminal reply. A custom model surfaces here without
        // re-plumbing.
        let provider = OpenAiProvider::new("test-key", "gpt-x").expect("construct");
        let (p, m) = provider.identity();
        assert_eq!(p, PROVIDER_ID);
        assert_eq!(m, "gpt-x");

        let default_provider = OpenAiProvider::with_default_model("test-key").expect("construct");
        let (p2, m2) = default_provider.identity();
        assert_eq!(p2, PROVIDER_ID);
        assert_eq!(m2, DEFAULT_MODEL);
    }

    #[test]
    fn new_with_empty_api_key_is_configuration_error() {
        let err = OpenAiProvider::new("", "gpt-x").unwrap_err();
        assert!(matches!(err, AiError::Configuration(_)));
    }

    #[test]
    fn new_with_whitespace_api_key_is_configuration_error() {
        let err = OpenAiProvider::new("   ", "gpt-x").unwrap_err();
        assert!(matches!(err, AiError::Configuration(_)));
    }

    #[test]
    fn new_with_empty_model_is_configuration_error() {
        let err = OpenAiProvider::new("test-key", "").unwrap_err();
        assert!(matches!(err, AiError::Configuration(_)));
    }

    #[test]
    fn build_completions_url_uses_default_base() {
        assert_eq!(
            build_completions_url(DEFAULT_BASE_URL),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn build_completions_url_trims_trailing_slash() {
        assert_eq!(
            build_completions_url("https://example.test/"),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn explain_payload_includes_sql_and_dialect_hint() {
        let payload = build_explain_request(
            "gpt-x",
            &ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: Some("postgres".into()),
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        assert_eq!(body["model"], "gpt-x");
        // No max_tokens is sent — the API's default bounds the reply
        // (ADR-0052).
        assert!(body.get("max_tokens").is_none());
        // The system prompt rides as the first message, not a separate
        // top-level `system` field.
        assert!(body.get("system").is_none());
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("SQL expert"));
        assert_eq!(body["messages"][1]["role"], "user");
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("SELECT 1"));
        assert!(content.contains("postgres"));
    }

    #[test]
    fn explain_payload_omits_dialect_when_none() {
        let payload = build_explain_request(
            "gpt-x",
            &ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: None,
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("SELECT 1"));
        assert!(!content.to_lowercase().contains("postgres"));
    }

    #[test]
    fn non_streaming_payload_omits_stream_flag_and_options() {
        let payload = build_explain_request(
            "gpt-x",
            &ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: None,
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        // Skipped when false / None so the non-streaming body stays lean.
        assert!(body.get("stream").is_none());
        assert!(body.get("stream_options").is_none());
    }

    #[test]
    fn suggest_payload_lists_qualified_tables_and_carries_prompt() {
        let payload = build_suggest_request(
            "gpt-x",
            &SuggestRequest {
                prompt: "active users this week".into(),
                dialect: Some("postgres".into()),
                schema: vec![
                    TableInfo::qualified("public", "users"),
                    TableInfo::unqualified("orders"),
                ],
                full_schema: None,
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("public.users"));
        assert!(content.contains("orders"));
        assert!(content.contains("postgres"));
        assert!(content.contains("active users this week"));
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("SQL expert"));
    }

    fn users_table_schema() -> dbboard_ai::TableSchema {
        dbboard_ai::TableSchema {
            table: TableInfo::qualified("public", "users"),
            columns: vec![
                column(
                    "id",
                    Some("integer"),
                    false,
                    true,
                    1,
                    Some("nextval('users_id_seq'::regclass)"),
                ),
                column("email", Some("text"), false, false, 2, None),
                column("note", None, true, false, 3, None),
            ],
            primary_key: vec!["id".into(), "email".into()],
        }
    }

    fn column(
        name: &str,
        declared: Option<&str>,
        nullable: bool,
        primary_key: bool,
        ordinal: u32,
        default_value: Option<&str>,
    ) -> dbboard_ai::ColumnInfo {
        dbboard_ai::ColumnInfo {
            name: name.into(),
            declared_type: declared.map(Into::into),
            nullable,
            primary_key,
            ordinal,
            default_value: default_value.map(Into::into),
        }
    }

    #[test]
    fn suggest_payload_prefers_full_schema_create_table_rendering() {
        // ADR-0028 Decision 8: when `full_schema` is non-empty the
        // provider renders the compact CREATE TABLE-ish form and skips
        // the names-only bullet list entirely.
        let payload = build_suggest_request(
            "gpt-x",
            &SuggestRequest {
                prompt: "active users this week".into(),
                dialect: Some("postgres".into()),
                schema: vec![TableInfo::qualified("public", "users")],
                full_schema: Some(vec![users_table_schema()]),
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("CREATE TABLE public.users ("));
        assert!(content.contains("id integer NOT NULL DEFAULT nextval('users_id_seq'::regclass)"));
        assert!(content.contains("email text NOT NULL"));
        // A column without a declared type renders as the bare name
        // (nullable, no NOT NULL marker).
        assert!(content.contains("\n  note,") || content.contains("\n  note\n"));
        assert!(content.contains("PRIMARY KEY (id, email)"));
        // The names-only bullet must not appear alongside the DDL form.
        assert!(!content.contains("- public.users"));
        assert!(content.contains("active users this week"));
    }

    #[test]
    fn suggest_payload_with_empty_full_schema_falls_back_to_names() {
        // `Some(vec![])` = the prefetch ran but produced nothing usable
        // (e.g. every describe failed); the provider falls back to the
        // names-only list rather than sending an empty Tables block.
        let payload = build_suggest_request(
            "gpt-x",
            &SuggestRequest {
                prompt: "anything".into(),
                dialect: None,
                schema: vec![TableInfo::qualified("public", "users")],
                full_schema: Some(Vec::new()),
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("- public.users"));
        assert!(!content.contains("CREATE TABLE"));
    }

    #[test]
    fn render_table_schema_omits_primary_key_line_when_table_has_none() {
        let mut out = String::new();
        super::render_table_schema(
            &mut out,
            &dbboard_ai::TableSchema {
                table: TableInfo::unqualified("audit_log"),
                columns: vec![column("entry", Some("TEXT"), true, false, 1, None)],
                primary_key: Vec::new(),
            },
        );
        assert_eq!(out, "CREATE TABLE audit_log (\n  entry TEXT\n);\n");
    }

    #[test]
    fn suggest_payload_with_empty_schema_states_no_tables() {
        let payload = build_suggest_request(
            "gpt-x",
            &SuggestRequest {
                prompt: "anything".into(),
                dialect: None,
                schema: Vec::new(),
                full_schema: None,
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.contains("no tables"));
        assert!(content.contains("anything"));
    }

    #[test]
    fn parsed_to_response_concatenates_choice_text_and_reads_usage() {
        let raw = json!({
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": "Hello, world."},
                 "finish_reason": "stop"}
            ],
            "usage": {"prompt_tokens": 11, "completion_tokens": 22, "total_tokens": 33}
        });
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "gpt-x").unwrap();
        assert_eq!(response.text, "Hello, world.");
        assert_eq!(response.tokens_in, 11);
        assert_eq!(response.tokens_out, 22);
    }

    #[test]
    fn parsed_to_response_stamps_provider_and_model_identity() {
        // ADR-0027 Decision 4: `AiResponse` carries provider/model so a
        // caller holding only the response (not the trait object) can
        // still stamp history without a second `identity()` call.
        let raw = json!({
            "choices": [{"index": 0, "message": {"content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 0, "completion_tokens": 0}
        });
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "gpt-x").unwrap();
        assert_eq!(response.provider, PROVIDER_ID);
        assert_eq!(response.model, "gpt-x");
    }

    #[test]
    fn parsed_to_response_tolerates_null_content_choice() {
        // A tool-call-only choice reports `content: null`; Stage 1 skips
        // it. A following text choice still produces the response.
        let raw = json!({
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": null},
                 "finish_reason": "tool_calls"},
                {"index": 1, "message": {"role": "assistant", "content": "real text"},
                 "finish_reason": "stop"}
            ],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "gpt-x").unwrap();
        assert_eq!(response.text, "real text");
    }

    #[test]
    fn parsed_to_response_with_no_text_is_provider_error() {
        let raw = json!({
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": null},
                 "finish_reason": "tool_calls"}
            ],
            "usage": {"prompt_tokens": 1, "completion_tokens": 0}
        });
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let err = parsed_to_response(parsed, "gpt-x").unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    #[test]
    fn parsed_to_response_with_empty_choices_is_provider_error() {
        let raw = json!({ "choices": [], "usage": {"prompt_tokens": 0, "completion_tokens": 0} });
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let err = parsed_to_response(parsed, "gpt-x").unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    #[test]
    fn parsed_to_response_defaults_usage_when_missing() {
        let raw = json!({"choices": [{"message": {"content": "no usage"}}]});
        let parsed: ChatResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "gpt-x").unwrap();
        assert_eq!(response.tokens_in, 0);
        assert_eq!(response.tokens_out, 0);
    }

    #[test]
    fn error_envelope_becomes_provider_error_with_kind_and_message() {
        let body = br#"{"error":{"type":"insufficient_quota","message":"You exceeded your current quota","code":"insufficient_quota"}}"#;
        let err = error_from_status_and_body(429, body);
        match err {
            AiError::Provider(msg) => {
                assert!(msg.contains("429"));
                assert!(msg.contains("insufficient_quota"));
                assert!(msg.contains("exceeded your current quota"));
            }
            other => panic!("expected Provider, got {other:?}"),
        }
    }

    #[test]
    fn error_with_non_json_body_falls_back_to_truncated_text() {
        let body = b"<html>Cloudflare 502</html>";
        let err = error_from_status_and_body(502, body);
        match err {
            AiError::Provider(msg) => {
                assert!(msg.contains("502"));
                assert!(msg.contains("Cloudflare"));
            }
            other => panic!("expected Provider, got {other:?}"),
        }
    }

    #[test]
    fn truncate_caps_long_text_on_a_char_boundary() {
        // Each Japanese char is 3 bytes → 6000 bytes raw, well past the
        // cap. The truncation must end on a char boundary, ellipsis
        // appended.
        let long = "あ".repeat(2000);
        let truncated = truncate_to_owned(&long);
        assert!(truncated.ends_with('…'));
        let prefix = truncated.trim_end_matches('…');
        assert!(prefix.len() <= MAX_ERROR_DETAIL);
        assert!(prefix.is_char_boundary(prefix.len()));
    }

    #[test]
    fn truncate_passes_short_text_through_unchanged() {
        let short = "hello";
        assert_eq!(truncate_to_owned(short), "hello");
    }

    #[test]
    fn qualify_uses_schema_when_present_and_bare_name_otherwise() {
        assert_eq!(
            qualify(&TableInfo::qualified("public", "users")),
            "public.users"
        );
        assert_eq!(qualify(&TableInfo::unqualified("orders")), "orders");
    }

    #[test]
    fn debug_redacts_the_api_key() {
        let provider = OpenAiProvider::with_default_model("sk-secret-12345").expect("construct");
        let debug = format!("{provider:?}");
        assert!(
            !debug.contains("sk-secret-12345"),
            "api key leaked into Debug: {debug}"
        );
        assert!(debug.contains("redacted"));
        assert!(debug.contains(DEFAULT_MODEL));
    }

    #[test]
    fn is_localhost_detects_loopback_forms_only() {
        assert!(is_localhost("http://127.0.0.1:8080"));
        assert!(is_localhost("http://localhost:9000"));
        assert!(is_localhost("http://[::1]:1234"));
        assert!(!is_localhost("http://example.com"));
        assert!(!is_localhost("https://api.openai.com"));
        assert!(!is_localhost("http://192.0.2.10"));
    }
}
