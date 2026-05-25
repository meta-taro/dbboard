# Roadmap

This is the **desktop** dbboard roadmap. The web sibling
([`dbboard-web`](https://github.com/meta-taro/dbboard-web)) has its own
roadmap; the two are coordinated at the concept level only.

Mark phases `✅ done` as they ship. Add concrete dates only after the
fact; estimates belong in the issue tracker, not here.

## Pacing Note

Two repos are maintained in parallel by a small team. To avoid splitting
focus:

- **Default**: alternate sprints between desktop and web, not concurrent
  work on the same layer in both.
- **Right now (2026-05-25)**: `desktop` Phases 1 / 1.5 / 1.6 / 1.7 have
  shipped and the workspace is tagged `0.1.0` (per ADR-0011). `web` is
  next up — the HTTP contract in `docs/api-contract.md` (including the
  10,000-row cap added during Phase 1.7 closeout) is the input. Once
  `web` mirrors the contract, the desktop side begins Phase 2 (adapter
  trait + capability model).
- **Exception**: contract changes (endpoint shapes, error categories,
  schema metadata) are drafted in one repo, mirrored in the other
  immediately, and only then built against.
- New DB adapter feature parity is not required at every step. The
  desktop repo ships an adapter first, then the web repo follows when
  it makes sense.

## Phase 1 — Turso vertical slice ✅ done (2026-05-25)

Goal: prove the full path "connect → introspect → query → render" end
to end against a single database before generalising.

- [x] Workspace skeleton (`dbboard-core`, `apps/dbboard`)
- [x] Add `dbboard-turso` crate
- [x] Hard-coded Turso connection from env or local file
- [x] Run `SELECT` and render a result table in egui
- [x] List tables in a sidebar
- [x] Error surface (connection failure, query failure)

Exit criteria met: `cargo run -p dbboard` against a local libSQL file
browses tables, runs queries, and renders results with errors surfaced
inline. Tagged at workspace `0.1.0` (per ADR-0011).

## Phase 1.5 — Local HTTP backend (ADR-0006, ADR-0009) ✅ done (2026-05-23)

Goal: introduce the `dbboard-server` crate behind the UI without
changing what the user can do.

- [x] Draft initial API contract (endpoint paths, request and response
  shapes, error categories) — recorded at
  [`docs/api-contract.md`](api-contract.md) as the canonical source
  (ADR-0009)
- [ ] Mirror the draft contract to `dbboard-web` *(human-owned;
  alternating-repo step per the Pacing Note)*
- [x] Add `crates/dbboard-server` (axum) implementing the contract
  against all three adapters (Turso / D1 / Postgres)
- [x] Auto-port loopback bind in `apps/dbboard`; pass port to the UI
- [x] Convert `dbboard-ui` from direct adapter calls to HTTP client
  (worker now drives `reqwest`; `Command`/`Reply` channels retained)
- [x] Integration tests against the local server (`tower::oneshot`
  in-process plus one real loopback round-trip; Turso `:memory:`)

Exit criteria: `cargo run -p dbboard` still does what Phase 1 did,
but every action now traverses HTTP and the same endpoints are
documented in both repos.

## Phase 1.6 — Cloudflare D1 adapter (REST) ✅ done

Goal: add a second concrete adapter against Cloudflare D1 over its REST
API, ahead of the trait extraction, to give Phase 2 a real second shape
(ADR-0007). UI and core are unchanged.

- [x] Add `crates/dbboard-d1` (`reqwest` + `rustls`, `/raw` endpoint)
- [x] `connect` / `ping` / `list_tables` / `query` mirroring the Turso
  adapter's surface
- [x] Env-driven backend selection in `apps/dbboard`
  (`DBBOARD_D1_ACCOUNT_ID` / `DBBOARD_D1_DATABASE_ID` /
  `DBBOARD_D1_TOKEN`, optional `DBBOARD_D1_BASE_URL`); falls back to
  local Turso when unset
- [x] Unit tests for envelope/value mapping; live round-trip test gated
  behind `DBBOARD_D1_*`

Exit criteria: with the `DBBOARD_D1_*` env vars set, `cargo run -p
dbboard` browses tables and runs queries against a real D1 database;
with them unset it still defaults to local Turso.

## Phase 1.7 — CockroachDB via shared `dbboard-postgres` adapter ✅ done

Goal: add a third concrete adapter for PostgreSQL-wire databases, with
CockroachDB as the first target, ahead of the trait extraction. This is
the first non-SQLite adapter (schemas, typed columns, connection pool),
giving Phase 2 a genuinely different shape (ADR-0008). UI and core are
unchanged.

- [x] Add generic `crates/dbboard-postgres` (`sqlx` + `tls-rustls-ring`)
- [x] `connect` / `ping` / `list_tables` / `query` mirroring the existing
  adapter surface
- [x] Dynamic decoding via the simple query protocol (`sqlx::raw_sql`):
  every value read as text → `Value::Text`, NULL → `Value::Null`; no
  `dbboard-core` change
- [x] Schema-qualified introspection via `information_schema.tables`
  (excludes `pg_catalog` / `information_schema` / `crdb_internal`)
- [x] Single-connection-string backend selection in `apps/dbboard`
  (`DBBOARD_PG_URL`, highest precedence); falls back to D1 then local
  Turso when unset
- [x] Unit tests for error classification / introspection mapping; live
  round-trip test gated behind `DBBOARD_PG_URL`

Exit criteria: with `DBBOARD_PG_URL` set to a CockroachDB connection
string, `cargo run -p dbboard` browses `schema.table` listings and runs
queries against a real CockroachDB database; with it unset the app still
defaults to D1 (if configured) or local Turso.

## Phase 2 — Extract the adapter trait *(current)*

Goal: turn the Turso-shaped types into a real abstraction without
breaking Phase 1. Designed jointly with the capability model (ADR-0012)
so per-DB features can be added later without breaking the HTTP
contract (ADR-0011).

- [ ] Define `DatabaseAdapter` trait in `dbboard-core`
- [ ] Move Turso-specific types behind the trait
- [ ] Connection management UI (add / edit / delete)
- [ ] Local config file (TOML) + OS keychain for secrets
- [ ] Query history (in-memory, then persisted)

Exit criteria: nothing in `dbboard-ui` knows the word "Turso".

## Phase 3 — Neon and Supabase adapters

Goal: prove the trait by adding two more adapters without changing the
UI or the core.

- [ ] Neon via the shared `dbboard-postgres` adapter (it is Postgres-wire;
  Phase 1.7 already covers the SQL path — this step is mostly the
  connection picker and any Neon-specific quirks)
- [ ] `dbboard-supabase` (REST + sqlx hybrid)
- [ ] Connection picker recognises adapter kind
- [ ] Adapter-specific quirks documented in each crate's README

Exit criteria: a user can switch between three live connections in one
session without restarting the app.

## Phase 4 — AI integration (optional layer)

Goal: ship the optional AI plugin layer behind a trait. Default builds
work without it.

- [ ] `dbboard-ai` crate with `AiProvider` trait
- [ ] First provider: Claude (Anthropic API)
- [ ] "Explain this query" command
- [ ] "Suggest SQL from prompt" command using current schema snapshot
- [ ] Settings UI for API key, provider choice
- [ ] Graceful degradation when no provider configured

Exit criteria: AI panel is hidden cleanly when not configured; visible
and usable when it is.

## Phase 5 — Quality of life

- [ ] Result table virtualisation for large result sets
- [ ] Export results (CSV / JSON)
- [ ] Saved queries
- [ ] Schema diff between two connections
- [ ] Performance: cold-start under 1s on a modern laptop

## Phase 6+ — Stretch

- Additional adapters (PlanetScale, MongoDB)
- Advanced schema visualisation
- Query performance analysis tools
- Plugin system for community extensions
- Agent-based AI workflows

## Out of Scope (for now)

- Mobile clients (the web repo's mobile-friendly UI covers this for now)
- Cloud sync of connections across machines
- Multi-user / sharing features
