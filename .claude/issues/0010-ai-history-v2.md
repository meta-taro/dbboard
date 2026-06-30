# 0010: AI calls recorded in `history.jsonl` (Phase 4 Stage 2 Group C, schema v:2)

- **Status**: open 2026-06-30
- **Phase**: 4 Stage 2 Group C. Phase 4 Stage 2 Group A (issue 0008
  closed via PR #43) and Group B (issue 0009 closed via PR #45) are
  both closed and on `develop`.
- **Opened**: 2026-06-30
- **Tracks**: ADR-0027
- **Cross-repo brief**: [0008-web-history-v2-mirror.md](0008-web-history-v2-mirror.md)
  (issued same PR as this issue)
- **Depends on**: ADR-0017 (the `history.jsonl` schema this slice
  bumps from v:1 to v:2, and the cross-repo coordination contract),
  ADR-0023 (the `AiProvider` trait surface this slice extends with
  `identity()`), ADR-0024 (at-rest permissions that cover the verbatim
  AI prompts/responses), ADR-0026 (the streaming + cancel terminal
  reply variants that carry the new provider/model/tokens fields).
- **Does NOT depend on**: Group D (DDL extraction + function-calling)
  — that is independent and lands on its own ADR.

## Context

ADR-0027 records the design. This issue is the implementation
plan; nothing here should re-litigate a Decision recorded in
ADR-0027. Surprises that surface during implementation become a
follow-up ADR.

Three observations motivate this slice (full discussion in
ADR-0027 §Context):

1. AI activity (Group A provider config + Group B streaming + token
   meter) leaves no durable trace today. A 30-second streamed
   `explain` is gone the moment the user navigates away.
2. `history.jsonl` is the right home — same `jq` UX, same ADR-0024
   at-rest hardening, same cross-repo mirror contract from brief 0003.
3. The schema bump (v:1 → v:2) is the cheapest forward-compatible
   move because ADR-0017's reader already drops unknown-version
   records and counts the skip (`history.rs:255`).

## Acceptance

### `dbboard-ui::history` (the v:2 module)

- [ ] `CURRENT_VERSION` bumped from `1` to `2`.
- [ ] `RecordWire` becomes a flat struct with optional fields and a
      `kind: "query" | "ai"` discriminator. v:1 records (no `kind`,
      `sql` present) read transparently as `kind: "query"`.
- [ ] New `HistoryEntry::Ai { … }` variant. The existing
      `HistoryEntry` (the v:1 SQL shape) becomes
      `HistoryEntry::Query { … }`. Public API renames are part of
      slice (a) and ripple into call sites in slice (c).
- [ ] Reader unit tests: v:2 query record round-trips, v:2 AI
      record round-trips, v:1 record reads as `Query`, v:2 record
      with unknown `kind` skips + counter ticks, v:2 record with
      unknown `intent` skips + counter ticks.
- [ ] Writer unit tests: query write produces v:2 + kind="query",
      AI write produces v:2 + kind="ai" with every documented
      field present (nulls preserved per ADR-0027 §Decision 4).
- [ ] Truncation: prompt and response capped at 64 KiB at write
      time with the `[truncated at 64 KiB]` marker appended
      (Decision 10). Unit test asserts the cap.
- [ ] `fixture::serialize` (the doc-hidden shim) extends to AI
      records.

### `dbboard-ai` (trait + value types)

- [ ] New `AiProvider::identity(&self) -> (&'static str, &str)`
      returning `(provider_id, model_id)`. Default impl returns
      `("unknown", "")` so existing in-tree mocks compile.
- [ ] `AiResponse` gains `provider: String, model: String` fields.
- [ ] `StreamEvent::MessageStop` already carries `stop_reason`;
      the worker copies it through to the new `Reply` fields.
- [ ] No new dependency.

### `dbboard-anthropic`

- [ ] `AnthropicProvider::identity()` returns
      `("anthropic", &self.model)`.
- [ ] `AiResponse` construction populates provider/model.
- [ ] All existing tests pass unchanged (the new fields are
      populated; assertions ignore them).

### `dbboard-ui::worker` (terminal reply plumbing)

- [ ] `Reply::AiResponded` / `AiStreamComplete` / `AiFailed` /
      `AiCancelled` each gain `provider: String, model: String`
      fields.
- [ ] The dispatch arms snapshot the slot's `identity()` once at
      spawn time and stamp it on every terminal reply (the slot
      can swap mid-request — the *spawn-time* identity is what
      the user actually got).
- [ ] Worker tests updated to assert provider/model land on the
      reply (the existing 11 tokio tests grow by one assertion
      each — minimal churn).

### `dbboard-ui::ai` + `dbboard-ui::lib` (the write point)

- [ ] `AiPanel` exposes the in-flight submit snapshot (the prompt
      + intent + start instant) so `lib.rs` can compose the AI
      `HistoryEntry` when a terminal reply lands. Either via a
      `submit_snapshot() -> Option<&AiSubmitSnapshot>` accessor or
      via the existing `streaming()` accessor extended with the
      submit metadata — slice (c) picks the smaller diff.
- [ ] `lib.rs` Reply dispatch arms for the four AI terminal
      variants build the `HistoryEntry::Ai { … }` and call
      `self.history.record_ai(entry)`. The ring + disk are updated
      symmetrically to the SQL path (`record_submit` →
      `record_completion`).
- [ ] No new `Reply` variant for AI history — the composition
      lives on the UI thread per ADR-0027 §Decision 6.

### Fixture + cross-repo

- [ ] `examples/emit_history_fixture` emits one `kind: "query"` +
      one `kind: "ai"` line minimum, both v:2.
- [ ] Brief 0008 lands in this PR with status `open` and the
      handoff procedure mirroring PR #29 / PR #31 (UTF-8 LF
      only, `--output PATH` flag to bypass PowerShell re-encoding).
- [ ] The fixture handoff itself (delivery to
      `dbboard-web/apps/api/test/fixtures/desktop-history.jsonl`)
      is **deferred** to a follow-up PR once the desktop side has
      merged — same pattern as the 2026-06-23 handoff.

### Docs

- [ ] `README.md` AI section gains a one-sentence warning: AI
      prompts and responses are logged verbatim in `history.jsonl`;
      ADR-0024 file permissions cover the at-rest threat model.
- [ ] `docs/roadmap.md` Phase 4 Stage 2 Group C tick to `[x]` with
      the PR number.
- [ ] `docs/decisions.md` ADR-0027 status flipped from `Proposed` to
      `Accepted` with the landing date and PR number on slice (d).
- [ ] `.claude/project-status.md` records the slice landing.
- [ ] `.claude/next-actions.md` updated post-slice-d to reflect
      Group C closure (Group D becomes the next standing option).
- [ ] This issue's status flipped to `closed` with the landing
      PR + date.

### Verification

- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
      clean.
- [ ] `cargo check --all-targets --all-features` clean.
- [ ] `cargo test --all-features` clean. Test count grows from 474
      + Group B's +22 to approximately +15 (≈8 history v:2 tests,
      ≈4 worker provider/model tests, ≈3 AiPanel snapshot tests).
- [ ] `cargo build --release` and `cargo test --all-features
      --release` clean (pre-push hook).
- [ ] No `unsafe` code introduced. Workspace `unsafe_code =
      "forbid"` upheld.

## Implementation slicing (suggested)

The four-slice cut is a **suggestion**, not a mandate. Whether
this lands as one PR or four is a slicing question — ADR-0027
does not prescribe.

- **Slice a**: `dbboard-ui::history` v:2 reader/writer +
  `HistoryEntry::Ai` variant + tests + fixture extension.
- **Slice b**: `dbboard-ai::AiProvider::identity()` +
  `AiResponse` provider/model + `dbboard-anthropic` impl + worker
  terminal-reply plumbing + tests.
- **Slice c**: `dbboard-ui` UI-thread write point — `AiPanel`
  submit snapshot + `lib.rs` `record_ai` dispatch + state tests.
- **Slice d**: docs sweep (README warning, roadmap, project-status,
  this issue ticked closed, ADR-0027 status flipped to Accepted,
  brief 0008 status updated).

## Out of scope (deferred to other groups)

- Logging the suggest-mode TableInfo schema-context blob —
  natural Group D follow-up once DDL extraction lands.
- Rich AI-record viewer in the history panel — Group C ships the
  record, the viewer is a follow-up PR.
- Multi-turn conversation threading — separate future ADR.
- Cost calculation from `tokens_in * input_price + tokens_out *
  output_price` — pricing tables change without notice.
- Server-side admin view over web's AI history store — web-side,
  out of brief 0008's Phase-2 scope.
