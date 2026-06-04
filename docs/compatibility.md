# Compatibility Matrix

This is the canonical list of what dbboard officially supports.
Policy is defined in [ADR-0011](decisions.md); this document is the
runbook that policy points at. Update it in the same PR that
introduces or drops a version.

## How to read the tiers

| Tier | Meaning | What we promise |
|---|---|---|
| **Tier 1** | Covered by an integration test that runs in CI, or runnable locally behind a documented env var until CI gains the credential. | Regressions block a release. |
| **Tier 2** | Expected to work because the wire/REST surface matches a Tier 1 entry, but no automated test pins it. | Bugs are fixed on a best-effort basis. |
| **Best effort** | Not listed below. | No promise. PRs welcome. |

Server-side databases with a public version number (Postgres,
CockroachDB) follow a **current major + previous major** rule. Managed
services without a user-visible version (Turso platform, Cloudflare D1,
Supabase) track the vendor's current API and the pinned client crate.

## Host (build) requirements

| Item | Version | Notes |
|---|---|---|
| Rust toolchain | stable, **MSRV 1.75** | Declared in `Cargo.toml` (`workspace.package.rust-version`). |
| OS | Windows 10+, macOS 13+, Linux (glibc 2.31+) | Mirrors `egui` / `eframe` 0.34 support. |
| C/C++ toolchain | per platform | Required by `libsql` native deps (see README). |

## Backend support

### Turso / libSQL

| Layer | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| `libsql` client crate | `0.9.x` | — | Pinned in `Cargo.toml` (`workspace.dependencies.libsql`). |
| Local libSQL file | covered | — | Default backend (`:memory:` and on-disk). |
| Turso remote | _planned for Phase 1.5 widening_ | — | Currently disabled by `default-features = false`. |

### Cloudflare D1

| Layer | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| REST API | `v4` (current) | — | Base URL `https://api.cloudflare.com/client/v4`; overridable via `DBBOARD_D1_BASE_URL`. |
| `/raw` endpoint format | covered | — | Live round-trip test gated on `DBBOARD_D1_*` env vars. |

D1 does not expose a user-visible version; the service is treated as a
single moving target tracked by the integration test.

### PostgreSQL-wire (CockroachDB / Neon / vanilla Postgres)

Shared `dbboard-postgres` adapter on `sqlx 0.8 + tls-rustls-ring`.

| Server | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| CockroachDB | `v24.x` | `v23.2` LTS | Postgres wire 3.0; live test gated on `DBBOARD_PG_URL`; `id()` returns `"postgres"`. |
| Neon (managed Postgres) | Postgres `17`, `16` | Postgres `15` | Same adapter; flavored as a first-class kind (ADR-0018) so the runtime adapter id is `"neon"`. Live test gated on `DBBOARD_NEON_URL` (TLS required — Neon enforces `sslmode=require`). |
| Vanilla PostgreSQL | Postgres `17`, `16` | Postgres `15` | Same adapter; no special handling. |

Older Postgres majors (≤ 14) are best effort — the wire protocol
matches, but no commitment.

### Supabase

Phase 3. Will be filled in when the adapter lands; expected baseline is
Supabase's currently supported Postgres majors and the matching
PostgREST + GoTrue API versions.

## Adding or moving a version

1. Open a PR that:
   - Edits this file (add row, move row between tiers, or remove a row).
   - Adds an entry to `CHANGELOG.md` under the next release.
   - If a client crate is upgraded across a breaking change, adds an
     ADR per `CLAUDE.md`.
2. For Tier 1 entries, the PR must also add or update the integration
   test (live or `:memory:` / mock) that exercises the version.
3. Dropping a Tier 1 version is a deprecation: announce it in one
   release, remove it in the next MINOR (or MAJOR after `1.0`).
