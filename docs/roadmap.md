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
- **Right now (2026-05-26)**: `desktop` Phases 1 / 1.5 / 1.6 / 1.7 have
  shipped and the workspace is at `0.1.0` (per ADR-0011). `web` has
  closed its Phase 1: the pnpm + Nuxt 4 + NestJS 11 monorepo scaffold
  with a `GET /health` smoke is on `develop`, and the contract is
  byte-content-mirrored at `dbboard@89b7c70`. The baton is back on
  `desktop` for Phase 2 (adapter trait + capability model +
  `GET /capabilities`). When `/capabilities` lands, the desktop side
  amends `docs/api-contract.md` and emits a handoff brief in the
  format of `939fe22` so the web side can re-sync and pick up its
  queued issues `0003` (NestJS HTTP surface), `0004` (Postgres
  adapter), `0005` (row cap + body limit + conformance tests).
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

- [x] Define `DatabaseAdapter` trait in `dbboard-core` *(ADR-0012)*
- [x] Move Turso-specific types behind the trait
- [x] Connection management UI (add / edit / delete) *(ADR-0016, Stage 1;
  HeidiSQL multi-process model — the running process keeps talking to
  its launch-time connection and the window mutates the persisted
  store. `ConnectionAdmin` use case in `dbboard-config`,
  `ConnectionsView` egui surface, all 11 locales translated.)*
- [x] Local config file (TOML) + OS keychain for secrets *(ADR-0013;
  `connections.toml` resolved via `directories`, secrets via the
  `keyring` crate behind a `SecretStore` trait; see
  [`docs/connections.md`](connections.md))*
- [x] Query history — in-memory (ADR-0014, Stage 1)
- [x] Query history — persistent JSON Lines (ADR-0017, Stage 2;
  `history.jsonl` next to `connections.toml`, shared record schema
  with `dbboard-web` per the cross-repo brief in
  `.claude/issues/0003-web-history-schema-mirror.md`)
- [x] In-process connection switching (ADR-0020; per-row **Connect**
  button on the connection list swaps the active adapter on the
  running server via `Arc<RwLock<Arc<dyn DatabaseAdapter>>>`. Each
  HTTP handler snapshots the adapter once at request start, so
  in-flight requests complete on the old adapter and new requests
  pick up the new one. Lifts the HeidiSQL multi-process limitation
  noted under ADR-0016 — a single desktop process can now drive
  many connections in one session.)

Exit criteria: nothing in `dbboard-ui` knows the word "Turso".

## Phase 2.5 — Multilingual UI (ADR-0015) ✅ done

Goal: ship the desktop UI in 11 locales (en, ja, ko, zh-CN, zh-TW, de,
fr, es, pt-BR, ru, it) without changing the HTTP contract or any
server-emitted text.

- [x] ADR-0015 — locale set, framework choice (fluent-rs over gettext),
  resolution chain (`DBBOARD_LANG` → OS → `en`), font strategy, scope rule
- [x] `crates/dbboard-i18n` — embedded Fluent resources, runtime
  `t!()` / `t_args!()` macros, OS locale detection via `sys-locale`
- [x] 11 `.ftl` resource files covering every UI string
- [x] `dbboard-ui` translates labels through the macros; `DbError`
  variants stay English on the wire but the UI prefixes a translated
  category label
- [x] `apps/dbboard` resolves the locale at startup and registers an
  OS-installed CJK font so `ja` / `ko` / `zh` users do not render tofu
- [x] Runtime locale switcher (ADR-0022; Language / 言語 submenu in
  the menu bar lists all 11 locales by native name and swaps the
  active Fluent bundle in place. `DBBOARD_LANG` still wins at
  startup; the switcher only mutates the current session. Closes the
  "shipped 11 locales but no switcher" gap ADR-0015 left open.)

Exit criteria: `DBBOARD_LANG=<tag>` switches every UI label to that
locale at startup; the menu-bar Language submenu (ADR-0022) switches
it at runtime in the running session; `DbError` body text stays English
(ADR-0009 HTTP contract); a malformed override falls back to the OS
locale; an unknown locale falls back to `en` without aborting.

## Phase 3 — Neon, Supabase, and Aurora DSQL adapters ✅ done (2026-06-04)

Goal: prove the trait by adding three more adapters without changing
the UI or the core.

- [x] Neon via the shared `dbboard-postgres` adapter (ADR-0018: flavored
  first-class kind. `PostgresAdapter::connect_neon` returns the same
  adapter but with `id() == "neon"`; new `DBBOARD_NEON_URL` env var
  ranks above `DBBOARD_PG_URL`; `ConnectionKind::Neon` is an additive
  v=1 variant in `connections.toml`; UI Add form lists "Neon" alongside
  the three existing kinds. Live test gated on `DBBOARD_NEON_URL`.)
- [x] Connection picker recognises adapter kind (delivered by ADR-0018
  alongside the Neon flavor; ADR-0019 / ADR-0021 extend the same
  machinery to Supabase and Aurora DSQL)
- [x] Supabase via the shared `dbboard-postgres` adapter (ADR-0019:
  second flavored first-class kind. `PostgresAdapter::connect_supabase`
  returns the same adapter with `id() == "supabase"`; new
  `DBBOARD_SUPABASE_URL` env var ranks between Neon and PG;
  `ConnectionKind::Supabase` is an additive v=1 variant. Both the
  direct `:5432` and pooler `:6543` endpoints fit the same kind — the
  URL itself picks. Live test gated on `DBBOARD_SUPABASE_URL`. REST
  hybrid deliberately deferred to a future ADR.)
- [x] AWS Aurora DSQL via the shared `dbboard-postgres` adapter
  (ADR-0021: third flavored first-class kind.
  `PostgresAdapter::connect_aurora_dsql` returns the same adapter with
  `id() == "aurora-dsql"`; new `DBBOARD_AURORA_DSQL_URL` env var ranks
  alphabetically first among the pg-wire flavors (above Neon, Supabase,
  and PG); `ConnectionKind::AuroraDsql` is an additive v=1 variant
  serialized as the kebab-case `kind = "aurora-dsql"`. The URL's
  password segment must carry a short-lived IAM authentication token
  (~15 min TTL); SDK-driven auto-refresh is deliberately deferred to a
  future ADR. Live test gated on `DBBOARD_AURORA_DSQL_URL`.)
- [x] Adapter-specific quirks documented in each crate's README

Exit criteria met: a user can switch between Neon, Supabase, Aurora
DSQL, and a generic Postgres / Cockroach connection in one session
without restarting the app (the in-process swap mechanism is delivered
by ADR-0020 under Phase 2), with each labelled distinctly in the
connection picker and history.

## Phase 4 — AI integration (optional layer)

Goal: ship the optional AI plugin layer behind a trait. Default builds
work without it. Trait + first-provider shape locked in
[ADR-0023](decisions.md); implementation tracked in
`.claude/issues/0005-dbboard-ai-trait-and-anthropic-provider.md`.

- [x] `dbboard-ai` crate with `AiProvider` trait (ADR-0023; trait
      crate landed via PR #20 on 2026-06-15 — `AiProvider` /
      `AiCapabilities` / `ExplainRequest` / `SuggestRequest` /
      `AiResponse` / `AiError`, 15 unit tests, no runtime I/O)
- [x] First provider: Claude (Anthropic API) — `dbboard-anthropic`
      crate (ADR-0023; landed via PR #22 on 2026-06-15 — `reqwest`
      against `POST /v1/messages`, `explain` / `suggest_sql`,
      construction-time key/model validation, redacted `Debug`,
      24 unit + 7 wiremock round-trip tests, no live network.)
- [x] `apps/dbboard` env-var wiring (ADR-0023; landed via PR #24
      on 2026-06-17 — `DBBOARD_ANTHROPIC_API_KEY` (required gate) +
      optional `DBBOARD_ANTHROPIC_MODEL` (default `claude-sonnet-4-6`)
      resolved at startup, `Option<Arc<dyn AiProvider>>` injected
      into `DbboardApp::connect`, `has_ai_provider()` accessor for
      the slice (b) panel to gate registration. README "AI
      integration (optional)" subsection added.)
- [ ] "Explain this query" command — _slice (b) of issue 0005,
      open against the dbboard-ui AI panel + worker round-trip_
- [ ] "Suggest SQL from prompt" command using current schema snapshot
      (`list_tables` result; full DDL extraction deferred) — _slice
      (b) of issue 0005_
- [ ] Settings UI for API key, provider choice — _Stage 2 ADR;
      env var `DBBOARD_ANTHROPIC_API_KEY` covers Stage 1 (PR #24)_
- [ ] Graceful degradation when no provider configured — _wiring
      half landed via PR #24 (`has_ai_provider()` returns false when
      env unset); the panel that hides on `false` follows in
      slice (b)_

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
