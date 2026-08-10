# Changelog

All notable changes to **dbboard** are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [SemVer](https://semver.org/spec/v2.0.0.html), where the
public API is the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) (see
[ADR-0011](docs/decisions.md)).

## [Unreleased]

## [0.6.0] — 2026-08-10

The first database that is not a SQL database. A Cloud Firestore
collection is now listable, queryable and describable from the desktop
client and from the MCP server, and the row model grew a nested cell so a
document has somewhere to land.

### Added

- `Value::Json`, a cell variant carrying a nested document tree, encoded on
  the wire as `{ "$json": … }` ([ADR-0091](docs/decisions.md), issue 0018).
  The prerequisite for the Firestore and MongoDB adapters: a document is a
  tree, and the flat variants had nowhere to put one. The payload is opaque
  — a document containing a `"$blob"` key stays that document — and
  `{ "$json": null }` is a document holding JSON null, not a SQL `NULL`.
  Purely additive: no existing adapter emits it, and SQL `JSON`/`JSONB`
  columns still arrive as `Text`. Consumers of the HTTP contract must accept
  the tag; see the `$json` section of [`docs/api-contract.md`](docs/api-contract.md).
- **`dbboard-firestore`**, the first non-SQL adapter
  ([ADR-0093](docs/decisions.md), issue 0019). `ping`, `list_tables`,
  `query` and `describe_table` against Cloud Firestore over its REST API,
  with service-account or emulator credentials. The query text is a
  Firestore `StructuredQuery` in JSON — the same object Google's docs show,
  with no translation layer — and results come back as ordinary rows, with
  nested documents in the new `Value::Json` cells. `describe_table` samples
  a collection and reports each field with the sample it was inferred from
  (`string (12/20 sampled)`), so an inference never renders as a declared
  schema. Read-only is structural rather than parsed: the crate contains no
  code able to build a Firestore write URL. Verified end-to-end against the
  local Firestore emulator, through the same `connect_adapter` path the
  client and the MCP server use, and with the browse query asserted as the
  exact text the sidebar generates.
- **Cloud Firestore is selectable in the desktop client**
  ([ADR-0094](docs/decisions.md), issue 0019). It is the first connection
  whose credential is optional: leaving the service-account box blank and
  ticking *Connect to the local emulator* is a valid configuration, not an
  unfinished form, and the edit form reopens in whichever of the two states
  the connection is actually in. The service-account JSON is stored in the
  OS keychain like every other secret and is never read back. A blank
  *Database ID* means the project's `(default)` database. The sidebar
  generates a bounded `StructuredQuery` instead of `SELECT * … LIMIT`, and
  drops *Count rows*, which Firestore answers through an endpoint this
  adapter does not implement; a browsed collection is read-only in the grid.
  Configurable without the UI through `DBBOARD_FIRESTORE_PROJECT_ID` and
  friends.
- **Firestore connections are usable from the MCP server** (issue 0019). No
  new wiring — every tool already went through the adapter trait — but the
  tool descriptions now say which kinds exist and that `firestore` takes a
  `StructuredQuery` rather than SQL, with an example. An agent that is only
  told "SQL" sends a `SELECT`, gets a parse error, and reads it as its own
  mistake. `list_connections` now names every kind the server can return,
  guarded by a test against the source rather than a hand-kept list: that
  list had already gone stale twice.

### Fixed

- Documentation described a write policy the code does not have. `run_write`
  never accepted `DROP INDEX` or `COMMENT ON`, and `DROP` is permanently
  closed for *every* object including an index — the allowlist is
  `INSERT` / `UPDATE` / `DELETE` / `MERGE` plus `CREATE TABLE` / `VIEW` /
  `INDEX` / `SCHEMA` and `ALTER TABLE`, exactly as ADR-0087 decided. The
  README, `docs/connections.md` and the 0.5.0 entry below said otherwise;
  two tests now pin the behaviour so the wording cannot drift again.
- `crates/dbboard-mcp/README.md` documented handing the MCP server a whole
  connection through `DBBOARD_MYSQL_URL` and friends. Those variables belong
  to `dbboard-server`'s single-connection resolution path; the MCP server
  resolves a tool call's `connection_id` against `connections.toml` and the
  keychain and reads none of them. It honours `--config` / `DBBOARD_CONFIG`,
  which choose *which* store file, and that is now what the section says.
- `docs/compatibility.md` had no MySQL / MariaDB section at all, three
  releases after the adapter shipped (ADR-0068).

## [0.5.1] — 2026-08-06

A patch release with nothing in it but two bugs found by using the thing.
Both broke a real workflow rather than an edge case: a connection through
an SSH bastion would die and stay dead until the app was restarted, and
schema introspection failed against every MySQL 8 table — which also meant
the MCP tool `list_relationships` had been returning an empty result
instead of an error.

No contract change; [`docs/api-contract.md`](docs/api-contract.md) is
unchanged from 0.2.0.

### Fixed

- **A connection through an SSH bastion could die and stay dead until the
  app was restarted** ([ADR-0092](docs/decisions.md)). Every call after it
  failed with `expected to read 4 bytes, got 0 bytes at EOF`. The tunnel now
  sends SSH keepalives (russh sends none by default, so an idle session was
  simply reaped), and a cached connection that has been idle for 30 seconds
  is pinged before it is used again — a failed ping throws it away and dials
  a fresh one, which is what rebuilds the forward.

- MySQL schema introspection failed on every table against MySQL 8, with
  `type conversion failed: … Rust type alloc::string::String (as SQL type
  VARCHAR) is not compatible with SQL type BLOB`. Since 8.0 the
  `information_schema` views are served from the data dictionary, which
  declares `TABLE_NAME` as `VARBINARY` and `DATA_TYPE` as `BLOB`, so
  sqlx's type check rejected bytes that were perfectly good UTF-8. Metadata
  text is now read as bytes and validated here, the way query cells already
  were. This unbreaks `describe_table`, `table_ddl`, `foreign_keys` and —
  through them — the MCP tools `describe_table`, `search_schema` and
  `list_relationships`, the last of which had been failing *quietly*: it
  swallows a per-table introspection error, so it returned an empty set of
  relationships instead of an error.

### Added

- **A reconnect action**: a reload icon on the connection pill, and a
  Reconnect button in the error banner. The app now heals a dead connection
  on its own, so this is for recovering *now* rather than on the next call.

## [0.5.0] — 2026-08-05

Fifth tagged release, and the one that makes dbboard usable by something
other than a person. `dbboard-mcp` gains the ability to **write** behind
a per-connection gate and a fail-closed allowlist, an **alias** that
keeps real connection ids out of an agent's transcript, and — the part
that was silently missing — **a way to be obtained at all**: the MCP
server is now published as a release binary rather than existing only as
a `cargo build` invocation.

This release also **retires the egui client**. Tauri 2 + SvelteKit is
the only client from here. Installs of the egui build stay on v0.4.0,
which is still downloadable and still works; it has no updater, so
moving off it means downloading the app from the
[download page](https://meta-taro.github.io/dbboard/).

The HTTP contract in [`docs/api-contract.md`](docs/api-contract.md) is
unchanged from 0.2.0.

### Added

- `dbboard-mcp` can **write** — `run_write` runs `INSERT` / `UPDATE` /
  `DELETE` / `MERGE` and `CREATE TABLE` / `VIEW` / `INDEX` / `SCHEMA` /
  `ALTER TABLE`
  behind a three-tier policy (ADR-0087): the connection must be opted in
  with `mcp_write = true`, the statement must parse to something on the
  allowlist, and a permanently-closed list — `GRANT` / `REVOKE` / `DENY`,
  user and role DDL, `SET PASSWORD`, `TRUNCATE`, and `DROP` of anything
  at all — is refused whatever the flag says. Classification is on
  the AST and fails closed. Every existing connection stays read-only
  across the upgrade, because an absent flag means `false`.
- `dbboard-mcp` gained `dump_database`, so an agent can take a backup
  before it writes. It never overwrites an existing file.
- **`dbboard-mcp` is a downloadable binary** (ADR-0090). Releases now
  carry `dbboard-mcp-windows-x86_64.exe` and
  `dbboard-mcp-macos-universal` with checksums, from the same tag as the
  app. Until now the only way to get the MCP server was to build it, so
  the documented `claude mcp add dbboard -- /absolute/path/to/dbboard-mcp`
  named a file that could not exist without a Rust toolchain — an AI
  agent told to use it searched, found nothing installable, and gave up.
  The download page still offers only the desktop app; the MCP server is
  a separate download from the release page, and the setup docs now give
  a concrete install path and the exact `claude mcp add` line per OS.
- `dbboard-mcp` documents passing credentials as environment variables
  (`DBBOARD_*` in the agent's `env` block) for agents that operate under
  a rule against writing credentials to a file, and states plainly that
  TLS uses the OS trust store with no flag to pass behind a
  TLS-terminating corporate proxy (ADR-0034).
- Connections carry an `mcp_write` flag, settable in `connections.toml`
  or from the app (*Connections → Edit → AI agent access*). Editing a
  connection without touching the toggle keeps whatever is stored, so a
  rename cannot silently revoke the permission.
- Connections carry an optional `mcp_alias` — the name `dbboard-mcp`
  shows an AI agent **instead of** the connection's id *and* name
  (ADR-0088). Ids and names are whatever you typed, so on a real install
  they leak a host or a store into the agent's transcript and its
  provider's logs. With an alias set, that is the only string the agent
  sees, and the real id stops working as a handle — one learned from an
  older session cannot be handed back. Aliases must be unique across
  every alias and id. Absent by default and settable in the same place
  as the write gate; everything below the MCP boundary
  (`annotations.toml`, `DBBOARD_CONNECTION`, the connection list) keeps
  using the real id.

### Changed

- **The Tauri 2 + SvelteKit app is the only dbboard client** (ADR-0089).
  It reached parity at v0.4.0 and has since led the egui client — the
  connection form edits SSH tunnels, which egui never learned. Building
  every write surface twice was costing more than the second build
  returned.
- The download page picks builds by product name (`dbboard-desktop…`)
  rather than by file extension, so a release that still carries both
  clients' assets — v0.4.0 does — cannot offer the retired one. This
  supersedes #135, where the answer depended on the order the GitHub
  Releases API happened to return assets in.
- Every generated release page now opens with a link to the
  [download page](https://meta-taro.github.io/dbboard/). A release lists
  raw asset filenames; someone arriving from a search result should not
  have to work out which one is theirs.

### Removed

- The egui client: `crates/dbboard-ui`, `apps/dbboard`, and the
  `eframe` / `egui_extras` / `egui_commonmark` dependencies. Releases up
  to and including v0.4.0 still carry its binaries and keep working;
  nothing after v0.4.0 ships them. It has no updater, so an egui install
  stays on v0.4.0 until it is replaced with a download from the page
  above.
- `dbboard-windows-x86_64.exe`, `dbboard-<version>-x86_64.msi`, and
  `dbboard-macos-universal-<version>.dmg` are no longer built or
  published. `SHA256SUMS.txt` still covers everything that is.
- `crates/dbboard-i18n`, whose message catalogues were egui's; the Tauri
  client carries its own under `apps/desktop/src/lib/i18n/`.
  `crates/dbboard-server` is **kept** despite losing its last in-repo
  consumer — it is the executable statement of the HTTP contract
  `dbboard-web` mirrors ([`docs/api-contract.md`](docs/api-contract.md)).

### Fixed

- The Tauri release jobs pinned Node 20 while `apps/desktop` pins pnpm 11,
  which imports `node:sqlite` (Node 22.5+); `pnpm install` died before
  resolving a package. Both jobs now run Node 22.

### Documentation

- `dbboard-mcp`: document registering the server with **Claude Code**
  (`claude mcp add`), which the top-level README claimed was covered but
  was not; correct the tool count in the README (five → seven); and add
  sections on restarting after a config change, running several agents at
  once against a **tunneled** connection, and reading connection-failure
  symptoms.
- Download page: mention the MCP server, and list MySQL/MariaDB among the
  supported engines.
- **dbboard is easier to find.** Publishing it was not the same as making
  it findable: an agent searching the web for dbboard concluded it was
  "not a publicly available tool". The repo now declares its homepage and
  topics, `README.md` leads with the download link, the site carries
  canonical / Open Graph tags plus `robots.txt` and `sitemap.xml`, and
  `CLAUDE.md`, `crates/dbboard-mcp/README.md` and `apps/desktop/README.md`
  all quote the URL so anyone — human or agent — reading the repo can
  answer "where do I get it".
- The top-level docs no longer describe egui as a current client:
  `README.md`, `CLAUDE.md`, `DESIGN.md`, `docs/architecture.md`,
  `docs/api-contract.md`, `docs/compatibility.md` and `docs/roadmap.md`
  were rewritten around the Tauri client, and `docs/desktop-parity.md` is
  marked archived — it tracked a gap that no longer exists.

## [0.4.0] — 2026-08-04

Fourth tagged release, and the largest so far. Headlined by a **new
desktop client** — Tauri 2 + SvelteKit — which starts the release as a
read-only spike (ADR-0059) and ends it at feature parity with the egui
build, then overtakes it with SSH-tunnel editing. Also lands **MySQL /
MariaDB** as the fourth engine and the first genuinely different SQL
dialect, logical **dump and restore**, an **OpenAI** provider, and
automated **PII / secret leak scanning** on every commit.

Both clients ship from the same tag; the egui single-exe build is still
released and still works. The HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) is unchanged from 0.2.0.

### Added

- **A second desktop client (Tauri 2 + SvelteKit)**, now the primary one.
  It began as an explicitly read-only spike over the egui-free core
  (ADR-0059) — frameless titlebar, sidebar shell, Query/Structure tabs, a
  design-token system with Auto/Light/Dark, a CodeMirror 6 SQL editor
  (ADR-0060), and a result grid with sort, selection, export, and a cell
  popup. Parity with egui then arrived slice by slice:
  - **Connection management write path** (ADR-0062) — create, edit,
    delete, plus passphrase-encrypted bundle import/export. Credentials
    are entered as **parts, never as a hand-written DSN** (ADR-0073); TLS
    is an explicit form choice defaulting to required, with no silent
    plaintext fallback (ADR-0078, ADR-0079); every filesystem path has a
    Browse button (ADR-0077); kinds that only exist in `connections.toml`
    are disabled in the list rather than refused on submit (ADR-0074);
    the edit form asks for exactly the fields the add form does
    (ADR-0080).
  - **Inline cell editing** (ADR-0063) — the first DB-write surface in
    the Tauri app, with an explicit save step.
  - **Logical backup and restore** (ADR-0064, ADR-0065) wired to the same
    core orchestrators the egui build uses.
  - **AI assistant** (ADR-0066) over the existing provider layer.
  - **Auto-update** (ADR-0067) — `tauri-plugin-updater` against a
    `latest.json` manifest assembled by release CI and verified with a
    signing key held only as a repository secret.
- **MySQL / MariaDB adapter** (ADR-0068) — the fourth engine, and the
  first that is not Postgres- or SQLite-shaped. Row-producing paths pin
  the simple/text wire protocol (ADR-0070); generated SQL follows the
  connection's identifier dialect (ADR-0072); the statement-timeout
  variable is **probed rather than assumed**, since MySQL spells it
  `max_execution_time` and MariaDB `max_statement_time` (ADR-0081); a
  listed table nobody can read degrades instead of failing the whole
  sweep (ADR-0071).
- **SSH tunnel** (ADR-0069) — pure-Rust local port forwarding (russh) for
  databases that only listen on a bastion's `localhost`. Host-key
  verification is **mandatory** (fingerprint pin XOR `known_hosts`, never
  blind trust); the connection form can fetch the host key for you to
  confirm instead of demanding you paste it (ADR-0076). Editing lives in
  the Tauri app only — egui users still hand-edit `connections.toml`.
  Passphrases and SSH passwords go to the OS keychain, never to TOML.
- **Logical dump** (ADR-0049) — a plan/threshold pass, keyset paging with
  progress and cancel, SQL literal + `INSERT` rendering, `table_ddl` as
  an adapter capability with Postgres catalog reconstruction and D1
  verbatim DDL, and a user-configurable huge-DB warn threshold
  (ADR-0050).
- **Logical restore / import** (ADR-0051) — a SQL statement splitter, a
  `sqlparser`-backed classifier, an orchestrator with an empty-target
  gate, and per-engine transaction strategy (atomic on Turso, per-
  statement on D1, transaction-or-fallback on Postgres/DSQL).
- **OpenAI (ChatGPT) provider** (ADR-0052) — a `dbboard-openai` Chat
  Completions provider wired through config, admin, UI, and i18n.
- **Two more read-only MCP tools**: `search_schema` for name lookup
  across a connection (ADR-0053) and `list_relationships` for
  foreign-key introspection (ADR-0054), bringing the tool set to seven.
- **PII / secret leak scanning** (ADR-0055) — `scripts/pii-scan.sh` runs
  on every commit, every commit message, and daily in CI, blocking real
  store names, credentials, and maintainer PII from entering a public
  repo. Commit identity is scanned too, because it cannot be edited
  after the fact (ADR-0084).
- **A branded egui design system** (ADR-0056) — palette, tokens, theme
  module — applied to the primary CTA, header identity, and count badge
  (ADR-0057).
- **A download page on GitHub Pages** (ADR-0047), plus Start Menu and
  Desktop shortcuts in the MSI.
- **Sortable result grid**, up to three columns (ADR-0048).

### Changed

- Aurora DSQL no longer receives the read-only transaction preamble; it
  rejects it, so the guard is applied differently there (ADR-0061).
- Long cell values open an editor instead of being read through a
  keyhole (ADR-0082); the sidebar splits and popovers place themselves
  (ADR-0083).
- The desktop lib is `rlib`-only, so its fingerprint can vary per build
  configuration — `cargo build` and `cargo test` no longer evict each
  other's fingerprint and recompile it. Pre-push dropped from ~136s to
  ~58s (ADR-0086).

### Fixed

- Long error messages wrap in the inline error banner instead of
  overflowing.
- The header pill and theme toggle moved off the menu bar, where they
  overlapped (ADR-0057).
- The PII identity allowlist admits GitHub's `web-flow` committer, so
  commits authored through the GitHub web UI no longer fail the scan
  (ADR-0085).

### Security

- SSH host-key verification is mandatory and has no "trust anything"
  path (ADR-0069).
- SSH key passphrases and passwords are stored only in the OS keychain;
  `connections.toml` holds a reference, never a secret. Switching auth
  method no longer persists a keyring reference that was never written.
- PII / secret scanning is wired into the commit path and CI (ADR-0055),
  with a known gap recorded: without a local `.pii-denylist` the
  business-identifying rules cannot fire, so the untracked denylist and
  the `PII_DENYLIST` CI secret are load-bearing.

## [0.3.0] — 2026-07-21

Third tagged release. Headlined by **dbboard as a read-only MCP server**
(ADR-0046) so an external AI agent — Claude Desktop, Claude Code — can
drive the databases dbboard is already configured with, without ever
seeing a secret and without being able to write. Also rolls up local
column/table annotations, the signed-off distribution installers +
Release CI, and a batch of AI-panel and packaging fixes. Desktop-only;
the HTTP contract in [`docs/api-contract.md`](docs/api-contract.md) is
unchanged from 0.2.0.

### Added

- **Read-only MCP server** (`dbboard-mcp`): a standalone stdio binary
  that exposes dbboard's configured connections to an MCP client as five
  read-only tools — `list_connections`, `list_tables`, `describe_table`,
  `run_read_query`, `get_annotations`. Read-only is **engine-enforced**
  (Postgres `BEGIN READ ONLY`, libSQL `PRAGMA query_only`, D1 AST
  classification), not string matching; results are bounded; secrets stay
  in the OS keychain and are never serialized into a tool result or error.
  Connection wiring is factored into a new `dbboard-connect` crate so the
  binary reuses the server's connect path without pulling in axum
  (ADR-0046).
- **Local column and table annotations** (`annotations.toml`): an editable
  Note column on the Structure tab, keyed by connection id / table /
  column. Notes live in the config directory, never touch the database,
  work on read-only connections, and apply uniformly across every adapter
  (ADR-0045).
- **Distribution installers + Release CI** (ADR-0044): a `v*.*.*` tag now
  publishes a GitHub Release carrying Windows (`.exe` + MSI) and macOS
  (`.dmg`) artifacts with a `SHA256SUMS.txt`, via `cargo-wix` /
  `cargo-bundle`. Artifacts are unsigned for now (SmartScreen / Gatekeeper
  warnings remain; code signing is tracked separately).

### Changed

- **AI scope caption** in the assistant panel now reads as a standing,
  emphasised guarantee — the assistant only drafts SQL, it never runs it
  or touches data on its own — instead of dismissible fine print
  (ADR-0045 follow-up).
- **Default Anthropic model** bumped to `claude-sonnet-5`.
- **Help menu** renders update-notice release notes as Markdown and stays
  open on inside clicks so links and change notes are usable (ADR-0043).

### Fixed

- **Anthropic streaming errors** now surface the API response body (e.g.
  insufficient balance, invalid model) instead of a bare `status 400`.

### Security

- `cargo deny` advisory/license drift resolved: three transitive
  build-time advisories (proc-macro-error2 unmaintained, the quick-xml
  DoS pair) documented as ignores with reasons, `MPL-2.0` allowed for
  `option-ext`, and the dead `CDLA-Permissive-2.0` allowance trimmed.
- A `security-reviewer` pass over the MCP crate found no CRITICAL/HIGH
  issues; the five secret/read-only invariants are verified at the source.

### Documentation

- ADR-0043 through ADR-0046 capture the decisions since 0.2.0.
- `crates/dbboard-mcp/README.md` (tool table, security posture, Claude
  Desktop wiring) and the dbboard-connect / dbboard-mcp entries in
  [`docs/architecture.md`](docs/architecture.md).

## [0.2.0] — 2026-07-17

Second tagged release. Rolls up Phase 3 (multi-connection management),
Phase 4 (AI assistant), the Windows internal-distribution work, and the
in-use quality-of-life batch. Desktop-only; the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) is unchanged from 0.1.0.

### Added

- **AI assistant** (`dbboard-ai` + Anthropic provider): natural-language
  → SQL with streaming output, cooperative cancel, a token meter, and
  schema-aware prompting via full `describe_table` DDL (ADR-0023 through
  ADR-0028).
- **Inline cell editing with explicit Save** (HeidiSQL-style): double-click
  a cell to edit, blur stages it, a pinned Save row commits every staged
  edit via a primary-key `UPDATE`. Editable only for single-table browse
  results with a resolved primary key (ADR-0042).
- **Multiple named connections** with OS-keychain secrets, live switching,
  and **encrypted `.dbbx` bundle export/import** (passphrase-encrypted,
  carries connections + resolved secrets in one file; ADR-0038).
- **Aurora DSQL** support with self-minted SigV4 IAM auth and timer-based
  token pool-swap so long-lived sessions don't get recycled
  (ADR-0036 / ADR-0037).
- **Query workflow**: persisted history, a Structure tab, an auto-`LIMIT`
  guard for bare `SELECT`s, result export (CSV / JSON), expandable cells,
  and right-click table quick-SQL that runs on pick (ADR-0030 / ADR-0031 /
  ADR-0035).
- **Light / Dark / Auto theme** that follows the OS setting, persists the
  choice, and syncs the Windows title bar (ADR-0041).
- **Startup update check** against GitHub Releases: a non-blocking,
  opt-out (`DBBOARD_NO_UPDATE_CHECK`) notification in the Help menu when a
  newer version is published (ADR-0040).
- **Unified error surface**: copyable, bilingual (Japanese + original
  English) error display (ADR-0039).
- **Localisation** across 11 locales.
- **Windows packaging**: console-suppressed release binary with embedded
  icon and version metadata, statically linked CRT (no VC++ redist), and
  in-tree `cargo-wix` MSI sources (ADR-0032).

### Documentation

- ADR-0012 through ADR-0042 capture every non-trivial decision since 0.1.0.
- Maintainer runbooks and tester onboarding for the internal distribution
  under [`docs/maintainer/`](docs/maintainer/) and
  [`docs/internal-testing.md`](docs/internal-testing.md).

## [0.1.0] — 2026-05-25

First tagged release. Closes Phase 1 (Turso vertical slice) and the
follow-on Phase 1.5 / 1.6 / 1.7 work; see
[`docs/roadmap.md`](docs/roadmap.md).

### Added

- **Database adapters** for the initial scope:
  - `dbboard-turso` — Turso / libSQL (`:memory:` and local file).
  - `dbboard-d1` — Cloudflare D1 via REST `/raw` (Phase 1.6, ADR-0007).
  - `dbboard-postgres` — PostgreSQL-wire (CockroachDB and Neon use the
    same adapter; Phase 1.7, ADR-0008).
- **Local HTTP backend** `dbboard-server` (axum) bound to loopback on
  an OS-assigned port; UI is now an HTTP client (Phase 1.5,
  ADR-0006 / ADR-0009).
- **egui UI** with table sidebar, SQL editor, result grid, and inline
  error surface.
- **HTTP contract** in [`docs/api-contract.md`](docs/api-contract.md) —
  the canonical surface shared with `dbboard-web`.
- **10,000-row cap** per query, uniform across adapters, returned as a
  `query` error (HTTP 400) instead of silently truncating.
- **Versioning & DB-support policy**: SemVer with the HTTP contract as
  the public API; tiered backend support
  ([ADR-0011](docs/decisions.md), [`docs/compatibility.md`](docs/compatibility.md)).
- **`cargo-deny`** configuration gating the dependency graph on
  advisories, licenses, duplicates, and unknown sources.
- **`cargo-husky`** pre-commit and pre-push hooks running fmt, clippy
  (`-D warnings`), check, and tests; pre-push additionally runs release
  build and tests, skipping on deletion-only pushes.

### Security

- TLS hardening for the Postgres adapter: `sslmode=Prefer` is upgraded
  to `Require` (explicit `disable` is respected) to avoid silent
  plaintext fallback.
- D1 transport errors are scrubbed of URL / account ID / database ID
  before surfacing to the user.
- Turso connection errors redact the file path.
- The loopback server is unauthenticated by design; widening the bind
  or persisting the port requires a per-launch secret first (ADR-0009).

### Documentation

- ADR-0001 through ADR-0011 capture every non-trivial decision so far.
- README, `docs/architecture.md`, `docs/api-contract.md`,
  `docs/compatibility.md`, and `docs/roadmap.md` reflect the shipped
  scope.

[Unreleased]: https://github.com/meta-taro/dbboard/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/meta-taro/dbboard/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/meta-taro/dbboard/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/meta-taro/dbboard/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/meta-taro/dbboard/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/meta-taro/dbboard/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/meta-taro/dbboard/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/meta-taro/dbboard/releases/tag/v0.1.0
