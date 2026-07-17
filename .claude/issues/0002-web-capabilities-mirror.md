# 0002: dbboard-web capabilities mirror (Phase 2)

- **Status**: open
- **Phase**: web-side Phase 2 (desktop-side equivalent: Phase 2 / ADR-0012)
- **Opened**: 2026-05-27
- **Owner**: human (cross-repo handoff)
- **Target repo**: <https://github.com/meta-taro/dbboard-web>
- **Follows**: [0001-web-contract-mirror](0001-web-contract-mirror.md)

## Context

Desktop Phase 2 lands the capability model from ADR-0012:

- a small required `DatabaseAdapter` trait shared by every adapter, plus
  `Option<&dyn Capability>` accessors per optional feature;
- a flat `Capabilities` struct that advertises which optional surfaces an
  adapter exposes;
- a new HTTP endpoint `GET /capabilities` so the UI can discover the
  connected adapter without probing each feature endpoint;
- a new `capability` error category (HTTP 404) for "this adapter doesn't
  implement that feature".

The shape was held back from issue 0001 on purpose ("Out of scope for
Phase 1 (intentionally)") because it had not yet been implemented on
desktop. It is now implemented at desktop commit `1c350f6` and
documented in [`docs/api-contract.md`](../../docs/api-contract.md). This
issue tracks the desktop-side handoff so `dbboard-web` can mirror the
addition.

The actual implementation work happens in `dbboard-web` and is out of
scope for this repo.

## Why now (not earlier, not later)

- **Not earlier**: the shape only stabilised when desktop Phase 2
  landed. Issue 0001 explicitly deferred it; mirroring before
  `1c350f6` would have been guessing.
- **Not later**: Phase 2 is purely **additive** to the Phase 1 surface
  (one new endpoint, one new error category, one new data shape — no
  existing endpoint or shape changes). A web service that mirrors
  Phase 1 today will still conform; this issue only adds the new
  surface. Delaying just leaves the two repos out of step.
- **ADR-0011 ties `1.0.0` to web interop**: the conformance test from
  issue 0001 needs to grow to cover `/capabilities` before `1.0.0`
  ships.

## Scope (what to mirror)

Implement the additions to `docs/api-contract.md` of this repo,
snapshotted at commit `1c350f6` or later. Concretely:

### Endpoint

- `GET /capabilities` → `200 { "id": "<adapter>", "capabilities": Capabilities }`
  - `id` is a lowercase, adapter-stable identifier. Current desktop
    values: `"turso"`, `"d1"`, `"postgres"`. The web side picks its own
    id for each adapter it ships (`"postgres"` is the obvious mirror).
  - The response **must not** include adapter-specific fields beyond
    `id` and `capabilities` — the contract is closed.

### Data shape

- `Capabilities` — flat object of `snake_case` boolean flags, one per
  optional capability in ADR-0012:
  ```jsonc
  {
    "has_views": false,
    "has_functions": false,
    "has_auth": false,
    "has_storage": false,
    "has_realtime": false
  }
  ```
  - Phase 2 ships every flag as `false`. Per-feature endpoints land in
    later phases alongside flipping the flag.
  - **Forward-compat**: clients must tolerate **additional** flags.
    Treat unknown flags as the safest default (typically `false`).
    Renaming or removing a flag is a breaking change governed by
    `docs/decisions.md` on both repos.

### Error category

- New category `capability` → HTTP **404 Not Found**.
  - Returned when the caller hits a capability the adapter does not
    implement (e.g. `GET /views` against a SQLite-class adapter).
  - Distinct from `query` so the UI can hide/grey the feature cleanly
    instead of surfacing it as a SQL error.
  - Same envelope as every other category:
    `{ "error": { "category": "capability", "message": "..." } }`,
    with `message` bare (no prefix).

## Out of scope for Phase 2 (intentionally)

- Per-feature endpoints (`/views`, `/functions`, `/auth/*`, `/storage/*`,
  `/realtime/*`). ADR-0012 reserves the surface; the endpoints
  themselves are sequenced behind flipping each flag and ship in
  later phases with their own contract amendments.
- Capability **negotiation** by the client (e.g. "send me only the flags
  you understand"). The flat additive object is the negotiation
  mechanism — clients filter on read.
- AI provider endpoints (Phase 4 on desktop). Still not in contract.
- Streaming / pagination as a row-cap escape hatch (still a separate
  ADR, not part of capability work).

## Acceptance

- [ ] `dbboard-web`'s NestJS service exposes `GET /capabilities` with
  the response shape above for **at least** the adapter shipped in
  issue 0001 (Postgres).
- [ ] The web `Capabilities` DTO is the flat snake_case shape — no
  nesting, no `enabled: bool` wrappers, no per-capability sub-objects.
- [ ] An adapter that doesn't implement a requested capability returns
  the `capability` envelope with HTTP 404 (not 400, not 501).
- [ ] The conformance test from issue 0001 grows a case that calls
  `GET /capabilities` against both the desktop loopback server and the
  web service and asserts the two response bodies are deeply equal
  *modulo* the `id` field (each side reports its own adapter id).
- [ ] `dbboard-web/docs/api-contract.md` is updated to mirror the
  desktop additions at `1c350f6` (or the equivalent "pointer + delta"
  if the web copy is a pointer).

## Tech recommendations (non-binding for web)

- **Adapter trait shape**: NestJS provider with a `getCapabilities():
  Capabilities` method on the same interface that already implements
  `listTables` / `query`. The desktop trait uses
  `Option<&dyn Capability>` accessors per feature; in TypeScript the
  equivalent is per-feature method-returns-`undefined` (or a discriminated
  capability registry). Pick whichever is idiomatic — the wire shape is
  what matters.
- **Response DTO**: a single `class CapabilitiesResponseDto` with
  `@Expose()`d `id: string` and `capabilities: CapabilitiesDto`. Mark
  unknown flags as opt-in via class-transformer's `excludeExtraneousValues`
  so the additive contract doesn't accidentally leak server-internal
  fields.
- **404 ergonomics**: when implementing the *first* feature endpoint
  (likely `/views` for Postgres), centralise the "feature not
  available" branch through a single Nest `HttpException` subclass so
  every capability-gated endpoint returns the identical envelope.

## Handoff procedure

1. Push the four pending desktop commits
   (`0dc9e17..1c350f6`, plus this issue's docs commit) so
   `dbboard-web` planners can read the contract at a stable tag.
2. In `dbboard-web`, open this issue (or its GitHub equivalent), link
   back to this file and to `dbboard@1c350f6:docs/api-contract.md`.
3. Set this file's status to `in-progress` once web work starts, then
   `done` (with the web PR link) when acceptance criteria are met.

## Notes

- The contract was extended on 2026-05-27 with `GET /capabilities`, the
  `Capabilities` shape, and the `capability` error category. Make sure
  the snapshot includes all three.
- Phase 2 is additive on purpose so the web side never has to break
  Phase 1 clients to mirror it. If a future capability requires a
  breaking change to an existing endpoint, that goes through
  `docs/decisions.md` in both repos per ADR-0004.
- If web discovers an ambiguity (e.g. "what id does a libSQL/SQLite
  adapter report when it isn't strictly Turso?"), file it back as a
  desktop-side ADR ticket rather than diverging silently.
