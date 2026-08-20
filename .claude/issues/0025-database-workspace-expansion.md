# 0025: Database Workspace expansion — review of the 2026-08-17 plan

- **Status**: open — review only; nothing is scheduled by this file
- **Opened**: 2026-08-17
- **Source**: [`.claude/plans/2026-08-17-database-workspace.md`](../plans/2026-08-17-database-workspace.md)
  (verbatim, Japanese)
- **Owner**: maintainer decides what, if anything, moves to `docs/roadmap.md`
- **Related**: ADR-0011 (contract = the SemVer public API), ADR-0036 (hand-rolled
  SigV4, no AWS SDK), ADR-0049 / ADR-0051 (in-process dump / restore),
  ADR-0087 (per-connection write gate), ADR-0089 (egui client deleted),
  issue 0021 (the v1.0 gates)

## What the plan is

A ten-year product vision: extend dbboard from "a client that connects to many
databases" into a **local-first Database Workspace** covering native per-engine
features, scheduling, backup with verification, object storage, load testing,
migration rehearsal with rollback verification, and replication topology — all
reachable from GUI, AI and MCP alike.

The philosophy section (§30) is the strongest part and is worth adopting on its
own, independent of any feature below it:

- separate **執行成功** from **検証成功** — a restore that exited 0 is not a
  restore that was verified
- separate **AI inference** from **measured fact**
- build a place where you can fail before production, repeatedly

Those three already describe how this repo works (engine-enforced read-only,
`api_contract_drift.rs`, the verification sheets). Writing them down as product
philosophy costs nothing and makes future feature arguments shorter.

## The one structural finding

**The plan quietly changes who dbboard is for.**

dbboard reaches a database through a SQL or HTTP connection and nothing else.
Everything in §8 (CPU / Memory / Disk I/O / Network), §18 and §19 requires
host-level observability, which that connection cannot provide. Against the
nine adapters that ship today:

| adapter | host metrics | replication state | server-side backup |
|---|---|---|---|
| Turso / D1 | no — HTTP API only | n/a | no |
| Neon / Supabase / Aurora DSQL | no host access | none exposed | `pg_dump` over the network |
| CockroachDB | partial via `crdb_internal` | yes | |
| MySQL / MariaDB (self-hosted) | partial via `performance_schema`, full only over SSH | `SHOW REPLICA STATUS`, needs privilege | `mysqldump` |
| Firestore | no | n/a | no |
| MongoDB | no | `rs.status()` | |

So the monitoring / topology half of the plan lands **only on databases the user
runs themselves**. The current adapter lineup is the opposite: seven of the nine
are managed or serverless. That is not a reason to reject the plan — it is a
reason to make the choice consciously now rather than discover it in Phase 5
with an empty graph on screen.

Two honest ways to take it:

1. **Add the self-host operator as a second audience.** Accept that topology and
   resource graphs are MySQL / PostgreSQL / MongoDB features and render "not
   available on this connection" everywhere else, driven by capability flags.
2. **Drop host metrics; keep DB-visible metrics.** TPS, latency percentiles,
   row counts, lag, and query-level timing are all obtainable through the
   connection on most engines. This keeps one audience and loses the CPU/disk
   charts.

Either is defensible. Shipping a screen that is blank for seven of nine
adapters is not.

## Concrete conflicts with decisions already made

**§6 external dump tools.** The plan names `mysqldump` / `pg_dump` /
`pg_restore`. Today's backup (ADR-0049) and restore (ADR-0051) are deliberately
**in-process** — that is why the collector handoff was one `.exe` with nothing
to install. Shelling out reintroduces "have the matching client version
installed", exactly what was removed. It is still worth doing for fidelity
(`pg_dump` preserves things the in-process dump does not), but as an **opt-in
second path**, not a replacement, and it needs its own ADR.

**§7 S3 / R2 / MinIO is cheaper than it looks.** ADR-0036 hand-rolled SigV4 to
avoid the AWS SDK and keep the rustls-ring posture. That signer lives in
`crates/dbboard-postgres/src/dsql_auth.rs` and S3 object operations use the same
algorithm. This can be built by lifting SigV4 into a shared crate rather than
pulling `aws-sdk-s3`. Worth flagging as a positive: the expensive dependency
decision was already taken, in the right direction.

**§5 Scheduler cannot work as written.** "Native if available, dbboard
automation if not" — the *if not* branch only runs while the desktop app is
open. A scheduled backup that silently does not run is worse than no scheduled
backup. Viable shapes: native only (`EVENT` / `pg_cron`), or a headless dbboard
subcommand invoked by Task Scheduler / launchd / systemd timer, reusing the
existing dump path. A resident daemon contradicts the local-first desktop
posture and should be last.

**§25 approval should not be a new mechanism.** ADR-0087 already gates writes
per connection (`mcp_write`), and adapters enforce read-only at the engine, not
by string matching. Migration, restore and rollback are destructive writes and
belong behind that same gate, extended rather than duplicated.

**§28 Phase 1 "Adapter Capability API" already exists.** `Capabilities` is in
`crates/dbboard-core/src/capabilities/mod.rs` with ten flags, and it is part of
`docs/api-contract.md` — the SemVer public API (ADR-0011). Adding
`supports_replication` and friends is *additive*, so it cannot force a 2.0. But
each addition must update the contract document, survive
`crates/dbboard-connect/tests/api_contract_drift.rs`, and be mirrored to
`dbboard-web`. **That mirror is v1.0 gate 2 and is still open.** Every capability
flag this plan adds makes that gate more expensive the longer it stays open.

**§22 Activity Timeline has no substrate left.** `history.jsonl` v:2 — the
`kind: "query" | "ai"` log built in ADR-0027 — went away with the egui client in
ADR-0089. What remains in the repo is `default_history_path()` and its two
tests; **nothing writes or reads that file.** The Tauri client keeps query
history in `localStorage`, per connection, capped at 50, with no AI, backup or
MCP events in it. So the timeline is not an extension of something that works —
it is a gap. That makes it more attractive rather than less: it is the one place
where GUI, MCP and automation would all write to the same record.

## What is a product of its own, not a feature

**Cross-engine migration (§4 translator, MySQL → PostgreSQL).** pgloader and AWS
DMS are entire products, and the type/DDL/procedure translation is where the
years go.

The genuinely differentiating idea is not the migration — it is the **rehearsal
loop**: baseline → backup → migrate → validate → benchmark → rollback → verify
the rollback. That loop can be proven on **same-engine** migration first (version
upgrade, host move, managed → self-hosted), which is both the commoner real case
and exercises every step without a translator. Cross-engine then becomes a
translator bolted onto a loop that already works, instead of a prerequisite for
finding out whether the loop is any good.

## Sizing

Eight phases at this breadth is multiple years for one maintainer plus an agent.
The realistic value of the document right now is not as a schedule but as a
**selection filter** — when the next feature request arrives, it says whether the
request is on the trunk or off it.

It should therefore be kept as a vision document (where it now is) and **not**
merged into `docs/roadmap.md`, which tracks what is actually being built.

## Relationship to v1.0

Nothing in this plan touches `docs/api-contract.md` in a breaking way. New
endpoints and new capability flags are additive (ADR-0011), so **all of it is
1.x work and none of it is a v1.0 gate**. The three open gates in issue 0021
stand unchanged, and this plan is an argument for closing them sooner: the
longer the `dbboard-web` mirror stays open, the more contract surface it has to
catch up on.

## Smallest slice worth building first, if any

Three candidates, ordered by value-per-cost. Each is useful alone and none
presumes the rest of the plan.

1. **Backup verification** — attach SHA-256, a manifest, schema fingerprint and
   row counts to the existing ADR-0049 dump, and show "backup completed" and
   "backup verified" as two separate results. Delivers §30's central idea, adds
   no subsystem, and directly serves the deployment where someone other than the
   maintainer runs the backups.
2. **Activity log** — one durable append-only record written by the GUI, the MCP
   server and any automation, replacing the localStorage query history and
   reviving what ADR-0089 removed. Prerequisite for §13, §22 and §23, and useful
   on its own the first time someone asks "what did the agent run last night?".
3. **Same-engine migration rehearsal on MySQL** — MySQL is the adapter actually
   in use, and dump plus restore already exist. This proves the whole loop end
   to end against a Docker target before anything cross-engine is attempted.

Item 1 is small enough to be a single PR. Item 2 needs an ADR (record schema,
location, retention, redaction). Item 3 needs 1 and 2 to be worth anything.

## Not decided here

Whether to build any of it, and in what order. This file records what the plan
costs and what it collides with; the pick is the maintainer's.
