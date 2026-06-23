# 0006: dbboard-web Aurora DSQL — no contract mirror needed (Phase 3 follow-up)

- **Status**: open (no-op brief — desktop's stance is "do nothing on the
  web side"; this file exists to *say so* explicitly so the web
  agent unblocks).
- **Phase**: web-side Phase 6 area (web ticket reservation: `0010`);
  desktop-side equivalent: ADR-0021 (Aurora DSQL as a flavored kind
  over `dbboard-postgres`), shipped via desktop PR #13.
- **Opened**: 2026-06-23
- **Owner**: human (cross-repo handoff)
- **Target repo**: <https://github.com/meta-taro/dbboard-web>
- **Follows**: [0003-web-history-schema-mirror](0003-web-history-schema-mirror.md)
  (last cross-repo outbound brief, 2026-06-04)
- **Anchors**:
  - Desktop ADR-0021 in [`docs/decisions.md`](../../docs/decisions.md)
    (search `## ADR-0021`).
  - Reference implementation: `crates/dbboard-postgres` `connect_aurora_dsql`
    + `FLAVOR_AURORA_DSQL` at desktop tip `cd41641` (PR #13 merged
    2026-06-04).
  - Web-side trigger: `dbboard-web/.claude/project-status.md` line
    listing **"Aurora DSQL adapter (`0010`)"** as "Blocked on a desktop
    handoff brief — check `../dbboard/.claude/issues/` for an `0010-*`
    (or similar) ticket". This file *is* that ticket; the desktop
    side simply numbered it `0006-web-aurora-dsql-no-mirror.md` to
    keep the outbound-brief sequence going (0001 / 0002 / 0003 / **0006**).

## TL;DR

**There is nothing for `dbboard-web` to mirror.** Desktop's ADR-0021
treats Aurora DSQL as a flavored kind over the existing Postgres
adapter — wire protocol, SQL surface, TLS, pool config, dynamic
decoding, and row cap are byte-identical to vanilla Postgres. From the
HTTP contract's point of view nothing happened. From web's point of
view: **the Postgres adapter shipped in ticket `0004` (web PR #9)
already handles Aurora DSQL URLs unchanged.**

The web side can therefore close ticket `0010` as **"no work required
on web side; superseded by `0004`"**, or simply mark it
"resolved by cross-repo coordination — see desktop `0006-web-aurora-dsql-no-mirror`."

## What this is — and what it isn't

Like `0003-web-history-schema-mirror`, this is **not** a wire-contract
mirror in the sense of issues `0001` / `0002`. The contract is
unchanged:

- `GET /capabilities` would report `"aurora-dsql"` as the adapter id
  if the desktop server were configured against an Aurora DSQL URL,
  but **that field's value is opaque to the contract** (ADR-0012
  treats `adapter` as a free-form identifier — the wire shape is fixed,
  the string contents are not).
- No new endpoint. No new error category. No new DTO. No new HTTP
  status code. `docs/api-contract.md` is **untouched** by ADR-0021.
- Per-record history JSON schema (ADR-0017) is also unchanged — the
  `conn` field records the connection id, not the adapter flavor.

What **is** unique to Aurora DSQL among the four pg-wire flavors
(Postgres / Neon / Supabase / Aurora DSQL) is the auth model:

- Aurora DSQL does **not** accept static passwords. The "password"
  segment of the connection URL must carry a short-lived **IAM
  authentication token** (~15 min TTL), generated either by the AWS
  CLI (`aws dsql generate-db-connect-admin-auth-token` /
  `generate-db-connect-auth-token`) or by an AWS SDK call
  (`@aws-sdk/dsql-signer` on the JS/TS side; the Rust analogue
  is `aws-sdk-dsql`).
- Desktop ADR-0021 ships **only the static-URL path** for Phase 3:
  the user pre-generates a token via the AWS CLI, pastes the
  `postgres://…` URL into dbboard, and re-pastes when the token
  expires. ADR-0021 §"Decision" path 1.
- SDK-driven token auto-refresh is **deliberately deferred to a future
  ADR**. ADR-0021 §"Decision" path 2: "Better UX but adds a multi-crate
  AWS SDK dependency (with its own TLS / async-runtime fingerprint) and
  is materially more work — exactly the kind of scope creep that
  ADR-0019 dodged by deferring the Supabase REST surface."

Crucially: **even the SDK-integrated path doesn't change the HTTP
contract.** It's an in-process credential helper. When desktop ships
it (some future ADR), the wire shape stays the same — only the
adapter's connect / ping path gains an interceptor that swaps the
URL's password segment for a fresh token. No new endpoint, no new DTO.
So even *that* future ADR will most likely be a "no mirror needed"
brief from desktop's side too — unless the IAM-auth-aware capability
flag (`has_iam_auth`, etc., ADR-0021 §"Decision" capability section)
crosses the contract, which would be additive and trivial to mirror.

## Why now (not earlier, not later)

- **Not earlier**: the desktop side only firmed up the "static URL,
  defer SDK" stance when ADR-0021 landed on 2026-06-04 and PR #13
  merged the implementation. Sending this brief before then would
  have been guessing at the auth model.
- **Not later**: the web side has been blocked on this brief for
  almost three weeks. Web's `project-status.md` explicitly flags
  ticket `0010` as "waiting on desktop". Continuing to leave that flag
  in place pollutes the web agent's mental model — it keeps treating
  Aurora DSQL as a coordination liability when it's actually
  resolved-by-being-no-op.

## What desktop is asking web to do

1. **Close web ticket `0010`** as resolved by cross-repo coordination.
   The technical claim: "Aurora DSQL URLs are pg-wire — they work
   through the existing Postgres adapter (web ticket `0004`, web
   PR #9). No web-side code change required."
2. **Document the IAM-token UX expectation** in web's connection docs
   (`docs/connections.md` if it exists, or whatever the equivalent
   is) so a user pasting an Aurora DSQL URL knows:
   - The URL must contain an IAM auth token in the password segment.
   - Tokens expire (~15 min); a stale token will surface as a
     connection error.
   - Generate fresh tokens via `aws dsql generate-db-connect-auth-token`
     or via `@aws-sdk/dsql-signer` in application code.
   - Web is free to ship a small "regenerate token" affordance (server-side
     module wrapping `@aws-sdk/dsql-signer`) at any time — this is a
     pure web-side decision, not a coordination point with desktop.
3. **Update web's ADR log** (`.claude/decisions.md`) with a short
   entry citing desktop ADR-0021 by anchor (same pattern web used for
   ADR-0017 in the 2026-06-05 entry) — for example
   `2026-06-23 — Aurora DSQL adapter: no mirror needed (desktop ADR-0021)`.

## What desktop is NOT asking web to do

- **Do not ship a separate `AuroraDsqlAdapter`** — pg-wire is pg-wire.
  The existing `PostgresAdapter` is correct.
- **Do not add a new HTTP endpoint** or DTO. The contract is unchanged.
- **Do not block web Phase 6 progress** on this. Web Phase 6 is the
  AI provider interface — *that* coordination is handled in a separate
  outbound brief: [`0007-web-ai-phase6-no-contract-mirror.md`](0007-web-ai-phase6-no-contract-mirror.md)
  (issued in the same desktop PR as this brief).
- **Do not mirror desktop's IAM-token-aware capability flags**
  (`has_iam_auth`, …). Those are deferred on desktop side and are
  application-layer concerns; when (if) they ever cross the contract,
  they'll arrive in their own brief.

## Future drift signal

If desktop ever ships path 2 (SDK-integrated token auto-refresh) AND
that ADR adds new capability flags or a new endpoint (e.g.
`POST /connections/:id/refresh-token`), that **will** generate a
fresh outbound brief in this 0NNN-web-* sequence. Until then, treat
Aurora DSQL as "a Postgres URL with a quirky password" on both sides.

## Acceptance

- [x] Desktop side ships this brief into the cross-repo handoff path
      (this PR).
- [ ] Web side closes ticket `0010` as resolved-by-no-op, citing this
      brief by anchor.
- [ ] Web side adds an entry to its `.claude/decisions.md` mirroring
      desktop ADR-0021 by reference (not by restating).
- [ ] Web side adds (or updates) connection docs to describe the
      IAM-token-in-URL expectation for Aurora DSQL.
- [ ] Web side removes the "Blocked on a desktop handoff brief" line
      for `0010` from its `project-status.md`.

## Notes

- This is the desktop side's first **explicit no-op brief**. The
  pattern was used implicitly before — ADR-0018 (Neon) and ADR-0019
  (Supabase) shipped without a `0NNN-web-*` brief precisely because
  they were also no-op from web's point of view. The lesson from the
  three-week blockage on ticket `0010` is that "implicit no-op" leaves
  the other side guessing; making it **explicit** unblocks the queue
  faster. Future no-op coordination should follow this pattern: a
  short outbound brief that says "no work required, here's why".
- Naming: `0006-web-aurora-dsql-no-mirror.md` over
  `0006-web-aurora-dsql-mirror.md` to make the "no" load-bearing in
  the filename. Web's `0010` referenced "an `0010-*` (or similar)
  ticket"; the "or similar" hedge is honoured — desktop uses its own
  sequence number (next slot = 0006), not the web one.
- SemVer impact (ADR-0011): **none** — there is no contract change.
