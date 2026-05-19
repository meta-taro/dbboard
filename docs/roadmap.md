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
- **Right now (2026-05-19)**: `desktop` Phase 1 runs first. `web` Phase
  1 (monorepo scaffold) is staged behind it so that the HTTP API
  contract (ADR-0004, ADR-0006) is shaped against a real, working
  desktop slice before web spends effort on a NestJS skeleton that
  would have to be revised.
- **Exception**: contract changes (endpoint shapes, error categories,
  schema metadata) are drafted in one repo, mirrored in the other
  immediately, and only then built against.
- New DB adapter feature parity is not required at every step. The
  desktop repo ships an adapter first, then the web repo follows when
  it makes sense.

## Phase 1 — Turso vertical slice  *(current)*

Goal: prove the full path "connect → introspect → query → render" end
to end against a single database before generalising.

- [ ] Workspace skeleton (`dbboard-core`, `apps/dbboard`)
- [ ] Add `dbboard-turso` crate
- [ ] Hard-coded Turso connection from env or local file
- [ ] Run `SELECT` and render a result table in egui
- [ ] List tables in a sidebar
- [ ] Error surface (connection failure, query failure)

Exit criteria: a developer can run `cargo run -p dbboard`, point at a
local libSQL file, browse tables, run queries, and see results.

## Phase 1.5 — Local HTTP backend (ADR-0006)

Goal: introduce the `dbboard-server` crate behind the UI without
changing what the user can do.

- [ ] Draft initial API contract (endpoint paths, request and response
  shapes, error categories) — record at `docs/api-contract.md`
- [ ] Mirror the draft contract to `dbboard-web`
- [ ] Add `crates/dbboard-server` (axum) implementing the contract
  against the Turso adapter
- [ ] Auto-port loopback bind in `apps/dbboard`; pass port to the UI
- [ ] Convert `dbboard-ui` from direct adapter calls to HTTP client
- [ ] Integration tests against the local server (no real DB needed
  for some, libSQL embedded for query tests)

Exit criteria: `cargo run -p dbboard` still does what Phase 1 did,
but every action now traverses HTTP and the same endpoints are
documented in both repos.

## Phase 2 — Extract the adapter trait

Goal: turn the Turso-shaped types into a real abstraction without
breaking Phase 1.

- [ ] Define `DatabaseAdapter` trait in `dbboard-core`
- [ ] Move Turso-specific types behind the trait
- [ ] Connection management UI (add / edit / delete)
- [ ] Local config file (TOML) + OS keychain for secrets
- [ ] Query history (in-memory, then persisted)

Exit criteria: nothing in `dbboard-ui` knows the word "Turso".

## Phase 3 — Neon and Supabase adapters

Goal: prove the trait by adding two more adapters without changing the
UI or the core.

- [ ] `dbboard-neon` (sqlx-postgres under the hood)
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
