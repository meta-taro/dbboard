# 0009: AI streaming + cooperative cancel + token meter (Phase 4 Stage 2 Group B)

- **Status**: open 2026-06-29
- **Phase**: 4 Stage 2 Group B (Streaming + Cancel + Token meter).
  Phase 4 Stage 2 Group A (issue 0008) is closed.
- **Opened**: 2026-06-29
- **Tracks**: ADR-0026
- **Depends on**: ADR-0023 (the `AiProvider` trait surface this
  slice extends — `AiCapabilities::has_streaming` and
  `AiError::Cancelled` were already reserved Stage 1),
  ADR-0025 (the slot-based atomic swap pattern carries over for
  the streaming dispatch arm), the worker channel structure from
  PR #27 / PR #39 / PR #43.
- **Does NOT depend on**: Group C or Group D. Group C
  (`history.jsonl` AI records, v:2 schema bump, the one cross-repo
  coordination point that needs a fresh `0NNN-web-*` brief) and
  Group D (full DDL extraction + function-calling) are independent
  of Group B and can land in any order after this one.

## Context

ADR-0026 records the design. This issue is the implementation
plan; nothing here should re-litigate a Decision recorded in
ADR-0026. Surprises that surface during implementation become a
follow-up ADR.

Three friction sources motivate this slice (full discussion in
ADR-0026 §Context):

1. No incremental feedback during long generations (Sonnet 4.6
   explanations of non-trivial SQL take 8–30 seconds).
2. No way to abort an in-flight request — token spend is committed
   once the request fires.
3. No visibility into token spend even though `AiResponse` carries
   `tokens_in` / `tokens_out` since PR #22.

Three pieces of infrastructure are already reserved in Stage 1 and
get activated by this slice rather than introducing parallel
machinery:

- `AiCapabilities::has_streaming` (ADR-0023).
- `AiError::Cancelled` (ADR-0023).
- `AiResponse.tokens_in / tokens_out` (ADR-0023).

## Acceptance

### `dbboard-ai` (trait extension + value types)

- [ ] New public type alias `AiStream =
      futures::stream::BoxStream<'static, AiResult<StreamEvent>>`.
- [ ] New `pub enum StreamEvent` with five variants per ADR-0026
      Decision 3 (`MessageStart`, `TextDelta`, `Usage`,
      `MessageStop`, `Error`).
- [ ] New `pub enum StopReason` with five named variants
      (`EndTurn`, `MaxTokens`, `StopSequence`, `ToolUse`,
      `Refusal`) plus `Other(String)` for forward-compat.
- [ ] Two new `AiProvider` trait methods: `async fn stream_explain`
      and `async fn stream_suggest_sql`, both returning
      `AiResult<AiStream>`.
- [ ] **Default implementations** on both methods that delegate to
      the existing atomic methods and yield a single
      `TextDelta` + `Usage` + `MessageStop` sequence. Tested in a
      `#[cfg(test)] mod tests` with a `MockNoStreamProvider` that
      only implements `explain` / `suggest_sql`.
- [ ] No new runtime dep on the trait crate. `futures` already
      pulled by `tokio` in workspace; the `BoxStream` is in
      `futures::stream`.

### `dbboard-anthropic` (SSE provider impl)

- [ ] New direct dep `reqwest-eventsource` (latest stable, MIT or
      Apache-2.0). `cargo deny check` clean.
- [ ] New module `crates/dbboard-anthropic/src/stream.rs` (small —
      maps Anthropic SSE event types to normalized `StreamEvent`).
- [ ] Override of `stream_explain` and `stream_suggest_sql` that
      POST to `/v1/messages?stream=true` (or with `"stream": true`
      in the JSON body — whichever the API uses) and produce a
      `BoxStream<StreamEvent>` via `EventSource` →
      `eventsource_stream::Event` → `serde_json::from_str` of the
      `data:` payload → normalized event.
- [ ] `RetryPolicy::Never` set explicitly (token-billed POSTs must
      not silently retry — see ADR-0026 Decision 4).
- [ ] `ping` events tolerated (filtered, never surfaced to UI).
- [ ] `error` events mapped to `StreamEvent::Error(AiError::Provider(...))`
      with the body's `error.type` + `error.message` formatted into
      the message (mirroring the atomic-path error formatting from
      PR #22).
- [ ] Non-text content-block deltas (`input_json_delta`,
      `thinking_delta`, `signature_delta`) **dropped at the
      provider layer** for Group B per ADR-0026 Decision 3. Group D
      revisits.
- [ ] `usage` field on `message_delta` parsed as **cumulative**
      values (replace, do not sum — ADR-0026 Decision 7).
- [ ] `capabilities()` now returns `AiCapabilities { has_streaming:
      true, has_function_calling: false }`.
- [ ] Wiremock tests for: (a) happy-path stream (3 deltas → stop),
      (b) `ping` interleaved with deltas, (c) mid-stream `error`
      event, (d) 5xx before `message_start`, (e) cancel-by-drop
      closes the connection (assert wiremock sees the disconnect).
- [ ] All 24 + 7 existing atomic-path tests still pass unchanged.

### `dbboard-ui` (worker + AiPanel)

- [ ] New `Command` variants per ADR-0026 Decision 6:
      `AiExplainStream`, `AiSuggestStream`, `CancelAiRequest`.
- [ ] New `Reply` variants per ADR-0026 Decision 6: `AiChunk`,
      `AiStreamComplete`, `AiCancelled`.
- [ ] Worker dispatch arm for `AiExplainStream` /
      `AiSuggestStream`: spawn the stream future, own a
      per-request `tokio_util::sync::CancellationToken`, race the
      stream against the token via `tokio::select!`. On cancel:
      drop the stream, emit `Reply::AiCancelled`. On error: drop
      the stream, emit `Reply::AiFailed { error }`. On stop: emit
      `Reply::AiStreamComplete { ... }`.
- [ ] Worker dispatch arm for `CancelAiRequest`: signals the
      stored `CancellationToken`. Atomic-path requests are also
      cancellable (Decision 10) — the worker holds the
      `JoinHandle` and aborts it on cancel.
- [ ] `AiPanel` gains state per ADR-0026 Decision 9:
      `streaming_enabled: bool` (toggle, default `false`),
      `accumulated_text: String` (in-flight buffer),
      `last_tokens_in: Option<u32>`, `last_tokens_out: Option<u32>`
      (cumulative — replace, do not sum, per Decision 7).
- [ ] `AiPanel` UI: streaming-mode checkbox gated on
      `provider.capabilities().has_streaming`. Cancel button
      enabled whenever `busy == true` (atomic or streaming).
      Token meter sublabel showing `"{in} in / {out} out"` when
      `last_tokens_*` is `Some`. Incremental text render in the
      response area while streaming.
- [ ] 3 new Fluent keys × 11 locales per ADR-0022 same-commit
      sync: `ai-cancel-button`, `ai-stream-toggle`,
      `ai-tokens-meter` (the latter takes `{ $in }` and
      `{ $out }` arguments). Tier 1 (en/de/fr/es/pt-BR/it/zh-CN)
      + Tier 2 (ja/ko/zh-TW/ru) translated in the same commit.
- [ ] `AiPanel` state-machine tests for: (a) toggle stream mode
      flips command variant, (b) cancel mid-stream emits
      `AiCancelled`, (c) chunks accumulate into the buffer, (d)
      `AiStreamComplete` clears `busy` and freezes the final
      meter values, (e) `AiFailed` mid-stream transitions to
      error display, (f) `provider.capabilities().has_streaming
      == false` hides the toggle.

### `apps/dbboard` (binary wiring)

- [ ] No change to `DbboardApp::connect` signature. Streaming
      flows through the existing `Arc<dyn AiProvider>` because
      the trait now carries the new methods.
- [ ] `resolve_ai_provider_from` and `DesktopAiSwitcher`
      unchanged.
- [ ] Existing `bootstrap_ai` return tuple unchanged.

### Docs

- [ ] `README.md` AI integration section gains a paragraph on
      streaming mode toggle + cancel button + token meter (kept
      under 8 lines — the README is already long).
- [ ] `docs/roadmap.md` Phase 4 row ticked for Group B with the
      `[x]` checkbox and the PR number reference.
- [ ] `docs/decisions.md` ADR-0026 status updated from `Proposed`
      to `Accepted` with the landing date and PR number on the
      last commit of the slice.
- [ ] `.claude/project-status.md` records the slice landing.
- [ ] `.claude/next-actions.md` rewritten to reflect the menu
      after Group B closure (selections become Group C / Group D
      / friction reports / web work).

### Verification

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
      clean.
- [ ] `cargo check --all-targets --all-features` clean.
- [ ] `cargo test --all-features` clean. Test count grows from
      474 to approximately 490+ (10 new `dbboard-ai` default-impl
      + `StreamEvent` tests, 5 new `dbboard-anthropic` wiremock,
      6 new `dbboard-ui` panel/worker dispatch tests).
- [ ] `cargo build --release` and `cargo test --all-features
      --release` clean (pre-push hook).
- [ ] No `unsafe` code introduced. Workspace `unsafe_code =
      "forbid"` upheld.

## Implementation slicing (suggested)

The four-slice cut is a **suggestion**, not a mandate. Whether
this lands as one PR or four is a slicing question — ADR-0026
does not prescribe.

- **Slice a**: `dbboard-ai` trait extension + `StreamEvent` /
  `StopReason` types + default delegate impls + tests.
- **Slice b**: `dbboard-anthropic` SSE wiring + `has_streaming
  = true` + wiremock tests.
- **Slice c**: `dbboard-ui` worker + `AiPanel` + Fluent keys ×
  11 locales + state-machine tests.
- **Slice d**: docs sweep (README, roadmap, project-status, this
  issue ticked closed, ADR status flipped to Accepted).

## Out of scope (deferred to other groups)

- AI calls recorded in `history.jsonl` — **Group C** (needs a
  fresh `0NNN-web-*` brief because it forces a v:2 schema bump).
- Streaming the `input_json_delta` / `signature_delta` content
  blocks — **Group D** (function-calling).
- Full DDL extraction into `SuggestRequest.schema` — **Group D**.
- Conversation history (multi-turn AI) — separate future ADR.
- Token budget meter that enforces a ceiling — ADR-0026 ships
  the **display**, ADR-0023 §9 `Quota` variant remains
  unenforced and deferred.
