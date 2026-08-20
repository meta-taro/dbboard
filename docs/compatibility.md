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
Supabase, Firestore) track the vendor's current API and the pinned
client crate.

## Host (build) requirements

| Item | Version | Notes |
|---|---|---|
| Rust toolchain | stable, **MSRV 1.92** | Declared in `Cargo.toml` (`workspace.package.rust-version`). |
| Node.js + pnpm | Node 20+, pnpm via corepack | The client's frontend build. Version pinned by `apps/desktop/package.json`'s `packageManager`. |
| OS | Windows 10+, macOS 13+ | Mirrors Tauri 2's WebView support (WebView2 on Windows, WKWebView on macOS). Linux is buildable but not released. |
| C/C++ toolchain | per platform | Required by `libsql` native deps (see README). |

## Backend support

### Turso / libSQL

| Layer | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| `libsql` client crate | `0.9.x` | — | Pinned in `Cargo.toml` (`workspace.dependencies.libsql`). |
| Local libSQL file | covered | — | Kind `turso` (`:memory:` and on-disk). |
| Turso Cloud / remote `sqld` | covered | — | Kind `turso-remote`, hrana over HTTP (ADR-0111). Verified by hand against Turso Cloud; `libsql://`, `https://`, `http://`, `wss://` and `ws://` are accepted. |

### Cloudflare D1

| Layer | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| REST API | `v4` (current) | — | Base URL `https://api.cloudflare.com/client/v4`; overridable via `DBBOARD_D1_BASE_URL`. |
| `/raw` endpoint format | covered | — | Live round-trip test gated on `DBBOARD_D1_*` env vars. |

D1 does not expose a user-visible version; the service is treated as a
single moving target tracked by the integration test.

### Cloud Firestore

Read-only over the REST API (ADR-0091, ADR-0093, ADR-0094). Runtime
adapter id is `"firestore"`.

| Layer | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| REST API | `v1` (current) | — | Base URL `https://firestore.googleapis.com/v1`; overridable per connection, which is how the emulator is reached. |
| Firestore emulator | covered | — | Live test gated on `DBBOARD_TEST_FIRESTORE_URL`; run `firebase emulators:start --only firestore`. Needs no credential, so this is the one live test any contributor can run. |
| Production Firestore | — | current service | Same `v1` surface, reached with a service-account key. No automated test pins it: doing so would mean holding a real Google credential. |
| Datastore-mode databases | — | — | Out of scope. The REST surface differs, and `:runQuery` is not offered. |

Firestore has no user-visible version; the service is treated as a
single moving target tracked by the emulator test. Queries are
Firestore `StructuredQuery` JSON, not SQL — see `docs/architecture.md`
for why the adapter has no write path rather than a blocked one.

### MongoDB

Read-only over the official driver (ADR-0091, ADR-0095, ADR-0096).
Runtime adapter id is `"mongodb"`.

| Server | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| MongoDB | `8.x` | `6.x`, `7.x` | Live test gated on `DBBOARD_TEST_MONGODB_URI`; run `docker run -d --rm -p 27117:27017 mongo:8`. The commands the adapter sends have been stable since 4.4, but only 8.x is pinned by a test. |
| MongoDB Atlas | — | current service | Same driver and same commands; `mongodb+srv://` discovers the replica set from DNS SRV. Untested here. |
| Amazon DocumentDB / Azure Cosmos (Mongo API) | — | — | Wire-compatible in principle, but neither implements every command in the read allowlist. Best effort. |

| Layer | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| `mongodb` driver crate | `3.x` | — | Pinned in `Cargo.toml` (`workspace.dependencies.mongodb`). |

Queries are MongoDB command documents as JSON, not SQL. The adapter
accepts `aggregate`, `count`, `distinct`, `find`, `listCollections`,
and `listIndexes`, and refuses everything else — including a read that
smuggles in `$merge`, `$out`, `$where`, `$function`, or
`$accumulator`.

MongoDB connections cannot be tunnelled over SSH. One URI may name
several hosts, and `mongodb+srv://` discovers a whole replica set from
DNS, so rewriting a single host to a loopback forward leaves the driver
failing over to members the tunnel never covered — it appears to work,
then silently stops.

### PostgreSQL-wire (CockroachDB / Neon / Supabase / Aurora DSQL / vanilla Postgres)

Shared `dbboard-postgres` adapter on `sqlx 0.8 + tls-rustls-ring`.

| Server | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| CockroachDB | `v24.x` | `v23.2` LTS | Postgres wire 3.0; live test gated on `DBBOARD_PG_URL`; `id()` returns `"postgres"`. |
| Neon (managed Postgres) | Postgres `17`, `16` | Postgres `15` | Same adapter; flavored as a first-class kind (ADR-0018) so the runtime adapter id is `"neon"`. Live test gated on `DBBOARD_NEON_URL` (TLS required — Neon enforces `sslmode=require`). |
| Supabase (managed Postgres) | Postgres `17`, `16`, `15` | — | Same adapter; flavored as a first-class kind (ADR-0019) so the runtime adapter id is `"supabase"`. Live test gated on `DBBOARD_SUPABASE_URL` (TLS required; both the direct `:5432` endpoint and the transaction-pooler `:6543` endpoint are covered — pick via the URL). |
| AWS Aurora DSQL | current managed service | — | Same adapter; flavored as a first-class kind (ADR-0021) so the runtime adapter id is `"aurora-dsql"`. Live test gated on `DBBOARD_AURORA_DSQL_URL` (TLS required; the URL's password segment must carry a short-lived IAM authentication token, ~15 min TTL). The `aurora-dsql-iam` connection kind (ADR-0036) mints that token itself from stored AWS credentials, for connections that outlive the ~15-min TTL. Aurora DSQL has no user-visible Postgres major version; the service is tracked as a single moving target. |
| Vanilla PostgreSQL | Postgres `17`, `16` | Postgres `15` | Same adapter; no special handling. |

Older Postgres majors (≤ 14) are best effort — the wire protocol
matches, but no commitment.

The Supabase REST surface (PostgREST, GoTrue, Storage, Realtime) is
deliberately out of scope at this stage: ADR-0019 limits Phase 3 to
the pg-wire path. A future ADR will decide whether to layer REST
capabilities on top of the flavored adapter.

Aurora DSQL IAM-token handling comes in two kinds. `aurora-dsql`
(ADR-0021) takes a manually supplied URL where the user owns token
freshness. `aurora-dsql-iam` (ADR-0036) mints the SigV4 token itself
from stored AWS credentials — using a hand-rolled pure-Rust signer, not
the AWS SDK, so the rustls-`ring` crypto stack (ADR-0034) is preserved.
v1 mints the token when the connection is built (startup and each
connection switch); continuous in-pool refresh before expiry is a
planned follow-up ADR.

### MySQL / MariaDB

Separate `dbboard-mysql` adapter on `sqlx 0.8 + tls-rustls-ring-native-roots` — a
distinct SQL dialect, not a pg-wire flavor (ADR-0068). Runtime adapter id
is `"mysql"` for every server below.

| Server | Tier 1 | Tier 2 | Notes |
|---|---|---|---|
| MySQL | `8.x` | `5.7.8`–`5.7.x` | Live test gated on `DBBOARD_MYSQL_URL`. Read-only queries run inside `SET TRANSACTION READ ONLY`, with the statement timeout spelled `max_execution_time` (milliseconds). 8.x serves `information_schema` from the data dictionary, which declares `TABLE_NAME` as VARBINARY and `DATA_TYPE` as BLOB — introspection decodes those as bytes (fixed in 0.5.1). |
| MariaDB | — | `10.1`+, `11.x` | Same wire protocol and same adapter; the statement timeout is spelled `max_statement_time` (seconds) and the adapter probes for the spelling once per connection, because asking a server for the variable it does not have is a hard error. No automated test pins a MariaDB version. |
| PlanetScale | — | current service | MySQL-wire; untested here. |

MySQL `5.6` and older have neither timeout variable. They are best
effort: queries still run, but the read-only path cannot bound them.

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
