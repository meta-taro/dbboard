# 0007: dbboard-web Phase 6 AI provider — no HTTP contract mirror needed (Phase 4 Stage 1 follow-up)

- **Status**: open (no-op brief — same shape as
  [`0006-web-aurora-dsql-no-mirror`](0006-web-aurora-dsql-no-mirror.md);
  desktop's stance is "ship your own thing, no coordination required
  on the wire"; this file exists to *say so* explicitly so the web
  agent unblocks).
- **Phase**: web-side Phase 6 (AI provider interface);
  desktop-side equivalent: ADR-0023 (`dbboard-ai` provider trait +
  Anthropic as first provider), Stage 1 complete via desktop PRs
  #18 / #20 / #22 / #24 / #27 — closes desktop issue
  [`0005`](0005-dbboard-ai-trait-and-anthropic-provider.md).
- **Opened**: 2026-06-23
- **Owner**: human (cross-repo handoff)
- **Target repo**: <https://github.com/meta-taro/dbboard-web>
- **Follows**: [`0006-web-aurora-dsql-no-mirror`](0006-web-aurora-dsql-no-mirror.md)
  (sibling brief in this PR).
- **Anchors**:
  - Desktop ADR-0023 in [`docs/decisions.md`](../../docs/decisions.md)
    (search `## ADR-0023`), Decision 3 in particular ("In-process
    wiring, not HTTP-mediated") and Decision 9 (Stage 2 deferrals).
  - Reference implementation:
    - Trait crate `crates/dbboard-ai` (PR #20, desktop@`584348f`).
    - First concrete provider `crates/dbboard-anthropic` (PR #22,
      desktop@`c705918`).
    - `apps/dbboard` env-var wiring (PR #24, desktop@`6ad670d`).
    - `dbboard-ui` AI panel + worker dispatch + 11-locale Fluent +
      docs sweep (PR #27, desktop@`c86424a`).
  - Web-side trigger: `dbboard-web/.claude/roadmap.md` Phase 6
    ("Optional AI provider interface") and its DoD bullet
    "**API-contract alignment on AI shapes**" with the desktop
    client.

## TL;DR

**Desktop's Phase 4 Stage 1 keeps AI off the HTTP contract entirely.**
ADR-0023 Decision 3 explicitly chose **in-process wiring, not
HTTP-mediated** for the AI provider — the same precedent set by
ADR-0020 (`swap_backend`) and ADR-0022 (`set_language`): mutate the
running desktop process directly when no wire contract is involved.
AI calls do **not** go through `dbboard-server`'s HTTP surface.

That means the web Phase 6 DoD bullet "API-contract alignment on AI
shapes" has **nothing to align with on the desktop side** for Stage 1.
`docs/api-contract.md` is untouched by ADR-0023. There is no
`POST /ai/explain`, no `POST /ai/suggest`, no `AiResponse` DTO on
the wire, no AI error category in the HTTP envelope. The contract
remains silent on AI.

**Practical consequence for web Phase 6**: build whatever AI module
shape fits NestJS-flavored cleanly. Take inspiration from desktop's
`AiProvider` trait shape (`explain` / `suggest_sql` / `id` /
`capabilities`) if useful, but treat it as a *peer design reference*,
not as a *contract mirror*. There is no breakage if web's request /
response DTOs diverge from desktop's `ExplainRequest` /
`SuggestRequest` / `AiResponse` Rust types — they aren't on the same
wire.

## What this is — and what it isn't

This is the second **explicit no-op brief** in the
[`0006`](0006-web-aurora-dsql-no-mirror.md) family. The lesson from
ticket `0010` being blocked for three weeks is that "implicit no-op"
leaves the other side guessing; this brief makes the no-op
**explicit** so web can move.

It is **not**:
- A request to ship the Anthropic provider on web. Web's stack is
  Node / NestJS; the equivalent move is `@anthropic-ai/sdk` (the
  official Node SDK) wrapped in a NestJS module under
  `apps/api/src/modules/ai/`. Desktop has no opinion on that crate
  vs. raw `fetch` vs. another wrapper — that's web's call.
- A request to use the same env var names. Desktop uses
  `DBBOARD_ANTHROPIC_API_KEY` and `DBBOARD_ANTHROPIC_MODEL`. Web
  may reuse those (cleanest for a maintainer running both side by
  side) or pick its own. Both repos already use the `DBBOARD_*`
  prefix for env vars (`DBBOARD_API_SECRET`, `DBBOARD_BIND_HOST`,
  `DBBOARD_PG_URL`, etc.), so reuse is the natural default.
- A request to delay web Phase 6 until desktop Stage 2 lands. If
  web wants to ship Phase 6 today on the same Stage 1 footing,
  go ahead — provider trait + at least one adapter + graceful
  degradation when the env var is absent. The desktop Stage 2 ADR
  *may* eventually surface a wire-level coordination point
  (Settings sync? remote provider via `/ai/*`?), at which point a
  fresh outbound brief in this `0NNN-web-*` sequence will land.

## What desktop is asking web to do

1. **Treat web Phase 6's "API-contract alignment on AI shapes" DoD
   bullet as already satisfied** — there is no contract surface to
   align with. Either edit the DoD bullet to read
   "API-contract alignment on AI shapes (no wire surface needed —
   see desktop `0007-web-ai-phase6-no-contract-mirror`)" or drop
   the bullet entirely.
2. **If web ships Phase 6 in the near term, mirror desktop's
   *patterns* (not contract)**:
   - **Provider trait shape**: desktop's `AiProvider` has `id()` /
     `capabilities()` / `explain()` / `suggest_sql()`. Web's port
     can mirror this (with TS-flavored type signatures) so the
     conceptual surfaces are recognisable across repos. Optional
     but recommended.
   - **Env-var-only Stage 1**: desktop deliberately deferred
     persisted-key storage (Settings UI + keychain + `ai-providers.toml`)
     to a Stage 2 ADR. Web should follow suit for symmetry —
     env-var-only first, persisted store later. Reduces the
     attack surface for the first ship and avoids a premature
     web/desktop key-format coordination.
   - **Graceful degradation = absence of the UI**: desktop hides
     the AI panel entirely when no provider is configured (no
     greyed-out button, no "AI unavailable" stub). Web's `AiPanel`
     equivalent should do the same — the absence of the env var
     means "no AI in this deployment", not "AI is broken".
   - **Capability flags default-false**: desktop's `AiCapabilities`
     struct is flat-bool, all-false default. Streaming and
     function-calling are off in Stage 1 even though the Anthropic
     API supports them, because the UI side isn't wired for them
     yet. Web can adopt the same posture.
3. **Open a web-side Phase 6 ticket** when ready (`.claude/issues/0NNN-*`),
   citing this brief by anchor in its "Anchors" / "Cross-repo" section.
   Same pattern web already uses for ADR-0017 (the
   2026-06-05 web ADR cites desktop ADR-0017 by anchor without
   restating the schema). Don't restate ADR-0023 — cite it.
4. **Update web's ADR log** (`.claude/decisions.md`) with a short
   entry — for example
   `2026-06-23 — AI provider interface: no HTTP contract mirror needed (desktop ADR-0023 Stage 1)`.

## What desktop is NOT asking web to do

- **Do not block on desktop Stage 2.** Stage 2 deferrals (Settings
  UI, persisted keychain, streaming, multi-provider switcher, DDL
  extraction, function-calling, AI history records) are queued
  on the desktop side and may or may not produce wire-level
  coordination. Treat them as "future briefs may arrive in this
  sequence" — don't pre-design around hypothetical contract surfaces.
- **Do not ship the same provider crate.** Desktop has `dbboard-anthropic`
  (a Rust crate); web should use whatever Node-side approach fits
  NestJS (`@anthropic-ai/sdk` is the obvious pick). The trait shape
  is the design reference; the implementation is independent.
- **Do not invent an HTTP route to mirror desktop's in-process AI calls.**
  ADR-0023 Decision 3 explicitly rejected this for desktop because
  the HTTP contract is the shared surface and inflating it with
  AI mirror routes would buy zero parity. The same reasoning applies
  in the opposite direction — if web ships `/ai/explain` and
  `/ai/suggest`, those are **web-only** endpoints not shared with
  desktop, the same way `/connections/*` (web PR #7) is web-only.
- **Do not record AI calls in `history.jsonl`.** Desktop deferred
  this to Stage 2 (ADR-0023 §9). If web ships Phase 6 first and
  records AI interactions in history, that creates a unilateral
  schema bump pressure on `history.jsonl` ahead of any v:2
  coordination — **do not do this without a fresh brief.** The
  v:1 per-record schema (ADR-0017) is locked until both repos
  coordinate a v:2 bump.

## Future drift signal

Desktop will issue a fresh outbound brief in this sequence when (if)
Stage 2 surfaces any of the following on the HTTP contract:

- A `POST /ai/*` route (e.g. for a "remote AI proxy" deployment).
- A new error category in `docs/api-contract.md` for AI failures
  surfacing through HTTP.
- An `Capabilities` field flagging server-side AI availability for
  remote clients.
- An `ai-providers.toml` schema that web should also load (very
  unlikely — keychain integration is desktop-shape only).
- AI calls recorded in `history.jsonl` (would bump per-record
  schema to v:2 and require a separate `0NNN-web-*` brief).

Until then, **web Phase 6 can ship without coordinating with desktop
on the wire**.

## Acceptance

- [x] Desktop side ships this brief into the cross-repo handoff path
      (this PR — same as the sibling
      [`0006-web-aurora-dsql-no-mirror`](0006-web-aurora-dsql-no-mirror.md)).
- [ ] Web side edits or drops the Phase 6 DoD bullet
      "API-contract alignment on AI shapes" (no wire surface to
      align with on Stage 1).
- [ ] Web side adds an entry to its `.claude/decisions.md` citing
      desktop ADR-0023 by anchor (not by restating Decision 3 / §9).
- [ ] When web is ready to ship Phase 6, opens a web-side
      `.claude/issues/0NNN-*` ticket citing this brief.

## Notes

- Numbering: `0007` follows `0006` (the Aurora DSQL no-op brief in
  the same desktop PR). Both briefs are explicit no-ops; both unblock
  a long-standing web "waiting on desktop" flag.
- SemVer impact (ADR-0011): **none** — there is no contract change.
  Same reasoning as `0006-web-aurora-dsql-no-mirror`.
- This brief deliberately does **not** prescribe a web-side trait
  shape, env var name set, NestJS module structure, or DI token
  layout. Those are web-shape decisions. The brief offers desktop's
  pattern as a *reference*, not a *contract*.
