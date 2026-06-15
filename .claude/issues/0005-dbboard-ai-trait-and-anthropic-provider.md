# 0005: `dbboard-ai` trait + Anthropic provider (Phase 4 Stage 1)

- **Status**: open
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

- [ ] New `crates/dbboard-anthropic` workspace member, depending
      on `dbboard-ai` + `reqwest` (`tls-rustls-ring`) + `tokio` +
      `serde_json`.
- [ ] `AnthropicProvider` struct holding `reqwest::Client`,
      `api_key: String`, `model: String`. Constructor
      `AnthropicProvider::new(api_key, model)` plus
      `AnthropicProvider::with_default_model(api_key)` defaulting
      to `claude-sonnet-4-6`.
- [ ] `id()` returns `"anthropic"`. `capabilities()` returns
      defaults (all-false for Stage 1).
- [ ] `explain` and `suggest_sql` build the system prompt + user
      message, POST to
      `https://api.anthropic.com/v1/messages`, parse the response
      envelope, surface `AiResponse` (or appropriate `AiError`
      variant).
- [ ] Error classification: HTTP 4xx with `rate_limit` /
      `overloaded_error` → `AiError::Provider`. HTTP 5xx →
      `AiError::Provider`. Network errors (timeout, TLS) →
      `AiError::Network`. Malformed response → `AiError::Provider`.
      Missing / invalid API key → `AiError::Configuration` at
      request time (Stage 1 trusts construction-time validation).
- [ ] Unit tests with `mockito` (or hand-rolled `httptest`) for
      success, rate-limit, 5xx, malformed response, timeout.
      No live-network tests in Stage 1 (gated live test deferred
      to a follow-up issue).
- [ ] `Debug` impl redacts the API key.

### `apps/dbboard` wiring

- [ ] `DBBOARD_ANTHROPIC_API_KEY` env var resolution at startup
      → construct `AnthropicProvider`. `DBBOARD_ANTHROPIC_MODEL`
      optional override.
- [ ] `DbboardApp::new` (or equivalent) takes
      `Option<Arc<dyn AiProvider>>`. None when env var absent or
      construction fails (logged but not fatal — desktop still
      runs without AI).
- [ ] README documents both env vars in the existing "Run"
      section. New "AI integration (optional)" subsection.

### `dbboard-ui`

- [ ] AI panel as an `egui::Window` toggled from the menu bar,
      registered only when `has_ai_provider()` is true.
- [ ] Two-mode UI: "Explain" (textarea for SQL → response box) and
      "Suggest" (textarea for prompt → response box). Active
      adapter id passed in as the `dialect` hint.
- [ ] Worker-side: `Command::AiExplain { sql }` /
      `Command::AiSuggest { prompt, schema }` /
      `Reply::AiResponded { text, tokens_in, tokens_out }` /
      `Reply::AiFailed { err }`. AI worker uses the same
      `tokio::runtime::Handle::block_on` pattern as
      `ConnectionSwitcher`.
- [ ] New Fluent keys (panel title, mode labels, send button,
      error categories) translated for all 11 ADR-0015 locales.
      Tier 1 + Tier 2 stay in sync (ADR-0022 Consequences rule).
- [ ] Unit tests on the UI state machine (mode switch, send while
      busy is a noop, response replaces stale content, error
      replaces stale content).

### Documentation

- [ ] `docs/architecture.md` gains an `AI layer` row in the crate
      table, with the dependency rule (`dbboard-anthropic` →
      `dbboard-ai`; `dbboard-ai` → `dbboard-core` for `TableInfo`
      only).
- [ ] `docs/roadmap.md` Phase 4 bullets tick as scope lands.
      Trait + first provider bullet refs ADR-0023.
- [ ] `README.md` mentions the optional AI panel in the run
      section, alongside the Connections and Language paragraphs
      (parallels ADR-0020 / ADR-0022).
- [ ] `crates/dbboard-anthropic/README.md` documents env vars,
      model override, the deferral list (no streaming, no
      keychain, no history).

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
