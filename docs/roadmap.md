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
- **Right now (2026-07-22)**: `desktop` has shipped Phases 1 through 5
  and released **v0.3.0** — all six adapters (Turso, D1, CockroachDB,
  Neon, Supabase, Aurora DSQL), the optional AI assistant (Phase 4), and
  now a read-only MCP server (`dbboard-mcp`, ADR-0046). The workspace is
  at `0.3.0`; the tagged Release CI is proven green (see Phase 5). The
  `web` status below is **last-known as of the 2026-05-26 sync** and has
  not been re-verified this session (`dbboard-web` is a separate repo,
  not checked out here — only the HTTP contract and the history JSON
  schema are shared). As of that sync `web` had closed its Phase 1 (pnpm
  + Nuxt 4 + NestJS 11 monorepo scaffold with a `GET /health` smoke on
  `develop`, contract byte-content-mirrored at `dbboard@89b7c70`), with
  the baton back on `desktop`. No contract change in this v0.3.0 line
  needs a web mirror — the MCP server is desktop-local and does not
  touch the shared HTTP contract.
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
  [`docs/connections.md`](connections.md). At-rest hardening
  follow-up — ADR-0024 / PR #25 (2026-06-22): `0o600` on Unix
  via the new `dbboard_config::secure_fs` module, inherited DACL on
  Windows, and a startup stderr warning when the resolved config
  dir traverses a cloud-sync folder (OneDrive / iCloud Drive /
  Dropbox / Google Drive).)*
- [x] Query history — in-memory (ADR-0014, Stage 1)
- [x] Query history — persistent JSON Lines (ADR-0017, Stage 2;
  `history.jsonl` next to `connections.toml`, shared record schema
  with `dbboard-web` per the cross-repo brief in
  `.claude/issues/0003-web-history-schema-mirror.md`. At-rest
  posture tightened by ADR-0024 / PR #25 — `0o600` on Unix on first
  creation, defensively re-tightened on every append for files
  that pre-date the ADR.)
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
- [x] Aurora DSQL self-minted IAM tokens — `aurora-dsql-iam` kind
  ([ADR-0036](decisions.md), PR #56): dbboard mints the ~15-min SigV4
  IAM token itself from stored AWS credentials (hand-rolled SigV4, no
  AWS SDK, preserving the rustls-ring posture), so no hand-refresh. Only
  the AWS secret access key is a secret (keychain); access key id,
  endpoint, region, database, username live inline. 段階A minted once
  at build time with a **Reconnect** button as the stopgap.
- [x] Aurora DSQL in-pool token auto-refresh (段階B) —
  ([ADR-0037](decisions.md), PR #61): a timer-based pool-swap
  (`PoolHandle::{Static,Refreshing}`, `Weak`-held background task
  re-signing at 2/3 of TTL) keeps an `aurora-dsql-iam` connection alive
  unattended around the clock, removing the manual-Reconnect need that
  段階A left open.
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
- [x] "Explain this query" command — slice (b) of issue 0005:
      `Command::AiExplain { sql, dialect }` routed through the worker
      to `AiProvider::explain`, response rendered in the egui panel.
- [x] "Suggest SQL from prompt" command using current schema snapshot
      (`list_tables` result; full DDL extraction later shipped as
      Stage 2 Group D-1 / ADR-0028) — slice (b) of issue 0005:
      `Command::AiSuggest { prompt, dialect, schema }` carries the
      current `Vec<TableInfo>` to `AiProvider::suggest_sql`.
- [x] Settings UI for API key, provider choice — _Stage 2 Group A,
      planned in ADR-0025 (`ai-providers.toml` + multi-provider
      switcher + Settings UI). Implementation tracked in
      [`.claude/issues/0008-ai-provider-settings-ui-and-persistence.md`](../.claude/issues/0008-ai-provider-settings-ui-and-persistence.md).
      Env var `DBBOARD_ANTHROPIC_API_KEY` keeps working as the
      highest-precedence resolution path (Stage 1 / PR #24).
      **Closed 2026-06-29 on `feature/ai-settings-ui`.** Slice a-1
      (`dbboard-config` layer = `ai-providers.toml` schema +
      `AiSettingsAdmin` use-case + `dbboard.ai.<id>.api_key` keyring
      namespace + `secure_fs` at-rest hardening) landed via PR #37 on
      2026-06-25. Slice a-2-α (`dbboard-ui` worker plumbing =
      `AiProviderSwitcher` trait + `Command::SwitchAiProvider` +
      `Reply::AiProviderSwitched` / `Reply::AiProviderSwitchFailed` +
      `NullAiSwitcher` apps-side stub) landed via PR #39 on
      2026-06-25. Slice a-2-β (`apps/dbboard` `DesktopAiSwitcher`
      real impl + `resolve_ai_provider_from` env > TOML > None
      precedence chain + `AiProviderSlot =
      Arc<RwLock<Option<Arc<dyn AiProvider>>>>` shared slot + worker
      per-request snapshot + 10 new unit tests + README "AI
      integration" rewritten with TOML as the primary path) landed
      via PR #41 on 2026-06-26. Slice (b) (`dbboard-ui`
      `AiSettingsView` egui state machine — List/Add/Edit/ConfirmDelete
      mirroring `ConnectionsView`, with `SecretField::{Keep,Set}` edit
      semantics from ADR-0016 §3 — plus 13 new unit tests, 19
      `ai-settings-*` Fluent keys + `ai-active-with-name` across all
      11 locales (ADR-0022 Tier 1+2 same-commit sync), AI panel
      "Active: { $name }" subtitle, `apps/dbboard` menu button +
      `AiSettingsView` mount + active-id label push + pending-switch
      drain) closes the loop._
- [x] Graceful degradation when no provider configured (ADR-0023
      Decision 11): `has_ai_provider()` gates both the menu entry
      and the panel; with no key set, neither renders. Defence-in-depth
      in the worker too — `Command::Ai*` with `ai_provider == None`
      returns `Reply::AiFailed { AiError::Configuration }` so the
      panel never deadlocks on its busy flag.
- [x] Streaming responses + cooperative cancel + token meter — _Stage 2
      Group B, planned in [ADR-0026](decisions.md). Implementation
      tracked in
      [`.claude/issues/0009-ai-streaming-cancel-tokens.md`](../.claude/issues/0009-ai-streaming-cancel-tokens.md).
      **Closed 2026-06-30 on `feature/ai-streaming-cancel-tokens`.**
      Slice (a) `2cb012e` — `dbboard-ai` trait extension with
      `stream_explain` / `stream_suggest_sql` returning
      `BoxStream<'static, AiResult<StreamEvent>>`, normalized
      `StreamEvent` / `StopReason` enums, and the
      `AiCapabilities::has_streaming` flag activated. Slice (b)
      `e5f49d0` — Anthropic SSE wired through `dbboard-anthropic` via
      `reqwest-eventsource` 0.6 with `RetryPolicy::Never` (token-billed
      POSTs must not silently retry). Slice (c) `e8f5fd5` —
      `dbboard-ui` worker rewired with a tokio async loop + std-to-tokio
      mpsc bridge thread + per-request `CancellationToken`;
      `tokio::select!` races the stream against the token, with the
      cancel arm emitting `Reply::AiCancelled` directly. Slice (d)
      `fff669c` — `AiPanel` state machine extended with `StreamingAcc`,
      lazy chunk accumulator, real `on_stream_chunk` /
      `on_stream_complete` / `on_cancelled`, Send↔Cancel button toggle,
      "Tokens: N in / M out" meter, and 3 new Fluent keys
      (`ai-cancel-button`, `ai-cancelled-message`, `ai-tokens-meter`)
      in all 11 locales._

- [x] AI calls recorded in `history.jsonl` with schema v:2 bump —
      _Stage 2 Group C, planned in [ADR-0027](decisions.md).
      Implementation tracked in
      [`.claude/issues/0010-ai-history-v2.md`](../.claude/issues/0010-ai-history-v2.md).
      **Closed 2026-07-01 on `feature/ai-history-v2`.** Slice (a)
      `b16537f` — `dbboard-ui::history` v:2 reader + writer with a
      `kind: "query" | "ai"` discriminator, `HistoryEntry::{Query, Ai}`
      variant split, 64 KiB write-side truncation, and transparent
      v:1 read-through as `kind: "query"`. Slice (b) `13f7736` —
      `dbboard-ai::AiProvider::identity()` additive method +
      `AiResponse { provider, model }` fields + `dbboard-anthropic`
      impl + `dbboard-ui::worker` spawn-time identity snapshot
      stamped on all four terminal AI reply variants. Slice (c)
      `0e76223` — `dbboard-ui::lib` UI-thread AI history write point
      (`PendingAiSubmit` submit-time snapshot, terminal-reply
      dispatch composing `HistoryEntry::Ai { … }` from the pending
      record + spawn-time identity + streaming accumulator peek,
      18 new unit tests). Slice (d) — docs sweep + `.claude/issues/0010`
      closed + brief 0008 anchors filled + ADR-0027 flipped to
      Accepted. The cross-repo mirror (web-side v:2 pickup) is
      tracked separately in [`.claude/issues/0008-web-history-v2-mirror.md`](../.claude/issues/0008-web-history-v2-mirror.md)._

- [x] Full DDL extraction via `DatabaseAdapter::describe_table` —
      _Stage 2 Group D-1, planned in [ADR-0028](decisions.md).
      Implementation tracked in
      [`.claude/issues/0011-ddl-extraction.md`](../.claude/issues/0011-ddl-extraction.md).
      **Closed 2026-07-03 on `feature/ddl-extraction` (PR #49, merge
      `6c34ee3`).** Slice (a) `a42a27c` (+ review-fix `bba4072`) —
      `dbboard-core` `TableSchema` struct, additive `ColumnInfo.ordinal`
      + `default_value`, `describe_table` trait method with a default
      `Capability`-error impl, and the `Capabilities::has_describe_table`
      flag. Slice (b) `b509a36` — Postgres (`information_schema` +
      composite PK), Turso, and D1 (`PRAGMA table_info`) implementations,
      each flipping `has_describe_table = true`; Postgres integration
      test gated by the `DBBOARD_PG_URL` env-var self-skip. Slice (c)
      `dfdaaca` — additive `SuggestRequest.full_schema`, Anthropic
      prompt rendering, worker `Command::PrefetchSchema` /
      `Reply::SchemaPrefetched` with a Semaphore-8 fan-out, the AiPanel
      "Include column details" checkbox (session-local, gated on
      `has_describe_table`) with a non-blocking partial-failure warning,
      and 11-locale i18n. A narrow `SchemaSource` trait (impl
      `DesktopSchemaSource` in `apps/dbboard`) gives the worker its
      in-process path to the live adapter — the one deviation from the
      ADR, recorded in the ADR status block. Slice (d) `3c3e3d8` —
      docs sweep + `.claude/issues/0011` closed + ADR-0028 flipped to
      Accepted. HTTP contract and `history.jsonl` unchanged, so no
      web mirror is needed._

Exit criteria met for Stage 1: AI panel hidden cleanly when not
configured; visible, two-mode, and usable when it is. Stage 2 Groups
A (in-app settings + multi-provider switcher), B (streaming + cancel
+ token meter), C (AI calls recorded in `history.jsonl` with a
v:2 schema bump), and D-1 (full-DDL schema snapshots via
`describe_table`) are now closed. The remaining Stage 2 deferral
(Group D-2 = function-calling / tool-use, which exposes
`describe_table` as the first callable tool) stays scoped to
ADR-0023 §9 and is queued for its own ADR (ADR-0029).

## Phase 5 — Quality of life

- [x] Result table virtualisation for large result sets — delivered by
      the `egui_extras::TableBuilder` grid rebuild (sticky header,
      resizable columns, `body.rows()` virtualisation, long-cell popup)
      ([ADR-0030](decisions.md), PR #51).
- [x] Query-run ergonomics — F5 / Ctrl·Cmd+Enter / editor right-click all
      run the current SQL, not just the Run button (PR #51).
- [x] Bare-`SELECT` auto-`LIMIT` guard — a visible, opt-out default
      `LIMIT 100` stops unbounded scans from freezing the UI
      ([ADR-0030](decisions.md), PR #51).
- [x] Table structure browser — click a sidebar table to inspect its
      columns via the cross-adapter `describe_table`
      ([ADR-0031](decisions.md), PR #51).
- [x] Table right-click quick-SQL — a sidebar-table context menu that
      drops two read-only starter queries (`SELECT *` and `COUNT(*)`,
      identifier-quoted and schema-qualified) into the editor; kept
      non-destructive by design for the collector handoff (PR #59).
- [x] Help menu with version + docs pointer — a menu-bar entry showing
      the running build version (so a handoff bug report pins an exact
      build) and a pointer at README/`docs/` (PR #60), plus a clickable
      **Project on GitHub** link back to the public repo (PR #65).
- [x] Unified copyable error display — every app-generated error is shown
      as a localized message plus its original English, both selectable
      with a Copy button, so a handoff user can paste the English into an
      AI/search while reading their own language ([ADR-0039](decisions.md),
      PR #70). SQL/provider error bodies stay verbatim.
- [x] In-app update check — a best-effort startup GET to the GitHub
      Releases API compares the latest published tag against the running
      version and, only when newer, shows an **Update available** notice in
      the Help menu with the release notes and a download link. Updating is
      manual; the check is silent on failure and opt-out via
      `DBBOARD_NO_UPDATE_CHECK` ([ADR-0040](decisions.md), PR #71).
      In-use follow-ups (PR #86): the Help menu now stays open while its
      update link and **変更点** changelog toggle are clicked (egui menus
      default to close-on-any-click), and the release notes render as
      Markdown via `egui_commonmark` instead of literal `**source**`
      ([ADR-0043](decisions.md); this raised the workspace MSRV to 1.92).
- [x] Auto-run table right-click quick-SQL — the starter query now
      executes on pick, not just drops into the editor (still read-only, still
      subject to the auto-`LIMIT` guard) (issue 0012, PR #76).
- [x] Inline cell editing with explicit Save (HeidiSQL-style) — double-click
      a cell to edit, blur stages it (theme-aware dirty tint, not yet
      written), a Save row below the grid commits every staged edit via
      primary-key `UPDATE`, serialized one at a time through the existing
      SQL-string query path, then re-browses to show engine-normalized
      values. Editable only for single-table browse results with a resolved
      primary key; arbitrary SQL, views and joins stay read-only. The app's
      first write path ([ADR-0042](decisions.md), issue 0013, PR #79).
      In-use follow-ups (PR #82): every cell is now a full click target so
      empty/NULL cells can be re-edited and right-clicked, the Save row is
      pinned to a bottom panel so it can't scroll off screen, and a hover
      hint teaches double-click-to-edit / right-click-for-NULL.
- [x] Light / Dark / Auto theme — a theme switch with an Auto mode that
      follows the OS setting and persists the choice (issue 0014, PR #77).
      In-use follow-up (PR #82): the Windows title bar now follows the theme
      too via `ViewportCommand::SetTheme`, so Dark no longer leaves a light
      title bar (Auto hands the bar back to the OS).
- [x] Official logo — formalised the hand-authored, original app icon
      (ADR-0032) as the project logo: canonical asset, DESIGN.md +
      README usage, kept-source master (issue 0015, PR #78).
- [x] Local table/column annotations — an editable **Note** column in the
      Structure tab, stored per-user in `annotations.toml` (keyed on the
      stable connection id, so a rename keeps the notes) and written through
      `secure_fs`. Nothing is written to any database, so it works on
      read-only connections and every adapter uniformly. Deliberately does
      *not* surface Postgres `pg_description` (its own ADR) or ride the
      `.dbbx` secret bundle ([ADR-0045](decisions.md), PR #90). Follow-up
      render refactor tracked in issue 0016.
- [x] Read-only MCP server (`dbboard-mcp`) — dbboard now doubles as a
      headless [MCP](https://modelcontextprotocol.io) server that hands its
      already-configured databases to an external AI agent (Claude Desktop /
      Claude Code) over stdio as a small **read-only** tool surface. Five
      fixed tools (`list_connections`, `list_tables`, `describe_table`,
      `run_read_query`, `get_annotations`), reusing the exact
      `connections.toml` + OS-keychain machinery as the GUI. Secrets never
      cross the wire (only `{id,name,kind}` is serialized); read-only is
      **engine-enforced** (`BEGIN TRANSACTION READ ONLY` / `PRAGMA
      query_only` / D1 AST classification), not string-matched; result sets
      clamp to 1000 rows; stdout is reserved for JSON-RPC and all logs go to
      stderr. The v0.3.0 headline feature ([ADR-0046](decisions.md),
      `crates/dbboard-mcp/`). This closes dbboard's AI story in both
      directions: AI *client* (Phase 4) and AI *server* (here).
- [ ] Export results (CSV / JSON)
- [ ] Saved queries
- [ ] Schema diff between two connections
- [ ] Performance: cold-start under 1s on a modern laptop

## Packaging & Distribution

- [x] Windows internal distribution — hardened release exe (console
      suppressed, embedded icon + version metadata, statically-linked
      MSVC CRT so no VC++ Redistributable is needed) plus cargo-wix MSI
      installer sources ([ADR-0032](decisions.md), PR #52). Building the
      MSI is a maintainer step (`cargo wix`); the plain exe needs no
      extra tooling.
- [x] Collector setup pack — `docs/collector-setup/` ships a
      secret-free `connections.template.toml` (D1 / aurora-dsql-iam /
      supabase) plus a Windows `cmdkey` quickstart, so the
      data-collection operator can seed the OS keychain and launch
      without a secret ever touching a tracked file. A guard test
      (`crates/dbboard-config/tests/collector_template.rs`) parses the
      shipped template through the production schema so drift fails
      `cargo test`, not the operator's launch (PR #63).
- [x] Encrypted connection bundle export/import — a passphrase-encrypted
      `.dbbx` file (`age` scrypt + ChaCha20-Poly1305) that carries all
      connections **and** their resolved secrets, collapsing the collector
      handoff from "template + three hand-seeded secrets" to one file plus
      an out-of-band passphrase ([ADR-0038](decisions.md), PR #68). Import
      is skip-and-report on id- and ref-collision; export/import zeroize the
      plaintext and passphrase.
- [x] Internal test-distribution guides — a maintainer runbook
      (`docs/maintainer/internal-distribution.md`: build, sanity-check,
      optional `.dbbx` export, delivery over two channels, do-not-commit
      hygiene) plus a tester onboarding guide (`docs/internal-testing.md`),
      and `.gitignore` rules that keep `*.dbbx` / `/dist/` /
      `connections.toml` out of the public repo (PR #72).
- [x] Build & hand off the collector release exe from develop — the
      ADR-0038-inclusive `dbboard.exe` was rebuilt and physically handed
      off to the data-collection operator (2026-07-16).
- [x] Release CI (build + checksums on a tagged push) — a `v*.*.*` tag push
      builds Windows (exe + MSI) and macOS (`.dmg`) on native runners and
      publishes them to the matching GitHub Release with a combined
      `SHA256SUMS.txt`; `workflow_dispatch` runs the same build as a
      non-publishing smoke test ([ADR-0044](decisions.md), PR #88). **Proven
      green** by the v0.3.0 release (2026-07-22): the first live tag push
      surfaced two macOS `cargo-bundle` quirks (no `--package` selector; can't
      read a workspace-inherited `version.workspace = true` — fixed by
      inlining the resolved version for the bundle step, PR #100) and one
      operational gotcha — `gh release upload` only *attaches* to an existing
      release, so the release object must be created before the publish job
      runs (it did for v0.1.0/v0.2.0 by hand). Making the publish step
      create-if-missing is a tracked follow-up.
- [x] macOS packaging — `[package.metadata.bundle]` lets `cargo bundle
      --release` produce `dbboard.app` on a Mac; the release CI wraps it in a
      compressed `.dmg` via `hdiutil` ([ADR-0044](decisions.md), PR #88).
      Sources are in-tree; the build + code-signing/notarization run on a Mac.
- [ ] Code signing (Authenticode / Apple Developer ID + notarization) —
      removes the SmartScreen / Gatekeeper "unknown publisher" warnings on the
      unsigned artifacts. Needs paid certs + repo secrets; the release
      workflow already has commented `codesign` / `notarytool` / `stapler`
      placeholders (ADR-0044 §Future).
- [ ] Linux packaging (AppImage / `.deb`)

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
