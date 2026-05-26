# Changelog

All notable changes to **dbboard** are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [SemVer](https://semver.org/spec/v2.0.0.html), where the
public API is the HTTP contract in
[`docs/api-contract.md`](docs/api-contract.md) (see
[ADR-0011](docs/decisions.md)).

## [Unreleased]

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

[Unreleased]: https://github.com/meta-taro/dbboard/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/meta-taro/dbboard/releases/tag/v0.1.0
