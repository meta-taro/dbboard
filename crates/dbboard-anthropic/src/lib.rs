//! Anthropic Messages API provider for the `dbboard-ai` trait
//! (ADR-0023).
//!
//! Stage 1 surface: `explain` and `suggest_sql`, both routed through
//! `POST /v1/messages`. No streaming, no function calling — those are
//! Stage 2 concerns and reserved as `AiCapabilities` flags only.
//!
//! Dependency rule (ADR-0023 Decision 1): this crate depends on
//! `dbboard-ai` plus `reqwest` / `serde` / `serde_json`. It never
//! depends on `dbboard-core` directly; `TableInfo` is consumed through
//! `dbboard-ai`'s re-export so swapping the trait crate cannot drag in
//! the domain crate behind the scenes.

use std::fmt::Write as _;

use async_trait::async_trait;
use dbboard_ai::{
    AiCapabilities, AiError, AiProvider, AiResponse, AiResult, AiStream, ExplainRequest,
    SuggestRequest, TableInfo, TableSchema,
};
use serde::{Deserialize, Serialize};

mod stream;

const PROVIDER_ID: &str = "anthropic";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 1024;

/// Cap on error/body text surfaced into an [`AiError`], so a hostile or
/// runaway response cannot dump an unbounded string into the UI.
const MAX_ERROR_DETAIL: usize = 2048;

const EXPLAIN_SYSTEM_PROMPT: &str = "You are a SQL expert. Explain the given SQL statement \
    in plain English. Be concise (1-3 sentences) and do not restate the SQL verbatim.";

const SUGGEST_SYSTEM_PROMPT: &str =
    "You are a SQL expert. Generate a single SQL statement that answers the user's request, \
    using only the tables listed. Reply with the SQL only — no commentary, no markdown fences.";

/// Configuration for [`AnthropicProvider`]. `base_url` is `None` in
/// production; tests override it to point at a wiremock server.
#[derive(Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

/// Anthropic Messages API client implementing [`AiProvider`].
///
/// The API key is held privately and never appears in `Debug` output,
/// log lines, or error messages.
pub struct AnthropicProvider {
    client: reqwest::Client,
    messages_url: String,
    model: String,
    // Kept private and never surfaced in Debug or errors.
    api_key: String,
}

impl AnthropicProvider {
    /// Build a provider with an explicit model id against the
    /// production Anthropic endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`AiError::Configuration`] when `api_key` or `model` is
    /// empty (or whitespace only), or when the HTTP client fails to
    /// initialise (e.g. the TLS backend cannot start).
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> AiResult<Self> {
        Self::with_config(AnthropicConfig {
            api_key: api_key.into(),
            model: model.into(),
            base_url: None,
        })
    }

    /// Build a provider against the [`DEFAULT_MODEL`] constant
    /// (`claude-sonnet-4-6`, per `rules/performance.md`'s
    /// "best coding model" pick).
    ///
    /// # Errors
    ///
    /// Same as [`Self::new`].
    pub fn with_default_model(api_key: impl Into<String>) -> AiResult<Self> {
        Self::new(api_key, DEFAULT_MODEL)
    }

    /// Build a provider from a full [`AnthropicConfig`].
    ///
    /// # Errors
    ///
    /// See [`Self::new`].
    pub fn with_config(config: AnthropicConfig) -> AiResult<Self> {
        if config.api_key.trim().is_empty() {
            return Err(AiError::Configuration("anthropic api key is empty".into()));
        }
        if config.model.trim().is_empty() {
            return Err(AiError::Configuration("anthropic model id is empty".into()));
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
        let messages_url = build_messages_url(base);
        Ok(Self {
            client,
            messages_url,
            model: config.model,
            api_key: config.api_key,
        })
    }

    async fn call_messages(&self, body: MessagesRequest) -> AiResult<AiResponse> {
        let response = self
            .client
            .post(&self.messages_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(transport_error)?;

        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(transport_error)?;

        if (200..300).contains(&status) {
            let parsed: MessagesResponse = serde_json::from_slice(&bytes).map_err(|e| {
                AiError::Provider(format!(
                    "malformed anthropic response (status {status}): {e} [{}]",
                    truncate_to_owned(&String::from_utf8_lossy(&bytes))
                ))
            })?;
            parsed_to_response(parsed, &self.model)
        } else {
            Err(error_from_status_and_body(status, &bytes))
        }
    }

    /// Open an SSE stream against `POST /v1/messages` with
    /// `"stream": true`. Per ADR-0026 Decision 4 the request uses
    /// [`reqwest_eventsource`] with `RetryPolicy::Never` so a
    /// token-billed POST is never silently retried.
    ///
    /// The pre-stream error path returns `Err(AiError)` on the outer
    /// future (config / transport setup); wire-level errors after the
    /// connection opens surface as `Ok(StreamEvent::Error)` chunks
    /// inside the stream.
    fn open_stream(&self, body: &MessagesRequest) -> AiResult<AiStream> {
        use reqwest_eventsource::{retry, RequestBuilderExt};

        let request = self
            .client
            .post(&self.messages_url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(body);

        let mut es = request.eventsource().map_err(|e| {
            AiError::Configuration(format!("anthropic streaming: cannot prepare request: {e}"))
        })?;
        // Never retry a token-billed POST — CLAUDE.md / ADR-0026
        // Decision 4. The default policy is exponential back-off,
        // which would silently double-charge on transient 5xx.
        es.set_retry_policy(Box::new(retry::Never));

        Ok(stream::anthropic_stream(es))
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> AiCapabilities {
        // ADR-0026 Slice b flips `has_streaming` on — the trait's
        // streaming methods are now backed by a real SSE provider.
        // `has_function_calling` stays off; that's Group C.
        AiCapabilities {
            has_streaming: true,
            has_function_calling: false,
        }
    }

    // ADR-0027 Decision 4: `(provider_id, model_id)` for history
    // stamping. `PROVIDER_ID` is a compile-time constant; `model` is a
    // per-instance `String` (constructor-time), so we hand back a
    // borrow into the provider. The `dbboard-ui` worker snapshots this
    // tuple once at task spawn time (see `worker::spawn_ai_task`) and
    // reuses the snapshot for every terminal reply — a mid-request
    // provider swap changes the *next* request's identity, never the
    // one already in flight.
    fn identity(&self) -> (&'static str, &str) {
        (PROVIDER_ID, &self.model)
    }

    async fn explain(&self, req: &ExplainRequest) -> AiResult<AiResponse> {
        self.call_messages(build_explain_request(&self.model, req))
            .await
    }

    async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse> {
        self.call_messages(build_suggest_request(&self.model, req))
            .await
    }

    async fn stream_explain(&self, req: &ExplainRequest) -> AiResult<AiStream> {
        let mut body = build_explain_request(&self.model, req);
        body.stream = true;
        self.open_stream(&body)
    }

    async fn stream_suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiStream> {
        let mut body = build_suggest_request(&self.model, req);
        body.stream = true;
        self.open_stream(&body)
    }
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `finish_non_exhaustive` lets clippy's
        // `missing_fields_in_debug` lint pass without forcing us to
        // surface the `reqwest::Client` (whose own Debug carries no
        // useful information for this struct).
        f.debug_struct("AnthropicProvider")
            .field("messages_url", &self.messages_url)
            .field("model", &self.model)
            // Never expose the API key, even if a future change starts
            // logging this struct or wiring it into an error envelope.
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

// --- wire types -----------------------------------------------------------

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<RequestMessage>,
    /// `true` when the request opens an SSE stream
    /// (`Accept: text/event-stream` is added by `reqwest-eventsource`
    /// automatically; the API also requires the body flag). Skipped
    /// from serialization when `false` so non-streaming requests
    /// produce the same body bytes as before (preserving the existing
    /// round-trip tests).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Debug, Serialize)]
struct RequestMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<ResponseBlock>,
    #[serde(default)]
    usage: Usage,
}

// `#[serde(other)]` catches any future block type (tool_use, image, …)
// without a hard parse failure; Stage 1 only consumes text.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
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

fn build_messages_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/v1/messages")
}

fn build_explain_request(model: &str, req: &ExplainRequest) -> MessagesRequest {
    let user_text = match req.dialect.as_deref() {
        Some(dialect) => format!("SQL ({dialect}):\n{}", req.sql),
        None => format!("SQL:\n{}", req.sql),
    };
    MessagesRequest {
        model: model.to_string(),
        max_tokens: DEFAULT_MAX_TOKENS,
        system: Some(EXPLAIN_SYSTEM_PROMPT.into()),
        messages: vec![RequestMessage {
            role: "user",
            content: user_text,
        }],
        stream: false,
    }
}

fn build_suggest_request(model: &str, req: &SuggestRequest) -> MessagesRequest {
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

    MessagesRequest {
        model: model.to_string(),
        max_tokens: DEFAULT_MAX_TOKENS,
        system: Some(SUGGEST_SYSTEM_PROMPT.into()),
        messages: vec![RequestMessage {
            role: "user",
            content: user_text,
        }],
        stream: false,
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

fn parsed_to_response(parsed: MessagesResponse, model: &str) -> AiResult<AiResponse> {
    let mut combined = String::new();
    for block in parsed.content {
        if let ResponseBlock::Text { text } = block {
            combined.push_str(&text);
        }
    }
    if combined.is_empty() {
        return Err(AiError::Provider(
            "anthropic returned no text content in response".into(),
        ));
    }
    Ok(AiResponse {
        text: combined,
        tokens_in: parsed.usage.input_tokens,
        tokens_out: parsed.usage.output_tokens,
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
        "anthropic api error (status {status}): {}",
        body_error_detail(body)
    ))
}

/// Extract a human-readable reason from an API error response body,
/// preferring the structured `{ "error": { … } }` envelope and falling
/// back to the raw (truncated) text when the body is not JSON. Shared by
/// the atomic and streaming error paths so both surface the same detail
/// (e.g. "credit balance too low") instead of a bare status code.
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
        AiError::Network(format!("anthropic request timed out: {scrubbed}"))
    } else {
        AiError::Network(format!("anthropic transport error: {scrubbed}"))
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
        build_explain_request, build_messages_url, build_suggest_request,
        error_from_status_and_body, is_localhost, parsed_to_response, qualify, truncate_to_owned,
        AnthropicProvider, MessagesResponse, DEFAULT_BASE_URL, DEFAULT_MODEL, MAX_ERROR_DETAIL,
        PROVIDER_ID,
    };
    use dbboard_ai::{AiError, AiProvider, ExplainRequest, SuggestRequest, TableInfo};
    use serde_json::json;

    #[test]
    fn provider_id_constant_is_anthropic() {
        assert_eq!(PROVIDER_ID, "anthropic");
    }

    #[test]
    fn default_model_is_claude_sonnet_4_6() {
        assert_eq!(DEFAULT_MODEL, "claude-sonnet-4-6");
    }

    #[test]
    fn with_default_model_constructs_against_the_default_model() {
        let provider = AnthropicProvider::with_default_model("test-key").expect("construct");
        assert_eq!(provider.model, DEFAULT_MODEL);
        assert_eq!(provider.id(), "anthropic");
        // ADR-0026 Slice b: `has_streaming` is now `true`. The trait
        // continues to advertise `has_function_calling: false`
        // (Group C).
        let caps = provider.capabilities();
        assert!(caps.has_streaming);
        assert!(!caps.has_function_calling);
    }

    #[test]
    fn identity_returns_provider_id_and_configured_model() {
        // ADR-0027 Decision 4: `identity()` is the source of truth
        // the `dbboard-ui` worker snapshots at task-spawn time and
        // stamps on every terminal reply. Anthropic must return its
        // stable id and the model configured at construction — a
        // custom model surfaces here without re-plumbing.
        let provider = AnthropicProvider::new("test-key", "claude-x").expect("construct");
        let (p, m) = provider.identity();
        assert_eq!(p, PROVIDER_ID);
        assert_eq!(m, "claude-x");

        let default_provider =
            AnthropicProvider::with_default_model("test-key").expect("construct");
        let (p2, m2) = default_provider.identity();
        assert_eq!(p2, PROVIDER_ID);
        assert_eq!(m2, DEFAULT_MODEL);
    }

    #[test]
    fn new_with_empty_api_key_is_configuration_error() {
        let err = AnthropicProvider::new("", "claude-x").unwrap_err();
        assert!(matches!(err, AiError::Configuration(_)));
    }

    #[test]
    fn new_with_whitespace_api_key_is_configuration_error() {
        let err = AnthropicProvider::new("   ", "claude-x").unwrap_err();
        assert!(matches!(err, AiError::Configuration(_)));
    }

    #[test]
    fn new_with_empty_model_is_configuration_error() {
        let err = AnthropicProvider::new("test-key", "").unwrap_err();
        assert!(matches!(err, AiError::Configuration(_)));
    }

    #[test]
    fn build_messages_url_uses_default_base() {
        assert_eq!(
            build_messages_url(DEFAULT_BASE_URL),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn build_messages_url_trims_trailing_slash() {
        assert_eq!(
            build_messages_url("https://example.test/"),
            "https://example.test/v1/messages"
        );
    }

    #[test]
    fn explain_payload_includes_sql_and_dialect_hint() {
        let payload = build_explain_request(
            "claude-x",
            &ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: Some("postgres".into()),
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        assert_eq!(body["model"], "claude-x");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["messages"][0]["role"], "user");
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("SELECT 1"));
        assert!(content.contains("postgres"));
        assert!(body["system"].as_str().unwrap().contains("SQL expert"));
    }

    #[test]
    fn explain_payload_omits_dialect_when_none() {
        let payload = build_explain_request(
            "claude-x",
            &ExplainRequest {
                sql: "SELECT 1".into(),
                dialect: None,
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("SELECT 1"));
        assert!(!content.to_lowercase().contains("postgres"));
    }

    #[test]
    fn suggest_payload_lists_qualified_tables_and_carries_prompt() {
        let payload = build_suggest_request(
            "claude-x",
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
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("public.users"));
        assert!(content.contains("orders"));
        assert!(content.contains("postgres"));
        assert!(content.contains("active users this week"));
        assert!(body["system"].as_str().unwrap().contains("SQL expert"));
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
            "claude-x",
            &SuggestRequest {
                prompt: "active users this week".into(),
                dialect: Some("postgres".into()),
                schema: vec![TableInfo::qualified("public", "users")],
                full_schema: Some(vec![users_table_schema()]),
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][0]["content"].as_str().unwrap();
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
            "claude-x",
            &SuggestRequest {
                prompt: "anything".into(),
                dialect: None,
                schema: vec![TableInfo::qualified("public", "users")],
                full_schema: Some(Vec::new()),
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][0]["content"].as_str().unwrap();
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
            "claude-x",
            &SuggestRequest {
                prompt: "anything".into(),
                dialect: None,
                schema: Vec::new(),
                full_schema: None,
            },
        );
        let body = serde_json::to_value(&payload).unwrap();
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("no tables"));
        assert!(content.contains("anything"));
    }

    #[test]
    fn parsed_to_response_concatenates_text_blocks_and_reads_usage() {
        let raw = json!({
            "content": [
                {"type": "text", "text": "Hello, "},
                {"type": "text", "text": "world."}
            ],
            "usage": {"input_tokens": 11, "output_tokens": 22}
        });
        let parsed: MessagesResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "claude-x").unwrap();
        assert_eq!(response.text, "Hello, world.");
        assert_eq!(response.tokens_in, 11);
        assert_eq!(response.tokens_out, 22);
    }

    #[test]
    fn parsed_to_response_stamps_provider_and_model_identity() {
        // ADR-0027 Decision 4: `AiResponse` gains provider/model so a
        // caller holding only the response (not the trait object) can
        // still stamp history without a second `identity()` call. This
        // is the atomic-path witness — the streaming path stamps
        // identity from the worker's spawn-time snapshot instead.
        let raw = json!({
            "content": [{"type": "text", "text": "ok"}],
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        let parsed: MessagesResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "claude-x").unwrap();
        assert_eq!(response.provider, PROVIDER_ID);
        assert_eq!(response.model, "claude-x");
    }

    #[test]
    fn parsed_to_response_keeps_text_when_unknown_block_is_present() {
        let raw = json!({
            "content": [
                {"type": "tool_use", "id": "t1", "name": "foo", "input": {}},
                {"type": "text", "text": "real text"}
            ],
            "usage": {"input_tokens": 1, "output_tokens": 1}
        });
        let parsed: MessagesResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "claude-x").unwrap();
        assert_eq!(response.text, "real text");
    }

    #[test]
    fn parsed_to_response_with_no_text_block_is_provider_error() {
        let raw = json!({
            "content": [
                {"type": "tool_use", "id": "t1", "name": "foo", "input": {}}
            ],
            "usage": {"input_tokens": 1, "output_tokens": 0}
        });
        let parsed: MessagesResponse = serde_json::from_value(raw).unwrap();
        let err = parsed_to_response(parsed, "claude-x").unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    #[test]
    fn parsed_to_response_with_empty_content_array_is_provider_error() {
        let raw = json!({ "content": [], "usage": {"input_tokens": 0, "output_tokens": 0} });
        let parsed: MessagesResponse = serde_json::from_value(raw).unwrap();
        let err = parsed_to_response(parsed, "claude-x").unwrap_err();
        assert!(matches!(err, AiError::Provider(_)));
    }

    #[test]
    fn parsed_to_response_defaults_usage_when_missing() {
        let raw = json!({"content": [{"type": "text", "text": "no usage"}]});
        let parsed: MessagesResponse = serde_json::from_value(raw).unwrap();
        let response = parsed_to_response(parsed, "claude-x").unwrap();
        assert_eq!(response.tokens_in, 0);
        assert_eq!(response.tokens_out, 0);
    }

    #[test]
    fn error_envelope_becomes_provider_error_with_kind_and_message() {
        let body = br#"{"type":"error","error":{"type":"rate_limit_error","message":"too many"}}"#;
        let err = error_from_status_and_body(429, body);
        match err {
            AiError::Provider(msg) => {
                assert!(msg.contains("429"));
                assert!(msg.contains("rate_limit_error"));
                assert!(msg.contains("too many"));
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
        // Strip the ellipsis (3 bytes) and confirm the prefix is at or
        // below the cap and on a char boundary.
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
        let provider = AnthropicProvider::with_default_model("sk-secret-12345").expect("construct");
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
        assert!(!is_localhost("https://api.anthropic.com"));
        // A plaintext non-loopback URL is still rejected by the
        // https_only guard at request time; this only checks the
        // localhost predicate itself.
        assert!(!is_localhost("http://192.0.2.10"));
    }
}
