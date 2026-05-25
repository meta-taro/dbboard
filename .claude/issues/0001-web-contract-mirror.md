# 0001: dbboard-web contract mirror (Phase 1)

- **Status**: open
- **Phase**: web-side Phase 1 (desktop-side equivalent: Phase 1.5 closeout)
- **Opened**: 2026-05-25
- **Owner**: human (cross-repo handoff)
- **Target repo**: <https://github.com/meta-taro/dbboard-web>

## Context

The desktop side (`dbboard`) closed Phase 1 at workspace `0.1.0` with a
stable HTTP contract documented in
[`docs/api-contract.md`](../../docs/api-contract.md). Per ADR-0011 the
contract is the public API governed by SemVer; per the Pacing Note in
`docs/roadmap.md` the next sprint belongs to `dbboard-web`.

This issue tracks the desktop-side handoff: a brief the human can carry
into the `dbboard-web` repository to seed its Phase 1 implementation.
The actual implementation work happens in `dbboard-web` and is out of
scope for this repo.

## Why now (not earlier, not later)

- **Not earlier**: until the contract stabilised, mirroring would have
  meant chasing a moving target. Phase 1 / 1.5 / 1.6 / 1.7 are now
  shipped and `0.1.0` is tagged.
- **Not later**: Phase 2 (ADR-0012, capability model) is purely
  additive — new endpoint prefixes (`/capabilities`, `/views`, etc.)
  and a new `capability` error category. It does not change the three
  existing endpoints. So a mirror built today is forward-compatible:
  the web side will add the capability endpoints in a later sprint
  without rewriting Phase 1.
- **ADR-0011 ties 1.0.0 to web interop**: `1.0.0` release requires the
  contract to be proven interoperable between the two repos. Web work
  is on the critical path for `1.0.0`.

## Scope (what to mirror)

Implement the exact surface defined in `docs/api-contract.md` of this
repo, snapshotted at commit `075a879` or later. Concretely:

### Endpoints
- `GET /health` → `200 { "status": "ok" }`
- `GET /tables` → `200 { "tables": [TableInfo, ...] }`
- `POST /query` body `{ "sql": "..." }` → `200 QueryResult`

### Data shapes
- `Value` — `Null` / `Integer(i64)` / `Real(f64)` / `Text` / `Blob`.
  Blob is `{ "$blob": "<standard-base64>" }` (exactly one key).
- `QueryResult` — `{ columns: Column[], rows: Value[][], rows_affected: u64 }`.
  `rows` is an array of bare arrays aligned with `columns`.
- `Column` — `{ name, declared_type | null }`.
- `TableInfo` — `{ schema | null, name }`.

### Errors
- Envelope: `{ "error": { "category", "message" } }`.
  `message` is **bare** (no category prefix) so the client can
  reconstruct the domain error without doubling.
- Categories → HTTP status:
  - `query` → 400
  - `type_conversion` → 422
  - `connection` → 502
  - `schema` → 502
- Unknown category seen by a client degrades to `query` (forward-compat).

### Request-level rejections (plain-text body, not the envelope)
- Body not valid JSON → 400
- Missing `sql` field → 422
- Content-Type not `application/json` → 415
- Body exceeds 64 KiB → 413

### Row cap (security-relevant)
- Each query is capped at **10,000 rows** uniformly across adapters.
- Over-cap is rejected with a `query` error (400), not silently
  truncated. The error message should hint "add a LIMIT clause".
- This is the row-count contract, not a byte cap. Web side should
  enforce identically.

## Out of scope for Phase 1 (intentionally)

- `GET /capabilities` and the `capability` error category. These come
  from ADR-0012 and **do not exist in desktop code yet**. They will be
  contract-amended after desktop Phase 2 lands them, then re-mirrored
  to web. Do not pre-implement them; the shape is still subject to
  change.
- AI provider endpoints (Phase 4 on desktop). Not in contract.
- Streaming / pagination as a row-cap escape hatch (Phase 2 desktop
  follow-on, separate ADR).

## Acceptance

- [ ] `dbboard-web` ships a NestJS service that conforms to the three
  endpoints with at least **one** real adapter (Postgres recommended —
  it overlaps with what the desktop `dbboard-postgres` adapter covers).
- [ ] Error envelope and status codes match the contract exactly.
- [ ] Request-level rejections (415 / 413 / 422-missing-sql) are
  exercised by tests against plain-text bodies, not envelopes.
- [ ] 10,000-row cap enforced with an integration test that asserts a
  `query` 400 (not a 200 with truncated `rows`).
- [ ] A contract-conformance test runs the *same* requests against
  both the desktop loopback server and the web service and compares
  responses for at least: `/health`, `/tables` on an empty database,
  `POST /query` for `SELECT 1`, an over-cap `SELECT`, and an invalid
  SQL statement.
- [ ] `dbboard-web/docs/api-contract.md` exists and is byte-identical
  (or reduced to a `# Mirror of desktop` pointer + delta notes) to
  this repo's version at the snapshot commit.

## Tech recommendations (non-binding for web)

- **Stack**: NestJS (already chosen for web). Use class-validator /
  class-transformer for request validation, not hand-rolled checks —
  the 422 vs 400 split needs to be deterministic.
- **Adapter first**: a single Postgres adapter using `pg` or
  `postgres.js`. Mirror the desktop Postgres adapter's behaviour
  (text-format decoding for unknown types, `sslmode=prefer` upgraded
  to `require`).
- **Body limits**: configure Express body-parser limit to 64 KiB and
  ensure 413 fires with plain text, not JSON envelope.
- **Conformance test transport**: web side runs the desktop binary in
  the test setup (or skips when unavailable, gated on an env var like
  desktop's `DBBOARD_PG_URL` pattern).

## Handoff procedure

1. Push the four pending desktop commits (`bad80e0..075a879`) so
   `dbboard-web` planners can read the contract at a stable tag.
2. In `dbboard-web`, open this issue (or its GitHub equivalent), link
   back to this file and to `dbboard@075a879:docs/api-contract.md`.
3. Set this file's status to `in-progress` once web work starts, then
   `done` (with the web PR link) when acceptance criteria are met.

## Notes

- The contract was last extended on 2026-05-25 with the 10,000-row cap
  rule (Phase 1.7 closeout). Make sure the snapshot includes it.
- Do not let web invent new endpoints unilaterally — contract changes
  go through `docs/decisions.md` in both repos per ADR-0004.
- If web discovers an ambiguity in the contract document, file it back
  as a desktop-side ADR ticket rather than diverging silently.
