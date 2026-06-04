# 0003: dbboard-web query-history schema mirror (Phase 2)

- **Status**: open
- **Phase**: web-side Phase 2 (desktop-side equivalent: ADR-0017)
- **Opened**: 2026-06-04
- **Owner**: human (cross-repo handoff)
- **Target repo**: <https://github.com/meta-taro/dbboard-web>
- **Follows**: [0002-web-capabilities-mirror](0002-web-capabilities-mirror.md)
- **Anchors**: desktop ADR-0017 in [`docs/decisions.md`](../../docs/decisions.md)
  (search `## ADR-0017`); reference implementation in
  `crates/dbboard-ui/src/history.rs` at desktop commit `72cb165` (or later).

## What this is — and what it isn't

This is **not** a wire-contract mirror in the sense of issues 0001 and
0002. ADR-0017 §8 deliberately rejects a `GET /history` endpoint: no
HTTP shape, no server state in the dbboard sense, no shared code path.
`docs/api-contract.md` is untouched.

What **is** shared is the **per-record JSON schema** used to persist
query history. The maintainer's framing on 2026-06-03:

> 共有というのはログの吐き先とかではなく、同一ログ形式である

i.e. "shared" means *same log record format*, not same destination.
A history record emitted by desktop and a history record emitted by
web must be byte-compatible enough that a single `jq` filter, a single
schema validator, and a single forward-compat policy work on both.

Storage location, write path, rotation policy, and read API may all
diverge between the two repos. The record shape may not.

## Why now (not earlier, not later)

- **Not earlier**: the schema only stabilised when desktop ADR-0017
  landed on 2026-06-04. Mirroring before then would have been guessing
  at the `actor` semantics and the error-envelope nesting.
- **Not later**: every web query that runs without this in place
  either accumulates history in a divergent shape (a future migration
  liability) or accumulates no history at all (a UX regression
  relative to desktop). Either outcome makes Phase 3 work harder.
- **Phase 2 is the right boundary**: Phase 2 is the "capability +
  observability" phase on both repos. The record schema is the
  observability half on web; landing it alongside the capability
  mirror (issue 0002) keeps the two phases shipped together.

## Scope (what to mirror)

Implement the persistence layer described by ADR-0017 sections 1, 2,
5, 6, 7 of this repo, snapshotted at commit `72cb165` or later.
Concretely:

### Record schema (single source of truth — copy verbatim)

```jsonc
{
  "v": 1,                              // schema version
  "ts": "2026-06-04T14:22:01.123Z",   // RFC 3339, UTC, ms precision
  "conn": "prod-pg",                   // connection id
  "actor": "alice@example.com",        // web populates; desktop emits null
  "sql": "SELECT * FROM users LIMIT 10",
  "status": "ok",                      // "ok" | "error"
  "duration_ms": 42,                   // wall-clock submit -> envelope
  "rows": 10,                          // row-returning result; null otherwise
  "rows_affected": null,               // DML result; null otherwise
  "error": null                        // {category, message} when status="error"
}
```

Field-by-field constraints (web side):

- **`v`**: literal `1`. A schema bump on either side is an ADR-level
  decision negotiated through `docs/decisions.md` on both repos. Do
  not bump `v` unilaterally.
- **`ts`**: RFC 3339, **UTC** (`Z` suffix), **millisecond** precision.
  No microseconds, no offset suffix. Day.js / `Intl` will not produce
  this shape by default — use `new Date().toISOString()` (which is
  exactly this shape in modern Node) or `date-fns/formatISO` with
  `representation: 'complete'` and slice to `.sssZ`.
- **`conn`**: the connection identifier the web service uses to
  resolve the backend for this request. Stable string, lowercase
  preferred. Web's notion of "connection" is the server-side connection
  record, not a per-user alias.
- **`actor`**: **populated** on web (`string | null`). The expected
  value is the authenticated user id / email / opaque session subject
  — whatever your auth layer hands the request handler. `null` is
  reserved for the desktop case and for web *unauthenticated*
  requests (if any survive Phase 2). Do not emit an empty string.
- **`sql`**: the literal SQL text. **Do not redact, do not lex, do
  not normalise.** ADR-0017 §7 is explicit about this: a redactor
  would be a perpetually wrong heuristic, and verbatim is the prior
  art across DBeaver / DataGrip / pgAdmin. See "Secret handling" below
  for the security implication on web.
- **`status`**: `"ok"` or `"error"`, lowercase. Future additions
  (`"cancelled"`, `"timeout"`) are additive — writers emit, readers
  default to unknown.
- **`duration_ms`**: integer milliseconds, wall-clock from request
  receipt to response envelope formation. **Not** server processing
  time minus queue time. On error, the duration up to the error.
- **`rows` / `rows_affected`**: mutually exclusive. SELECT-class
  returns `rows: <int>, rows_affected: null`. DML returns
  `rows: null, rows_affected: <int>`. DDL or `ok` with no result
  population returns both `null`.
- **`error`**: when `status == "error"`, an object
  `{ "category": "<connection|query|schema|type_conversion|capability>", "message": "<English text>" }`.
  Categories match the desktop `DbError` taxonomy (ADR-0009 /
  ADR-0004 / ADR-0012). Message is raw English — UI translation is
  not applied to log records (the file must stay locale-agnostic so
  cross-team `jq` works).

### Forward-compat policy

- **Writers** emit only the fields above. Adding a new optional field
  is allowed (it does not bump `v`); deleting or renaming an existing
  field is **not** allowed without an ADR on both repos.
- **Readers** must use the equivalent of serde's
  `#[serde(default)]` without `#[serde(deny_unknown_fields)]`:
  unknown fields are ignored, missing optional fields take their
  default. Records with `"v"` that is not `1` or `"status"` outside
  the known set are **dropped with a counter increment**, not parsed
  partially.

### Storage (web may diverge here — and probably should)

ADR-0017 §3 picks `directories::ProjectDirs` + a single `history.jsonl`
file because desktop is single-user, single-process. Web is neither.
The following are **non-binding suggestions** for web; the only hard
rule is "the per-record shape is identical."

- **Per-tenant Postgres table** is the obvious primary target — one
  row per record with a `jsonb payload` column whose value is exactly
  the record above. A `payload->>'v' = '1'` partial index keeps the
  forward-compat path cheap.
- **Stream as `.jsonl` for export**. Whatever the canonical store is,
  `GET /history/export.jsonl` (or equivalent) **must** stream records
  in the schema above so a user who migrates from desktop to web (or
  the reverse) can `cat dbboard-history.jsonl | curl --data-binary @-`
  without an intermediate transform.
- **Do not introduce a `created_at` column that diverges from `ts`.**
  Either reuse `payload->>'ts'` directly or set `created_at = ts` at
  insert time. Two timestamps that drift is a debugging trap.

### Rotation / retention (web's call)

ADR-0017 §4 picks 50 MiB / 100 000 lines on desktop because the file
is the canonical store. Web's canonical store is a database; the
equivalent question is "retention policy on the history table." That
is a web-side product decision, not a schema decision — pick whatever
fits your tenant model.

### Secret handling — web has more responsibility here than desktop

ADR-0017 §7 logs queries verbatim because the desktop file lives
under the OS user's own config dir at the same trust level as
`connections.toml`. **On web, the equivalent file lives on a
shared server** — verbatim logging means the operator can read every
tenant's `WHERE token = 'sk_live_…'`. This is a *deployment* policy,
not a *schema* policy, but the brief must flag it:

- The schema is the same. The contents must be the same.
- The **access control** around that store is a web-side ADR.
  Suggested defaults: tenant-scoped read API, retention policy that
  matches whatever your audit log retention is, and no admin-side
  "show me all queries" affordance unless your privacy stance allows it.
- A future "redact-at-read" capability could be a Stage 3 cross-repo
  ADR. It would not change the schema; it would add a server-side
  filter in front of the export endpoint.

## Out of scope for Phase 2 (intentionally)

- `GET /history` over HTTP as part of the cross-repo contract.
  ADR-0017 §8 keeps history off the wire. If the web UI needs a
  history endpoint internally, that is a web-only API and does not
  need to mirror anything desktop-side.
- A `v: 2` schema (multi-statement results, query plan, etc.). Wait
  for desktop to ship it under a new ADR before mirroring.
- Encryption at rest. Desktop §7 rejects it for the `jq`
  differentiator reason; web's decision is independent and orthogonal
  to schema mirror work.
- An admin-side "tenant analytics" view over the history store.
  Out of scope until the schema and access-control stories are both
  shipped.

## Acceptance

- [ ] Web's persistence layer writes a JSON object per query
  completion whose shape is byte-compatible with the ADR-0017 schema
  (same field names, same types, same `v`, same `status` values, same
  error envelope nesting).
- [ ] The `ts` field is RFC 3339 UTC with millisecond precision.
  A test asserts this round-trips through `Date.parse` and
  `Date.prototype.toISOString` without drift.
- [ ] The `actor` field is populated from the authenticated session
  for any logged-in request; `null` for unauthenticated.
- [ ] The `error` envelope uses categories from the desktop
  `DbError` taxonomy (`connection`, `query`, `schema`,
  `type_conversion`, `capability`). A web-internal category that
  doesn't exist on desktop is a contract violation.
- [ ] A reader tolerates unknown fields and skips records with
  unknown `v` / unknown `status`. A counter is exposed (log line,
  metric, whatever fits) so the skip count is observable.
- [ ] An export endpoint streams `application/x-ndjson` with one
  ADR-0017 record per line. Round-trip via `jq -c .` is a no-op.
- [ ] The web sibling ADR cites desktop ADR-0017 by anchor (file +
  date) rather than copying the schema, so a single source of truth
  is preserved.

## Tech recommendations (non-binding for web)

- **Schema definition**: a single Zod (or class-validator) schema in
  a shared TS file, used both at write (`safeParse` before insert)
  and at export (parse on read for validation). The runtime shape is
  cheap; a bug that emits an extra field or a wrong-typed field is
  the high-cost class of error.
- **Per-record write**: do **not** batch writes across requests at
  the schema layer. The schema is a per-event shape; batching at the
  storage layer (single `INSERT … VALUES (...), (...)` per request
  burst) is fine and is an internal optimisation. Do not invent a
  "batch envelope" — the desktop side has no concept of one.
- **Forward-compat test**: a unit test that constructs a record with
  an unknown field (e.g. `"future_field": 42`) and asserts the
  reader (a) accepts it, (b) does not echo the unknown field on read.
- **Schema-bump discipline**: when (not if) a future ADR bumps `v`
  to `2`, both repos must land the change in the same window. A
  Stage 2.1 with a `v` mismatch is a worse user experience than
  delaying the schema bump by a sprint.

## Handoff procedure

1. Push the pending desktop commits
   (`62ed834..72cb165`, plus this issue's docs commit) so the
   `dbboard-web` planner can read ADR-0017 at a stable point.
2. In `dbboard-web`, open this issue (or its GitHub equivalent), link
   back to this file and to
   `dbboard@72cb165:docs/decisions.md` (ADR-0017 anchor).
3. Set this file's status to `in-progress` once web work starts, then
   `done` (with the web PR link) when acceptance criteria are met.

## Notes

- The schema constraint is *the per-record JSON*, not the file
  format. Web is free to store records in Postgres rows, in Redis
  streams, in BigQuery, or in `.jsonl` on disk — as long as the
  per-record shape round-trips through `jq` against a desktop file
  without surprises.
- If web discovers an ambiguity (e.g. "what does `duration_ms` mean
  when the query is cancelled mid-stream?"), file it back as a
  desktop-side ADR ticket rather than diverging silently.
- ADR-0017 §9 explicitly designates this brief as the cross-repo
  coordination artefact. Keep it updated as the source of truth for
  the schema mirror status, not as a one-shot dump.
