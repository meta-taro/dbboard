# 0010: AI calls recorded in `history.jsonl` (Phase 4 Stage 2 Group C, schema v:2)

- **Status**: closed 2026-07-01 (landed on `feature/ai-history-v2` across four commits: slice a `b16537f` + slice b `13f7736` + slice c `0e76223` + slice d = this commit)
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

### `dbboard-ui::history` (the v:2 module) — slice (a) `b16537f`

- [x] `CURRENT_VERSION` bumped from `1` to `2`.
- [x] `RecordWire` becomes a flat struct with optional fields and a
      `kind: "query" | "ai"` discriminator. v:1 records (no `kind`,
      `sql` present) read transparently as `kind: "query"`.
- [x] New `HistoryEntry::Ai { … }` variant. The existing
      `HistoryEntry` (the v:1 SQL shape) becomes
      `HistoryEntry::Query { … }`. Public API renames are part of
      slice (a) and ripple into call sites in slice (c).
- [x] Reader unit tests: v:2 query record round-trips, v:2 AI
      record round-trips, v:1 record reads as `Query`, v:2 record
      with unknown `kind` skips + counter ticks, v:2 record with
      unknown `intent` skips + counter ticks.
- [x] Writer unit tests: query write produces v:2 + kind="query",
      AI write produces v:2 + kind="ai" with every documented
      field present (nulls preserved per ADR-0027 §Decision 4).
- [x] Truncation: prompt and response capped at 64 KiB at write
      time with the `[truncated at 64 KiB]` marker appended
      (Decision 10). Unit test asserts the cap.
- [x] `fixture::serialize` (the doc-hidden shim) extends to AI
      records.

### `dbboard-ai` (trait + value types) — slice (b) `13f7736`

- [x] New `AiProvider::identity(&self) -> (&'static str, &str)`
      returning `(provider_id, model_id)`. Default impl returns
      `("unknown", "")` so existing in-tree mocks compile.
- [x] `AiResponse` gains `provider: String, model: String` fields.
- [x] `StreamEvent::MessageStop` already carries `stop_reason`;
      the worker copies it through to the new `Reply` fields.
- [x] No new dependency.

### `dbboard-anthropic` — slice (b) `13f7736`

- [x] `AnthropicProvider::identity()` returns
      `("anthropic", &self.model)`.
- [x] `AiResponse` construction populates provider/model.
- [x] All existing tests pass unchanged (the new fields are
      populated; assertions ignore them).

### `dbboard-ui::worker` (terminal reply plumbing) — slice (b) `13f7736`

- [x] `Reply::AiResponded` / `AiStreamComplete` / `AiFailed` /
      `AiCancelled` each gain `provider: String, model: String`
      fields.
- [x] The dispatch arms snapshot the slot's `identity()` once at
      spawn time and stamp it on every terminal reply (the slot
      can swap mid-request — the *spawn-time* identity is what
      the user actually got).
- [x] Worker tests updated to assert provider/model land on the
      reply (the existing 11 tokio tests grow by one assertion
      each — minimal churn).

### `dbboard-ui::ai` + `dbboard-ui::lib` (the write point) — slice (c) `0e76223`

- [x] `AiPanel` exposes the in-flight submit snapshot (the prompt
      + intent + start instant) so `lib.rs` can compose the AI
      `HistoryEntry` when a terminal reply lands. Chose the
      `PendingAiSubmit` shape on `DbboardApp` (mirroring
      `PendingSubmit` for SQL, ADR-0017 model) over an
      `AiPanel::submit_snapshot()` accessor — the write point is
      already a `DbboardApp` responsibility and the pending record
      never needs to survive an AI panel state reset.
- [x] `lib.rs` Reply dispatch arms for the four AI terminal
      variants build the `HistoryEntry::Ai { … }` and call
      `self.history.record_ai(entry)`. The ring + disk are updated
      symmetrically to the SQL path via the existing
      `PersistentHistoryStore` API.
- [x] No new `Reply` variant for AI history — the composition
      lives on the UI thread per ADR-0027 §Decision 6.

### Fixture + cross-repo — slice (a) + slice (d)

- [x] `examples/emit_history_fixture` emits one `kind: "query"` +
      one `kind: "ai"` line minimum, both v:2. Delivered as part of
      slice (a) `b16537f` — the fixture example now emits 11 lines
      (10 query + 1 AI, all v:2) with a pinned assertion in
      `fixture_output_matches_brief_conventions`.
- [x] Brief 0008 lands in this PR with status `open` and the
      handoff procedure mirroring PR #29 / PR #31 (UTF-8 LF
      only, `--output PATH` flag to bypass PowerShell re-encoding).
- [x] The fixture handoff itself (delivery to
      `dbboard-web/apps/api/test/fixtures/desktop-history-v2.jsonl`)
      is **deferred** to a follow-up PR once the desktop side has
      merged — same pattern as the 2026-06-23 handoff. Tracked on
      brief 0008 Handoff procedure §3.

### Docs — slice (d)

- [x] `README.md` AI section gains a warning: AI prompts and
      responses are logged verbatim in `history.jsonl` as
      `kind: "ai"` records; ADR-0024 file permissions cover the
      at-rest threat model.
- [x] `docs/roadmap.md` Phase 4 Stage 2 Group C ticked to `[x]`
      with the four-slice commit ID rollup.
- [x] `docs/decisions.md` ADR-0027 status flipped from `Proposed`
      to `Accepted` (2026-07-01) with the four-slice commit ID
      rollup embedded in the status body.
- [x] `.claude/project-status.md` records the slice landing.
- [x] `.claude/next-actions.md` updated post-slice-d to reflect
      Group C closure (Group D becomes the next standing option).
- [x] This issue's status flipped to `closed` with the landing
      slice trail.

### Verification

- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
      clean.
- [x] `cargo check --all-targets --all-features` clean.
- [x] `cargo test --all-features` clean. Test count grew per slice;
      `dbboard-ui` alone picked up 18 new unit tests on slice (c)
      covering the AI history helpers and all four terminal-reply
      arms.
- [x] `cargo build --release` and `cargo test --all-features
      --release` clean (pre-push hook, re-run at slice-d).
- [x] No `unsafe` code introduced. Workspace `unsafe_code =
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
