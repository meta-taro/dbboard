# dbboard

A high-performance desktop database client for modern serverless and
distributed databases.

dbboard is a learning and reference project that explores multi-database
integration, local-first tooling, and pluggable AI-assisted workflows. It
exposes a unified, native UI for Neon, Supabase, Aurora DSQL, and
Turso/libSQL, with an adapter-based architecture that makes adding new
databases straightforward.

## Status

Pre-1.0; workspace at `0.1.0` with Phases 1 and 3 closed. The Turso,
Cloudflare D1, CockroachDB, Neon, Supabase, and AWS Aurora DSQL adapters
all ship over the local HTTP backend. See [`CHANGELOG.md`](CHANGELOG.md)
for what landed and [`docs/roadmap.md`](docs/roadmap.md) for the next
phase.

This is the **desktop** implementation. The web counterpart lives at
[meta-taro/dbboard-web](https://github.com/meta-taro/dbboard-web) (Nuxt +
NestJS). The two share concepts and feature parity goals but are
independent codebases.

## Goals

- **Performance first** — native Rust UI built on egui.
- **Local first** — no required external services to run.
- **Modular** — database and AI layers are decoupled.
- **Extensible** — new databases and AI providers can be added behind traits.

## Supported Databases (initial scope)

- Turso / libSQL (SQLite-based distributed DB)
- Cloudflare D1 (SQLite-based, REST API)
- CockroachDB (distributed SQL, PostgreSQL-wire)
- Neon (managed PostgreSQL)
- Supabase (managed PostgreSQL)
- AWS Aurora DSQL (managed PostgreSQL-wire)

All six adapters ship today. The four pg-wire flavors share the
generic `dbboard-postgres` adapter (`sqlx` + `tls-rustls-ring`),
differing only in the runtime label exposed by `DatabaseAdapter::id()`
(`"postgres"`, `"neon"`, `"supabase"`, `"aurora-dsql"`) so the
connection picker and history records can label each connection
precisely. See [ADR-0018](docs/decisions.md) (Neon),
[ADR-0019](docs/decisions.md) (Supabase), and
[ADR-0021](docs/decisions.md) (Aurora DSQL).

The Supabase REST/auth layer and Aurora DSQL's SDK-driven IAM token
auto-refresh are deliberately deferred to a future ADR — at this
stage all pg-wire flavors use the same `postgres://…` URL contract.

The authoritative per-version support matrix (Tier 1 / Tier 2 / best
effort) lives in [`docs/compatibility.md`](docs/compatibility.md);
versioning and DB-support policy are defined in
[ADR-0011](docs/decisions.md).

## Architecture

Three main layers, organised as a cargo workspace:

- **UI layer** — Rust + egui, native desktop interface.
- **Database adapter layer** — abstracts database-specific logic behind a
  single trait so multiple providers plug in.
- **AI integration layer (optional)** — pluggable providers (Claude,
  OpenAI, local LLMs). Isolated from core DB operations.

See [`docs/architecture.md`](docs/architecture.md) for the full crate map
and dependency rules.

## Requirements

- Rust stable (latest)
- `cargo` (bundled with Rust)
- A C/C++ toolchain for `libsql` native deps:
  - Windows: MSVC Build Tools
  - macOS: Xcode Command Line Tools
  - Linux: `build-essential`

## Setup

```sh
git clone https://github.com/<your-org>/dbboard.git
cd dbboard
cargo test
```

Running `cargo test` once installs the `cargo-husky` git hooks
(pre-commit, pre-push).

## Run

```sh
cargo run -p dbboard
```

On startup the binary boots a small HTTP server bound to loopback
(`127.0.0.1`) on an OS-assigned port, and the UI talks to it over that
local connection — the same API contract the web sibling implements (see
[`docs/api-contract.md`](docs/api-contract.md)). The server is local-only
and shuts down when you close the window; nothing listens on a public
interface.

By default the app opens an in-memory Turso/libSQL database, so it runs
with no configuration. The backend is chosen by, in priority order:

1. The environment variables documented below
   (`DBBOARD_AURORA_DSQL_URL` > `DBBOARD_NEON_URL` >
   `DBBOARD_SUPABASE_URL` > `DBBOARD_PG_URL` > `DBBOARD_D1_*` >
   `DBBOARD_TURSO_PATH`). Among the four pg-wire flavors the order is
   alphabetical — setting two flavored vars at once is unusual but
   the precedence is fully defined.
2. `DBBOARD_CONNECTION=<id>` resolved against `connections.toml` — the
   local connection store backed by your OS keychain (ADR-0013).
3. If `connections.toml` has exactly one entry, that one is auto-selected.
4. Otherwise an in-memory Turso/libSQL database.

See [`docs/connections.md`](docs/connections.md) for the connection-store
schema and where the file lives per OS.

Once registered, the **Connections** window (top menu bar) lets you
add / edit / delete entries and swap the active connection on the
running process — the per-row **Connect** button swaps the backend
in-place, no app restart needed (in-flight requests intentionally
finish on the old backend; new ones pick up the new one). See
[ADR-0020](docs/decisions.md) for the swap semantics.

The same menu bar carries a **Language** / **言語** submenu listing
the 11 shipped locales by their native names. Picking one swaps the
UI language in place; the `DBBOARD_LANG` env var still drives the
startup default and is unchanged by the runtime switcher. See
[ADR-0022](docs/decisions.md).

### Local Turso/libSQL (default)

| Variable | Purpose | Default |
|---|---|---|
| `DBBOARD_TURSO_PATH` | libSQL file path, or `:memory:` | `:memory:` |

### Cloudflare D1

Set all three of the following to connect to D1 instead of Turso:

| Variable | Purpose |
|---|---|
| `DBBOARD_D1_ACCOUNT_ID` | Cloudflare account ID |
| `DBBOARD_D1_DATABASE_ID` | D1 database ID (`wrangler d1 info <name>`) |
| `DBBOARD_D1_TOKEN` | API token with the **D1 Edit** permission |
| `DBBOARD_D1_BASE_URL` | _(optional)_ API root override; defaults to `https://api.cloudflare.com/client/v4` |

The account and database IDs are shown in the Cloudflare dashboard
(Workers & Pages → D1) or via `wrangler d1 info <database-name>`. Create
the API token under **My Profile → API Tokens** with a D1 read/write
permission. If any of the three required variables is missing, the app
falls back to the local Turso default.

```sh
DBBOARD_D1_ACCOUNT_ID=... DBBOARD_D1_DATABASE_ID=... DBBOARD_D1_TOKEN=... \
  cargo run -p dbboard
```

### CockroachDB / PostgreSQL

Set a single connection string to connect to CockroachDB or any generic
PostgreSQL-wire database (vanilla Postgres, self-hosted) via the
`dbboard-postgres` adapter:

| Variable | Purpose |
|---|---|
| `DBBOARD_PG_URL` | Full connection string, e.g. `postgresql://user:pass@host:26257/db?sslmode=verify-full` |

For **CockroachDB Cloud**, copy the connection string from the cluster's
**Connect** dialog in the CockroachDB Cloud Console (Basic free tier
works). For a **self-hosted** node started with
`cockroach start-single-node`, use its `postgresql://…` string; the
default SQL port is `26257`. CockroachDB requires TLS, so keep
`sslmode=verify-full` (or the mode your deployment expects).

```sh
DBBOARD_PG_URL='postgresql://user:pass@host:26257/db?sslmode=verify-full' \
  cargo run -p dbboard
```

For **Neon**, **Supabase**, and **AWS Aurora DSQL** the same adapter is
used but the connection is labelled distinctly at runtime
(`"neon"`, `"supabase"`, `"aurora-dsql"` vs `"postgres"`) so the
picker and history records can tell them apart. Each flavor has its
own env var, all of which outrank `DBBOARD_PG_URL`:

| Variable | Purpose |
|---|---|
| `DBBOARD_NEON_URL` | Neon connection string. TLS required — `sslmode=require` (or stronger). See [ADR-0018](docs/decisions.md). |
| `DBBOARD_SUPABASE_URL` | Supabase connection string. TLS required. Both the direct `:5432` endpoint and the transaction-pooler `:6543` endpoint work — the URL itself picks. See [ADR-0019](docs/decisions.md). |
| `DBBOARD_AURORA_DSQL_URL` | Aurora DSQL connection string. TLS required. The password segment must be a fresh short-lived IAM authentication token (~15 min TTL); an expired token surfaces as a connection error at startup. See [ADR-0021](docs/decisions.md). |

All four pg-wire vars contain credentials — keep them out of version
control (use `.env`, which is gitignored). The app never logs them.

```sh
DBBOARD_NEON_URL='postgres://user:pass@ep-…neon.tech/db?sslmode=require' \
  cargo run -p dbboard

DBBOARD_SUPABASE_URL='postgres://user:pass@db.<ref>.supabase.co:5432/postgres?sslmode=require' \
  cargo run -p dbboard

# Aurora DSQL: the password segment is a short-lived IAM token.
DBBOARD_AURORA_DSQL_URL='postgres://admin:<IAM-token>@<cluster>.dsql.<region>.on.aws:5432/postgres?sslmode=require' \
  cargo run -p dbboard
```

### AI integration (optional)

dbboard ships an optional AI panel that can explain SQL and suggest
queries against the active connection's schema. The panel and the
menu entry that toggles it are both hidden when no provider is
configured — graceful degradation = absence (see
[ADR-0023](docs/decisions.md) Decision 11).

When wired, the **AI Assistant** menu entry (top bar, between
Connections and Language) opens a two-mode window: **Explain SQL**
(paste SQL, get a natural-language walkthrough) and **Suggest SQL**
(describe a question, get a SQL draft using the active connection's
table list as context). Responses render inline; errors are surfaced
with translated prefixes so a 429 or network failure does not look
identical to a successful empty response. AI calls do **not** travel
the dbboard-web HTTP contract — they go directly from the desktop
binary's worker thread to the provider over `reqwest`.

Stage 1 wires a single provider (Anthropic Messages API):

| Variable | Purpose | Default |
|---|---|---|
| `DBBOARD_ANTHROPIC_API_KEY` | API key from the Anthropic console. **Required** to enable the AI panel. | _(unset = AI panel hidden)_ |
| `DBBOARD_ANTHROPIC_MODEL` | Model identifier override. | `claude-sonnet-4-6` |

If the key is missing or construction fails (e.g. an empty model
override), the binary logs to stderr and continues without AI — the
rest of the app keeps working. The key never appears in `Debug`
output or in `history.jsonl`; it is held only in memory for the
process lifetime.

```sh
DBBOARD_ANTHROPIC_API_KEY='sk-ant-…' cargo run -p dbboard
```

Stage 2 capabilities (streaming, multi-provider switcher,
keychain-backed `ai-providers.toml`, AI calls recorded in
`history.jsonl`, full-DDL schema snapshots, function-calling) are
deferred — see ADR-0023 §9.

## Development

Before committing, the pre-commit hook runs:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
cargo test --all-features
```

Before pushing, the pre-push hook also runs:

```sh
cargo build --release
cargo test --all-features --release
```

Pure deletion pushes (`git push --delete <branch>`) skip the
build/test cycle — there is no working tree to validate.

You can run these manually at any time.

### Security checks

dbboard creates `connections.toml` and `history.jsonl` under your
per-user config dir. On Unix both land as mode `0o600`; on Windows
they inherit the user-only DACL of `%APPDATA%\Roaming\<user>\`. If
the resolved config dir lives under a cloud-sync vendor folder
(OneDrive Known Folder Move, iCloud Drive, Dropbox, Google Drive),
the binary emits one stderr warning at startup naming the vendor.
The single most effective hardening on a lost laptop is full-disk
encryption — enable BitLocker / FileVault / dm-crypt. See
[`docs/connections.md` § File permissions](docs/connections.md#file-permissions-and-at-rest-posture-adr-0024)
and ADR-0024 for the full posture.

`cargo-deny` gates the dependency graph on advisories, licenses,
duplicate versions, and unknown sources. Configuration lives in
[`deny.toml`](deny.toml).

```sh
cargo install --locked cargo-deny    # one-time, ~5 min build
cargo deny check                     # advisories + licenses + bans + sources
```

CI does not run this yet; run it locally when adding or upgrading a
dependency. New license expressions surfaced by the check go into
`deny.toml`'s `licenses.allow` list with a one-line rationale.

## Contributing

This project follows the rules in [`CLAUDE.md`](CLAUDE.md). In short:

1. Write a failing test before changing behaviour.
2. Keep changes small and focused.
3. Use conventional-style commit messages in English.
4. Record non-trivial decisions in [`docs/decisions.md`](docs/decisions.md).

## License

See [`LICENSE`](LICENSE).
