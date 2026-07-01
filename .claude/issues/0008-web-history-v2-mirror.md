# 0008: dbboard-web `history.jsonl` v:2 mirror (AI records)

- **Status**: open
- **Phase**: web-side follow-up to desktop ADR-0027 (Phase 4 Stage 2
  Group C)
- **Opened**: 2026-06-30
- **Owner**: human (cross-repo handoff)
- **Target repo**: <https://github.com/meta-taro/dbboard-web>
- **Follows**: [0003-web-history-schema-mirror](0003-web-history-schema-mirror.md)
  (the v:1 mirror brief)
- **Anchors**: desktop ADR-0027 in [`docs/decisions.md`](../../docs/decisions.md)
  (search `## ADR-0027`, Accepted 2026-07-01); v:2 reference
  implementation in `crates/dbboard-ui/src/history.rs` on
  `feature/ai-history-v2` at the four-slice landing (slice a
  `b16537f` = reader + writer, slice b `13f7736` = provider identity,
  slice c `0e76223` = UI write point, slice d = this docs sweep).
  The desktop merge commit ID lands here in the post-merge doc-sync
  chore PR — see "Handoff procedure" §2 below.

## What this is

A **schema bump** of the per-record JSON contract from brief 0003.
The desktop side moved from v:1 (SQL-only records) to v:2 (SQL records
*plus* AI-call records), introducing a top-level `"kind"`
discriminator. Brief 0003's stance still holds in full — *what is
shared is the per-record JSON shape, not the storage destination or
the read API*.

This is **not** a wire-contract mirror in the HTTP-endpoint sense.
The desktop side has no `/history` endpoint and ADR-0017 §8 +
ADR-0027 reaffirm that decision. `docs/api-contract.md` is untouched.

## Why now

- Desktop ADR-0027 ships AI history recording. v:1 web readers will
  see desktop-emitted v:2 records and skip them (counter ticks — the
  forward-compat path from brief 0003 working as designed). Mirroring
  is the move that converts those skipped records into observable
  history on the web side.
- The pattern established by brief 0003 reserved v:2 for "multi-record
  types" — this is exactly that use. The mechanism is ready; this
  brief activates it.
- **Lead time matters**: this brief lands *in the same PR as the
  desktop ADR-0027 draft*, so web's planning starts before desktop
  merges — matches the lead-time rule that made PR #33's
  explicit-no-op briefs (0006 / 0007) usable on the web side.

## Scope (what to mirror)

Implement the persistence layer described by ADR-0027 (desktop),
snapshotted at the desktop merge commit. Concretely:

### Record schemas (single source of truth — copy verbatim)

**`kind: "query"`** — the v:1 shape rebadged. Every field from
brief 0003's record schema, with two changes:

```jsonc
{
  "v": 2,                              // was 1
  "kind": "query",                     // NEW — discriminator
  "ts": "2026-06-30T05:12:01.456Z",
  "conn": "prod-pg",
  "actor": "alice@example.com",
  "sql": "SELECT * FROM users LIMIT 10",
  "status": "ok",                      // "ok" | "error"
  "duration_ms": 42,
  "rows": 10,
  "rows_affected": null,
  "error": null
}
```

**`kind: "ai"`** — the new shape.

```jsonc
{
  "v": 2,
  "kind": "ai",
  "ts": "2026-06-30T05:12:01.456Z",
  "conn": null,                        // OPTIONAL — null when no DB context
  "actor": null,                       // web populates if authenticated
  "intent": "explain",                 // "explain" | "suggest_sql"
  "prompt": "SELECT * FROM users …",
  "response": "This query reads …",
  "status": "ok",                      // "ok" | "error" | "cancelled"
  "duration_ms": 4231,
  "tokens_in": 412,                    // OPTIONAL — null when unknown
  "tokens_out": 218,                   // OPTIONAL — null when unknown
  "provider": "anthropic",
  "model": "claude-sonnet-4-6",
  "stop_reason": "end_turn",           // "end_turn" | "max_tokens" | "stop_sequence" | "tool_use" | "refusal" | "other:<text>" | null
  "error": null                        // {category, message} when status="error"
}
```

Field-by-field constraints (web side, AI-specific):

- **`v`**: literal `2`. A future bump is an ADR-level decision on both
  repos (same rule as brief 0003).
- **`kind`**: literal `"query"` or `"ai"`. Unknown values drop the
  record + counter tick. This is the dispatch primitive.
- **`intent`**: `"explain"` (AI explains user-supplied SQL) /
  `"suggest_sql"` (AI generates SQL from a natural-language prompt).
  Unknown values drop + counter (same gate as unknown `status`).
- **`prompt`**: the user input verbatim. For `explain`, the SQL the
  user pasted. For `suggest_sql`, the natural-language request.
  **Do not redact, do not lex, do not normalise** — same stance as
  v:1's `sql` field (ADR-0017 §7 / ADR-0027 §Decision 8).
- **`response`**: the AI text verbatim. On `status: "cancelled"`,
  this carries the partial accumulator at cancel time — the user paid
  for those bytes; the record preserves them (ADR-0026 Decision 12
  carried through to persistence).
- **`status`**: `"ok"` | `"error"` | `"cancelled"`. `cancelled`
  carries `error: null`. Lowercase. Additive future values
  (`"timeout"`, etc.) drop + counter on this reader.
- **`tokens_in` / `tokens_out`**: integer or null. Null when the
  provider didn't surface a `Usage` event before the terminal one
  (atomic-default-impl path, or cancel before first chunk). When
  present, the value is the cumulative token count at terminal time
  (replace-not-sum on the writer side — ADR-0026 Decision 7).
- **`provider`**: provider id (lowercase short name). Stable string
  suitable for `jq 'select(.provider == "anthropic")'`.
- **`model`**: model id string as the provider reports it.
- **`stop_reason`**: enum-on-the-wire string, or null. Values:
  `"end_turn"` | `"max_tokens"` | `"stop_sequence"` | `"tool_use"`
  | `"refusal"` | `"other:<text>"` (forward-compat escape hatch for
  the `StopReason::Other(String)` variant). Unknown values are
  *tolerated*, not skipped — this field is informational, not
  load-bearing.
- **`error.category`** for AI records: `"network"` | `"provider"` |
  `"configuration"`. Mirrors the `AiError` variants from ADR-0023
  §5. **`"cancelled"` is NOT an error category** — cancel is a
  top-level `status`. A web-internal AI category that doesn't exist
  on desktop is a contract violation (same rule brief 0003 set for
  the DbError taxonomy).

### Forward-compat policy (carries over from brief 0003, refined)

- **Writers** emit only the documented fields per `kind`. Adding a
  new optional field within a `kind` is allowed (no `v` bump).
  Adding a new `kind` value requires a coordinated ADR + brief on
  both repos (this is the *protocol* for future Group X expansions).
- **Readers** use the equivalent of serde's `#[serde(default)]`
  without `#[serde(deny_unknown_fields)]`. Unknown top-level fields
  are ignored. Records with unknown `v` / unknown `kind` / unknown
  `status` / unknown `intent` are **dropped with a counter
  increment**, not parsed partially.
- **v:1 records remain readable** by v:2 readers — implicit
  `kind: "query"` when the field is absent.

### Storage (web may diverge — and probably will)

Same stance as brief 0003 §Storage. The hard rule is "per-record
shape is identical." The recommendations carry over:

- Per-tenant Postgres table with `jsonb payload`. Add a
  `payload->>'kind'` partial index so `WHERE kind = 'ai'` is cheap.
- Export endpoint streams `application/x-ndjson` with v:2 records.
  `jq -c .` round-trip is a no-op.
- `created_at` column reuses `payload->>'ts'`.

### Secret handling — same web responsibility as brief 0003, more
acute

AI prompts and responses on web mean *the operator can read every
tenant's natural-language questions and the AI's verbatim
responses*. Same `WHERE token = 'sk_live_…'` issue as v:1's `sql`
field, applied to a domain where users tend to paste even more
sensitive content (debugging help, schema explanations of internal
tables, etc.).

- The schema is the same. The contents must be the same.
- Web's access control around this store remains a web-side ADR.
- A future "AI-prompt redact-at-read" would be a Stage 3 cross-repo
  ADR. It would not change the schema.

## Out of scope for this brief (intentionally)

- `GET /ai-history` over HTTP as part of the cross-repo contract.
  Same stance as brief 0003 §8 — history stays off the wire.
- v:3 schema (multi-turn AI threads, schema-context blob,
  cost calculation, etc.). Wait for a desktop ADR before mirroring.
- An admin-side "all-tenants AI usage" view. Web product decision.
- A web-only AI provider that desktop doesn't have. Allowed; the
  `provider` field is a free string. Just don't introduce a new
  `intent` value or `error.category` without a desktop ADR — those
  are the contract surfaces.

## Acceptance

- [ ] Web's persistence layer writes a JSON object per query
      completion and per AI completion whose shape is byte-compatible
      with the ADR-0027 schema (same field names, same types, same
      `v: 2`, same `kind` values, same `status` values, same `intent`
      values, same `error` envelope nesting).
- [ ] v:1 records on disk continue to read transparently as
      `kind: "query"`. A test asserts a v:1 record loads as the
      equivalent v:2 query record.
- [ ] AI records with `status: "cancelled"` carry `error: null`
      (the cancel-is-not-an-error invariant from ADR-0026 Decision
      12 / ADR-0027 Decision 5).
- [ ] A reader tolerates unknown top-level fields and skips records
      with unknown `v` / unknown `kind` / unknown `status` / unknown
      `intent`. The skip counter is observable (log line, metric,
      whatever fits).
- [ ] The export endpoint streams v:2 records in NDJSON. Round-trip
      via `jq -c .` is a no-op.
- [ ] The web sibling ADR cites desktop ADR-0027 by anchor (file +
      date) rather than copying the schema, so the single source of
      truth is preserved.

## Tech recommendations (non-binding for web)

- **Schema definition**: a Zod (or class-validator) discriminated
  union on `kind`, used both at write (`safeParse` before insert) and
  at export (parse on read for validation). The runtime check is
  cheap; a bug that emits a wrong-typed `tokens_out` is the high-cost
  class of error.
- **Forward-compat test**: a unit test that constructs a v:2 AI
  record with an unknown `intent: "summarize"` and asserts the reader
  drops it and increments the counter.
- **v:1 round-trip test**: a unit test that loads a v:1 record (from
  brief 0003's fixture) and asserts it lands as a `kind: "query"`
  v:2 in-memory record.
- **Schema-bump discipline**: when v:3 lands (multi-turn AI threads,
  for example), both repos must land in the same window — same rule
  brief 0003 set, same reason.

## Handoff procedure

1. Desktop opens the PR that includes ADR-0027 + this brief. Web
   side reads the brief at draft stage so planning starts before
   merge.
2. Once desktop merges, the post-merge doc-sync chore PR updates
   the **Anchors** section above with the desktop merge commit ID
   (mirrors the PR #33 / PR #29 fixture-handoff pattern).
3. Desktop side emits a v:2 fixture via `cargo run --example
   emit_history_fixture --output PATH`. Hand-off file lands at
   `dbboard-web/apps/api/test/fixtures/desktop-history-v2.jsonl`
   (the v:1 fixture stays at the existing path for the back-compat
   test). UTF-8 LF only; the `--output` flag bypasses PowerShell
   re-encoding.
4. In `dbboard-web`, open this issue (or its GitHub equivalent), link
   back to this file and to the desktop ADR-0027 anchor.
5. Set this file's status to `in-progress` once web work starts,
   then `done` (with the web PR link) when acceptance criteria are
   met.

## Notes

- The schema constraint is *the per-record JSON*, not the file
  format. Web is free to store records in Postgres rows, in Redis
  streams, in BigQuery, or in `.jsonl` on disk — as long as the
  per-record shape round-trips through `jq` against a desktop file
  without surprises.
- If web discovers an ambiguity (e.g. "what does `duration_ms` mean
  when an AI stream is cancelled before the first chunk?"), file it
  back as a desktop-side ADR ticket rather than diverging silently.
- ADR-0027 explicitly designates this brief as the cross-repo
  coordination artefact for the v:2 bump. Keep it updated as the
  source of truth for the schema mirror status, not as a one-shot
  dump.
