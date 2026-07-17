# Changelog

All notable changes to **dbboard** are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [SemVer](https://semver.org/spec/v2.0.0.html), where the
public API is the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) (see
[ADR-0011](docs/decisions.md)).

## [Unreleased]

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
