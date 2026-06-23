# 0005: `dbboard-ai` trait + Anthropic provider (Phase 4 Stage 1)

- **Status**: closed by PR #27 (merged 2026-06-23 at desktop@c86424a) —
  all acceptance boxes ticked. Slice (a) (trait crate + Anthropic
  provider + `apps/dbboard` env-var wiring) closed previously by
  PRs #20 / #22 / #24; slice (b) (`dbboard-ui` AI panel + worker
  dispatch + 11-locale Fluent + docs sweep) closed by PR #27.
- **Phase**: 4 (AI integration, optional layer)
- **Opened**: 2026-06-12
- **Tracks**: ADR-0023
- **Depends on**: nothing — Phase 1 / 2 / 2.5 / 3 are closed and the
  AI layer is additive to all of them.

## Context

ADR-0023 (2026-06-12) opens Phase 4 by committing the trait shape
and the first concrete provider:

- `crates/dbboard-ai` — pure trait crate, no I/O. Defines
  `AiProvider`, `AiCapabilities`, `ExplainRequest`,
  `SuggestRequest`, `AiResponse`, `AiError`.
- `crates/dbboard-anthropic` — first concrete provider, talks to
  the Anthropic Messages API over `reqwest`.
- `apps/dbboard` wires `Option<Arc<dyn AiProvider>>` at startup
  based on `DBBOARD_ANTHROPIC_API_KEY` (required) and
  `DBBOARD_ANTHROPIC_MODEL` (optional, default
  `claude-sonnet-4-6`).
- `dbboard-ui` adds an AI panel that renders only when the
  provider is present (graceful degradation = absence).

Stage 2 deferrals (streaming, multi-provider switcher,
`ai-providers.toml` + keychain, AI calls in query history,
full-DDL schema snapshots, function-calling) are recorded in
ADR-0023 §9 and out of scope for this issue.

## Acceptance

### `dbboard-ai` (trait crate, no I/O)

- [x] New `crates/dbboard-ai` workspace member, depending on
      `dbboard-core` only (for `TableInfo`) plus `async_trait` +
      `serde` + `thiserror`. No `reqwest`, no `tokio`. (`tokio` is
      a dev-dependency only — required to drive `async fn` trait
      tests; it is not a runtime dep of the trait crate.)
- [x] `AiProvider` trait: `async_trait` + `Send + Sync`,
      object-safe behind `Arc<dyn AiProvider>`. Required methods:
      `id(&self) -> &'static str`, `capabilities(&self) ->
      AiCapabilities`, `async fn explain(&self, req:
      &ExplainRequest) -> AiResult<AiResponse>`,
      `async fn suggest_sql(&self, req: &SuggestRequest) ->
      AiResult<AiResponse>`.
- [x] `AiCapabilities` flat bool struct (`has_streaming`,
      `has_function_calling`), `#[derive(Copy, Debug, Default,
      Deserialize, Serialize)]`, all-false by default.
- [x] `ExplainRequest { sql: String, dialect: Option<String> }`,
      `SuggestRequest { prompt: String, dialect: Option<String>,
      schema: Vec<TableInfo> }`,
      `AiResponse { text: String, tokens_in: u32, tokens_out:
      u32 }`. `TableInfo` re-exported from `dbboard-core`.
- [x] `AiError` enum with `Configuration` / `Network` /
      `Provider` / `Quota` / `Cancelled`, `#[derive(Debug,
      thiserror::Error)]`, `AiResult<T> = Result<T, AiError>`
      type alias.
- [x] Unit tests: object-safety (`Arc<dyn AiProvider>` constructs),
      capability flag round-trip through JSON, `AiError` Display
      covers every variant. (15 tests pass on the trait crate.)

### `dbboard-anthropic` (first concrete provider)

- [x] New `crates/dbboard-anthropic` workspace member, depending
      on `dbboard-ai` + `reqwest` (workspace `rustls-tls` feature —
      issue wording said `tls-rustls-ring`, but that is the sqlx
      feature name; reqwest's rustls feature is `rustls-tls`) +
      `serde` + `serde_json`. `async-trait` for the AiProvider impl;
      `tokio` + `wiremock` as dev-deps only.
- [x] `AnthropicProvider` struct holding `reqwest::Client`,
      `api_key: String`, `model: String` (plus a cached
      `messages_url` resolved at construction). Constructors
      `AnthropicProvider::new(api_key, model)` and
      `AnthropicProvider::with_default_model(api_key)` defaulting
      to `claude-sonnet-4-6`. `AnthropicProvider::with_config` takes
      an `AnthropicConfig { api_key, model, base_url }` for test
      base-URL overrides (wiremock).
- [x] `id()` returns `"anthropic"`. `capabilities()` returns
      `AiCapabilities::default()` (all-false for Stage 1).
- [x] `explain` and `suggest_sql` build the system prompt + user
      message, POST to `{base}/v1/messages` with `x-api-key` +
      `anthropic-version: 2023-06-01` headers, concatenate text
      blocks from the response, and surface
      `AiResponse { text, tokens_in, tokens_out }`. Tool-use and
      other unknown block types deserialize cleanly via
      `#[serde(other)]` and are ignored.
- [x] Error classification: HTTP 4xx (incl. 401 auth, 429 rate-limit)
      → `AiError::Provider`. HTTP 5xx → `AiError::Provider`.
      Malformed response body → `AiError::Provider`. Timeout / TLS /
      transport → `AiError::Network` (with the URL scrubbed via
      `reqwest::Error::without_url`). Construction-time empty key
      or empty model → `AiError::Configuration`. Runtime 401 is
      intentionally NOT re-raised as Configuration — the design
      trusts the construction-time check.
- [x] Round-trip tests with `wiremock` for success (explain /
      suggest), request-body shape (model + system prompt + user
      content), rate-limit 429, server 5xx, authentication 401,
      and malformed success body. 24 unit tests + 7 integration
      tests, all green. Live-network test (gated behind
      `DBBOARD_ANTHROPIC_API_KEY`) deferred to a follow-up issue.
- [x] `Debug` impl redacts the API key (`<redacted>`) and uses
      `finish_non_exhaustive` so the `reqwest::Client` field stays
      hidden without tripping the `missing_fields_in_debug` lint.

### `apps/dbboard` wiring

- [x] `DBBOARD_ANTHROPIC_API_KEY` env var resolution at startup
      → construct `AnthropicProvider`. `DBBOARD_ANTHROPIC_MODEL`
      optional override. Implemented in `apps/dbboard::resolve_ai_provider`;
      missing key or construction error logs to stderr and returns `None`.
- [x] `DbboardApp::new` (or equivalent) takes
      `Option<Arc<dyn AiProvider>>`. None when env var absent or
      construction fails (logged but not fatal — desktop still
      runs without AI). `DbboardApp::connect` + `DbboardApp::new`
      both accept the option; the accessor `has_ai_provider()` lets the
      slice (b) UI panel gate registration.
- [x] README documents both env vars in the existing "Run"
      section. New "AI integration (optional)" subsection.

### `dbboard-ui`

- [x] AI panel as an `egui::Window` toggled from the menu bar,
      registered only when `has_ai_provider()` is true.
      (`crates/dbboard-ui/src/ai.rs`; menu button in
      `apps/dbboard/src/main.rs` gated on the same accessor.)
- [x] Two-mode UI: "Explain" (textarea for SQL → response box) and
      "Suggest" (textarea for prompt → response box).
      `dialect` hint is currently `None` from the UI side — adapter
      id resolution from the loopback server is deferred to Stage 2
      (commented in `lib.rs::ui()`); the request still carries it
      end-to-end so the provider sees the field when wiring catches up.
- [x] Worker-side: `Command::AiExplain { sql, dialect }` /
      `Command::AiSuggest { prompt, dialect, schema }` /
      `Reply::AiResponded { text, tokens_in, tokens_out }` /
      `Reply::AiFailed { error }`. AI worker uses the same
      `tokio::runtime::Handle::block_on` pattern as
      `ConnectionSwitcher`. Defence-in-depth: a `Command::Ai*` with
      `ai_provider == None` surfaces `Reply::AiFailed
      { AiError::Configuration }` so the panel's busy flag clears.
- [x] New Fluent keys (`ai-menu-button`, `ai-panel-title`,
      `ai-mode-explain` / `ai-mode-suggest`,
      `ai-input-explain` / `ai-input-suggest`, `ai-send-button`,
      `ai-busy`, `ai-empty`, `ai-error-prefix-*` for the 5 AiError
      variants) translated for all 11 ADR-0015 locales (en, ja, ko,
      zh-CN, zh-TW, de, fr, es, pt-BR, ru, it). Tier 1 + Tier 2 in
      sync (ADR-0022 Consequences rule).
- [x] Unit tests on the UI state machine: 11 tests in
      `crates/dbboard-ui/src/ai.rs` covering open/close/toggle, mode
      switch, empty/whitespace input noop, explain + dialect, suggest
      + schema, send-while-busy noop, on_response clears busy and
      records success, on_error clears busy and translates the message,
      fresh response replaces stale error and vice versa,
      `ai_error_display` variant coverage. 5 worker dispatch tests in
      `crates/dbboard-ui/src/worker.rs` covering explain success,
      suggest success, provider error, no-provider configuration
      failure, and unchanged SwitchConnection smoke.

### Documentation

- [x] `docs/architecture.md` AI layer paragraphs updated to mention
      PR #24 env-var wiring AND the slice (b) UI panel routing
      (worker block_on, `Reply::AiResponded` / `AiFailed`, hide-on-absence).
- [x] `docs/roadmap.md` Phase 4 bullets ticked for slice (b);
      Stage 1 exit criteria called out as met, Stage 2 scope refs
      ADR-0023 §9.
- [x] `README.md` AI integration subsection rewritten to describe
      the panel surface (menu entry placement, two modes, error
      rendering) on top of the existing env-var docs.
- [x] `crates/dbboard-anthropic/README.md` Configuration section
      drops the "sibling PR" wording and adds the slice (b) wiring
      (worker → provider routing, hide-on-absence reference to
      Decision 11).

## Notes

- ADR-0023 §9 captures the explicit Stage 2 deferrals — reviewers
  should not relitigate them in this PR.
- HTTP contract: untouched. `dbboard-server` and
  `docs/api-contract.md` are unchanged. The web sibling has its
  own provider story; no `0NNN-web-*` mirror brief.
- Pre-commit + pre-push hooks (cargo-husky) cover format / clippy
  `-D warnings` / check / test for both debug and release. Two
  new crates expand the workspace test run but should not slow
  any individual crate.
- This is a single PR scope. If it grows past ~10 commits, split
  by crate (`dbboard-ai` lands first, `dbboard-anthropic`
  second, then `apps/dbboard` + `dbboard-ui` wiring), each PR
  green on its own.
