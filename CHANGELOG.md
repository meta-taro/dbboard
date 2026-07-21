# Changelog

All notable changes to **dbboard** are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [SemVer](https://semver.org/spec/v2.0.0.html), where the
public API is the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) (see
[ADR-0011](docs/decisions.md)).

## [Unreleased]

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

[Unreleased]: https://github.com/meta-taro/dbboard/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/meta-taro/dbboard/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/meta-taro/dbboard/releases/tag/v0.1.0
