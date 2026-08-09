<img src="assets/dbboard-logo-256.png" alt="dbboard logo" align="right" width="96" height="96">

# dbboard

### ⬇️ [**Download dbboard**](https://meta-taro.github.io/dbboard/) — Windows `.exe` / macOS `.dmg`

[![Download](https://img.shields.io/badge/Download-dbboard-2f81f7?style=for-the-badge)](https://meta-taro.github.io/dbboard/)
[![Latest release](https://img.shields.io/github/v/release/meta-taro/dbboard?style=for-the-badge)](https://github.com/meta-taro/dbboard/releases/latest)

A high-performance desktop database client for modern serverless and
distributed databases.

dbboard is a learning and reference project that explores multi-database
integration, local-first tooling, and pluggable AI-assisted workflows. It
exposes a unified, native UI for Neon, Supabase, Aurora DSQL, MySQL,
Turso/libSQL and Cloud Firestore, with an adapter-based architecture that
makes adding new databases straightforward.

## Status

Pre-1.0; workspace at `0.4.0`. Phases 1, 3, and the Phase 4 AI assistant
are closed, and dbboard now doubles as an **MCP server** (`dbboard-mcp`)
for external AI agents (ADR-0046) — read-only unless you opt a connection
in, and never for privilege changes, `TRUNCATE` or `DROP` (ADR-0087).
The Turso, Cloudflare
D1, CockroachDB, Neon, Supabase, AWS Aurora DSQL, MySQL / MariaDB and
Cloud Firestore adapters all ship over the local HTTP backend. See
[`CHANGELOG.md`](CHANGELOG.md) for what
landed and [`docs/roadmap.md`](docs/roadmap.md) for the next phase.

The client is **Tauri 2 + SvelteKit** over the shared Rust core crates. The
egui client it replaced shipped through v0.4.0 and was retired in
[ADR-0089](docs/decisions.md); releases up to v0.4.0 still carry its binaries.

This is the **desktop** implementation. The web counterpart lives at
[meta-taro/dbboard-web](https://github.com/meta-taro/dbboard-web) (Nuxt +
NestJS). The two share concepts and feature parity goals but are
independent codebases.

## Download

A prebuilt Windows installer (`.exe`) and macOS disk image (`.dmg`) for the
latest release are on the **[download page](https://meta-taro.github.io/dbboard/)**
(GitHub Pages), or directly on the
[Releases](https://github.com/meta-taro/dbboard/releases) page. Every
release ships a `SHA256SUMS.txt`; verify your download before running it.
The binaries are not code-signed yet, so Windows SmartScreen / macOS
Gatekeeper will warn on first launch (see [ADR-0047](docs/decisions.md)
and [ADR-0044](docs/decisions.md)).

## Goals

- **Performance first** — Rust core; the UI is a WebView shell over it, not a
  bundled browser runtime.
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
- MySQL / MariaDB (`mysql://…`, its own SQL dialect)
- Cloud Firestore (document store, REST API — read-only)

All eight adapters ship today. The four pg-wire flavors share the
generic `dbboard-postgres` adapter (`sqlx` + `tls-rustls-ring`),
differing only in the runtime label exposed by `DatabaseAdapter::id()`
(`"postgres"`, `"neon"`, `"supabase"`, `"aurora-dsql"`) so the
connection picker and history records can label each connection
precisely. See [ADR-0018](docs/decisions.md) (Neon),
[ADR-0019](docs/decisions.md) (Supabase), and
[ADR-0021](docs/decisions.md) (Aurora DSQL).

MySQL / MariaDB is the first engine on a genuinely different SQL dialect
(back-tick identifiers, back-slash string escapes, `X'…'` blobs, no
`NaN`/`±Inf` in `DOUBLE`): it has its own `dbboard-mysql` adapter over
`sqlx`'s MySQL driver and a distinct `SqlDialect::MySql`. See
[ADR-0068](docs/decisions.md).

Cloud Firestore is the first adapter that is not SQL at all. A query is a
Firestore [`StructuredQuery`](https://firebase.google.com/docs/firestore/reference/rest/v1/StructuredQuery)
written as JSON — the same object Google's docs show, with no translation
layer and no invented query language — and nested documents come back in
`Value::Json` cells. It is **read-only by construction**: the REST API splits
reads from writes at the endpoint, and the crate contains no code able to
build a write URL. It connects with a service-account key (stored in the OS
keychain, never in `connections.toml`) or, with no credential at all, to the
local emulator. See [ADR-0091](docs/decisions.md),
[ADR-0093](docs/decisions.md), and [ADR-0094](docs/decisions.md).

Aurora DSQL also has a second connection kind, `aurora-dsql-iam`
([ADR-0036](docs/decisions.md)): instead of a manually supplied URL
whose token expires in ~15 minutes, dbboard mints the SigV4 IAM token
itself from stored AWS credentials, for connections that need to stay
up 24/7. A background task re-mints the token and swaps in a freshly
authenticated pool before it expires
([ADR-0037](docs/decisions.md), 段階B), so an unattended connection
survives Aurora DSQL's idle recycle without a manual reconnect.

The Supabase REST/auth layer is deliberately deferred to a future ADR —
at this stage all pg-wire flavors use the same `postgres://…` URL
contract (the `aurora-dsql-iam` kind excepted, which stores AWS
credentials rather than a URL).

The authoritative per-version support matrix (Tier 1 / Tier 2 / best
effort) lives in [`docs/compatibility.md`](docs/compatibility.md);
versioning and DB-support policy are defined in
[ADR-0011](docs/decisions.md).

## Architecture

Three main layers, organised as a cargo workspace:

- **UI layer** — a Tauri 2 shell (`apps/desktop`) with a SvelteKit frontend,
  calling the core crates over Tauri IPC.
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
cd apps/desktop
pnpm install        # first time only
pnpm tauri dev
```

The frontend never touches a database: it calls Tauri commands that wrap the
same `McpService` the MCP server exposes, so the engine-enforced read-only
path has one implementation. Nothing listens on a network interface. (The
loopback HTTP backend in `crates/dbboard-server` is the executable statement
of the contract the web sibling implements — see
[`docs/api-contract.md`](docs/api-contract.md) — and is no longer booted by
the client.)

By default the app opens an in-memory Turso/libSQL database, so it runs
with no configuration. The backend is chosen by, in priority order:

1. The environment variables documented below
   (`DBBOARD_AURORA_DSQL_URL` > `DBBOARD_NEON_URL` >
   `DBBOARD_SUPABASE_URL` > `DBBOARD_PG_URL` > `DBBOARD_MYSQL_URL` >
   `DBBOARD_D1_*` > `DBBOARD_FIRESTORE_PROJECT_ID` >
   `DBBOARD_TURSO_PATH`). Among the four pg-wire flavors
   the order is
   alphabetical — setting two flavored vars at once is unusual but
   the precedence is fully defined.
2. `DBBOARD_CONNECTION=<id>` resolved against `connections.toml` — the
   local connection store backed by your OS keychain (ADR-0013).
3. If `connections.toml` has exactly one entry, that one is auto-selected.
4. Otherwise an in-memory Turso/libSQL database.

See [`docs/connections.md`](docs/connections.md) for the connection-store
schema and where the file lives per OS.

Once registered, the **Connections** window (top bar) lets you
add / edit / delete entries and swap the active connection on the
running process — the per-row **Connect** button swaps the backend
in-place, no app restart needed (in-flight requests intentionally
finish on the old backend; new ones pick up the new one). See
[ADR-0020](docs/decisions.md) for the swap semantics.

The same window can **Export** all connections to a passphrase-encrypted
`.dbbx` bundle (`age` scrypt + ChaCha20-Poly1305) that carries the
connection metadata **and** its secrets, and **Import** one on another
machine — the one-file way to move a whole setup without hand-seeding the
keychain. Import is skip-and-report on id/reference conflicts. See
[ADR-0038](docs/decisions.md) and
[`docs/connections.md`](docs/connections.md#moving-connections-between-machines-encrypted-bundle).

A connection to a database that only listens on a bastion's `localhost`
can reach it through an **SSH tunnel**: the desktop connection form has an
**SSH tunnel** section (for the Postgres family and MySQL) to set the
bastion host/port/user, key- or password-based auth, and a mandatory
server host-key pin (fingerprint or `known_hosts`). dbboard opens a
pure-Rust local port forward (russh) and rewrites the connection URL to the
tunnel before dialing; the key passphrase / SSH password live only in the
keychain. See [ADR-0069](docs/decisions.md) and
[`docs/connections.md`](docs/connections.md#ssh-tunnel-connectionsssh).

The top bar carries a **Language** / **言語** menu listing
the 11 shipped locales by their native names. Picking one swaps the
UI language in place; the `DBBOARD_LANG` env var still drives the
startup default and is unchanged by the runtime switcher. See
[ADR-0022](docs/decisions.md).

The **Help** menu shows the running version and, when a newer release has
been published, an **Update available** notice with the release notes. This is
the one network call dbboard makes on its own behalf: a single best-effort
check at startup against the signed `latest.json` published with each release,
compared to the running version. You choose whether to install it; the
download is verified against the release signing key before anything is
replaced ([ADR-0067](docs/decisions.md)). The check is silent when offline or
on any error, and you can turn it off completely by setting
`DBBOARD_NO_UPDATE_CHECK` to any non-empty value. See
[ADR-0040](docs/decisions.md).

A **Backup…** button on the query toolbar dumps the active connection to a
single self-contained `.sql` file — schema plus data, in the source engine's
own dialect (verbatim DDL for the Turso/D1 SQLite family, reconstructed DDL
for the Postgres family including Neon, Supabase, and Aurora DSQL). The dump
streams `INSERT` batches straight to disk with keyset pagination, so a large
database never buffers in memory. Before starting, a preflight row count warns
if the database is bigger than a threshold (500k rows by default, adjustable
under the **Backup** menu and remembered across restarts) so a giant dump
isn't kicked off blindly; while running, a window shows table/row progress and
a percentage bar and can **Cancel** at any time (the file keeps the partial
dump). See [ADR-0049](docs/decisions.md) and [ADR-0050](docs/decisions.md) (the
configurable threshold).

A **Restore…** button on the same toolbar plays a `.sql` file back into the
active connection — the read side of the backup above, and it also accepts
foreign scripts (`pg_dump`, `sqlite3 .dump`). It runs whole-script against an
engine of the matching family (SQLite for Turso/D1, Postgres for Neon,
Supabase, and Aurora DSQL) and applies statements inside a transaction where
the engine supports one (Aurora DSQL falls back to per-statement). To stay
safe, restore targets **empty** databases: loading into a connection that
already holds tables raises a confirmation rather than merging or diffing.
A window shows per-statement progress and can **Cancel** mid-run; the summary
reports how many schema and data statements applied and which failed. See
[ADR-0051](docs/decisions.md).

The examples below set an env var for one run; they assume the working
directory is `apps/desktop` (that is where `pnpm tauri dev` lives). Anything
you would rather keep is better placed in `connections.toml` — see
[docs/connections.md](docs/connections.md).

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
  pnpm tauri dev
```

### Cloud Firestore

The project id alone is enough to select Firestore. Everything else is
optional, which makes the emulator the zero-configuration case:

| Variable | Purpose |
|---|---|
| `DBBOARD_FIRESTORE_PROJECT_ID` | Google Cloud project id. Selects the Firestore backend. |
| `DBBOARD_FIRESTORE_SERVICE_ACCOUNT` | _(optional)_ The service-account key, as the JSON file's whole contents. **Omit it to talk to the local emulator**, which authenticates every request as `owner`. |
| `DBBOARD_FIRESTORE_DATABASE_ID` | _(optional)_ Named database; defaults to the project's `(default)`. |
| `DBBOARD_FIRESTORE_BASE_URL` | _(optional)_ API root override; defaults to `https://firestore.googleapis.com`. This is where the emulator's address goes. |

A service-account key is never sent over plain HTTP: an `http://` base URL
is refused outright when one is configured, and the client is built
`https_only` so a redirect cannot route around it. The emulator is exempt
because it issues no credential to leak.

```sh
# The local emulator — no credential at all.
DBBOARD_FIRESTORE_PROJECT_ID=demo-project \
DBBOARD_FIRESTORE_BASE_URL=http://127.0.0.1:8080 \
  pnpm tauri dev

# A real project. The key is a secret: keep it out of version control.
DBBOARD_FIRESTORE_PROJECT_ID=my-project \
DBBOARD_FIRESTORE_SERVICE_ACCOUNT="$(cat key.json)" \
  pnpm tauri dev
```

Queries are Firestore `StructuredQuery` objects in JSON, not SQL — the
sidebar's *Select top 100* generates one for you:

```json
{
  "from": [{ "collectionId": "users" }],
  "limit": 100
}
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
  pnpm tauri dev
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
  pnpm tauri dev

DBBOARD_SUPABASE_URL='postgres://user:pass@db.<ref>.supabase.co:5432/postgres?sslmode=require' \
  pnpm tauri dev

# Aurora DSQL: the password segment is a short-lived IAM token.
DBBOARD_AURORA_DSQL_URL='postgres://admin:<IAM-token>@<cluster>.dsql.<region>.on.aws:5432/postgres?sslmode=require' \
  pnpm tauri dev
```

Because the Aurora DSQL IAM token expires in ~15 minutes, the env-var
form above suits short sessions. For a long-lived / 24/7 connection use
the `aurora-dsql-iam` connection kind instead, which stores AWS
credentials in `connections.toml` + the OS keychain and lets dbboard
mint the token itself. It is configured in the connection store, not via
an env var — see [docs/connections.md](docs/connections.md) and
[ADR-0036](docs/decisions.md).

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

Responses stream incrementally for providers that advertise the
capability (both wired providers do — the Anthropic provider per
ADR-0026, the OpenAI provider per ADR-0052). Text
appears chunk by chunk as the model generates it, and a running
**Tokens: N in / M out** meter updates from the cumulative usage
chunks. While a request is in flight the Send button is replaced
with **Cancel**: clicking it drops the in-flight stream (closing the
HTTP connection so the server stops generating) while preserving any
partial text already shown, and a quiet *Cancelled.* line marks the
outcome. The same cancel button works on the atomic path used by
providers without streaming support.

In Suggest mode, an **Include column details** checkbox appears when
the active database adapter can describe tables (all three bundled
adapters can — ADR-0028). Ticking it makes Send first fetch each
table's columns and primary key from the live connection, then embed
that schema in the prompt, which largely eliminates hallucinated
column names in the drafts. The trade-off is prompt size: the full
schema counts against the provider's context window and your token
bill — watch the token meter, and leave the box off (its default,
reset each session) for large schemas or metered keys. If some tables
fail to describe, a warning shows the count and the suggestion
proceeds with the rest.

Two providers are wired today — the **Anthropic** Messages API and
the **OpenAI** Chat Completions API (ADR-0052, default model
`gpt-4o`). Both are configured via **either** of two paths, evaluated
in the order below; the first to resolve wins.

#### 1. `ai-providers.toml` + OS keychain (recommended)

Add one or more entries to a per-user `ai-providers.toml` next to
`connections.toml` (path: same OS config dir as the connection store)
and the API key is held in the OS keychain — Windows Credential
Manager, macOS Keychain, or Linux Secret Service — under
`dbboard.ai.<id>.api_key`. The TOML file itself records the keyring
reference, never the literal key.

The runtime store is the same one the Settings UI mutates (open it
from the **AI Providers** menu, ADR-0025 slice b): adding an entry
seeds the keychain, "Use" rebuilds the in-process provider and
updates `active_id` atomically, and "Delete" tears down the entry
and its secret together. A hand-edited TOML works too — useful for
seeding a new machine without opening the window — provided the
`keyring_api_key_ref` it names has a matching keychain entry:

The `kind` discriminator sits inline on each `[[providers]]` entry
(`"anthropic"` or `"openai"`); `model` is optional and falls back to
the provider's default when omitted:

```toml
# ai-providers.toml
version = 1
active_id = "primary"

[[providers]]
id = "primary"
name = "Anthropic"
kind = "anthropic"
model = "claude-sonnet-5"             # optional; omitted = default
keyring_api_key_ref = "dbboard.ai.primary.api_key"

[[providers]]
id = "gpt"
name = "ChatGPT"
kind = "openai"
model = "gpt-4o"                      # optional; omitted = gpt-4o
keyring_api_key_ref = "dbboard.ai.gpt.api_key"
```

The env-variable path below stays Anthropic-only; configure OpenAI
through `ai-providers.toml` (or the **AI Providers** Settings window).

#### 2. Environment variables (back-compat / CI)

| Variable | Purpose | Default |
|---|---|---|
| `DBBOARD_ANTHROPIC_API_KEY` | API key from the Anthropic console. Sets the panel up without touching `ai-providers.toml`. | _(unset = fall through to TOML)_ |
| `DBBOARD_ANTHROPIC_MODEL` | Model identifier override. | `claude-sonnet-5` |

When `DBBOARD_ANTHROPIC_API_KEY` is set, it **always wins** over
`ai-providers.toml`. This preserves the original Stage 1 wiring for
headless / CI use and avoids surprising a user who already exports
the env var. Unset it to let the TOML path take over:

```sh
DBBOARD_ANTHROPIC_API_KEY='sk-ant-…' pnpm tauri dev
```

If neither path resolves a provider (or any branch fails — missing
keyring entry, construction error), the binary logs to stderr and
continues without AI; the panel and menu entry are hidden. The key
never appears in `Debug` output or in `history.jsonl`; it is held
only in memory for the process lifetime.

Every completed AI call (streamed or atomic, `ok` / `error` /
`cancelled`) is recorded to `history.jsonl` as a `kind: "ai"` record
alongside SQL history (schema v:2 per ADR-0027). The prompt and
response are written **verbatim** — same stance as the SQL text in
v:1 query records. The at-rest protection is the same too: `0o600`
on Unix / user-only DACL on Windows, per ADR-0024. On a shared
machine, be aware that anything typed into the AI panel — including
schema-context in follow-ups and any secrets pasted into an
`Explain` request — lands unredacted on disk under your user account.

Remaining deferred Stage 2 capability (full-DDL schema snapshots +
function-calling — Group D) is tracked in ADR-0023 §9. Groups A / B
/ C are closed (ADR-0025 / ADR-0026 / ADR-0027).

## MCP server

Besides being an AI *client*, dbboard also ships as a headless
[MCP](https://modelcontextprotocol.io) *server* — `dbboard-mcp` — that
hands the databases dbboard is already configured with to an external AI
agent (Claude Desktop, Claude Code) as a small tool surface over stdio.
It reuses the exact same `connections.toml` + OS keychain machinery as
the GUI, so it adds no new place to keep credentials. See
[ADR-0046](docs/decisions.md) and the crate README,
[`crates/dbboard-mcp/README.md`](crates/dbboard-mcp/README.md), for the
full spec.

Nine fixed tools. Seven read — `list_connections`, `list_tables`,
`describe_table`, `search_schema` (ADR-0053), `list_relationships`
(ADR-0054), `run_read_query`, and `get_annotations` (dbboard's local
notes, ADR-0045) — plus `run_write` and `dump_database` (ADR-0087).
The security posture is the reason it is safe to point an agent at:

- **Secrets never cross the wire.** The only connection metadata
  serialized is `{ id, name, kind }` — no URLs, tokens, or keyring
  references, and no error message embeds one.
- **Reading is engine-enforced read-only, not string-matched.**
  Postgres-wire runs each statement inside `BEGIN TRANSACTION READ ONLY`,
  libSQL/Turso under `PRAGMA query_only`, and D1 classifies the AST — so
  `UPDATE`, DDL, multi-statement batches, and `SELECT … FOR UPDATE` all
  fail at the source.
- **Writing is off until a human turns it on, per connection**
  (`mcp_write` in `connections.toml`, or *Connections → Edit → AI agent
  access*). Even then `run_write` only accepts an allowlisted statement
  (`INSERT` / `UPDATE` / `DELETE` / `MERGE`, `CREATE TABLE` / `VIEW` /
  `INDEX` / `SCHEMA`, `ALTER TABLE`) — classified on the AST, failing
  closed — and `GRANT` / `REVOKE` / `DENY`, user and role DDL,
  `SET PASSWORD`, `TRUNCATE`, and `DROP` of anything at all are refused
  whatever the flag says (ADR-0087).
- **Result sets are bounded.** `run_read_query` clamps `max_rows` to a
  hard cap of 1000 (default 200) with a `truncated` flag.
- **stdout is sacred.** JSON-RPC frames own stdout; all logging goes to
  stderr (`RUST_LOG`, default `info`).

**Get it from the
[latest release](https://github.com/meta-taro/dbboard/releases/latest)** —
`dbboard-mcp-windows-x86_64.exe` or `dbboard-mcp-macos-universal`. It is a
single executable with no runtime dependencies, and it is a *separate*
download from the desktop app: the installer on the download page does not
contain it. Put it somewhere stable, because the path goes into the agent's
config:

```powershell
# Windows (PowerShell)
mkdir -Force "$env:LOCALAPPDATA\dbboard"
Move-Item -Force ~\Downloads\dbboard-mcp-windows-x86_64.exe "$env:LOCALAPPDATA\dbboard\dbboard-mcp.exe"
```

```sh
# macOS
mkdir -p ~/.local/bin
mv ~/Downloads/dbboard-mcp-macos-universal ~/.local/bin/dbboard-mcp
chmod +x ~/.local/bin/dbboard-mcp
xattr -d com.apple.quarantine ~/.local/bin/dbboard-mcp   # Gatekeeper, unsigned build
```

Register it with **Claude Code** in one command — `--scope user` makes it
available in every project on the machine:

```sh
# macOS / Linux
claude mcp add dbboard --scope user -- "$HOME/.local/bin/dbboard-mcp"
```

```powershell
# Windows (PowerShell)
claude mcp add dbboard --scope user -- "$env:LOCALAPPDATA/dbboard/dbboard-mcp.exe"
```

To build it yourself instead:

```sh
cargo build --release -p dbboard-mcp
# binary at target/release/dbboard-mcp(.exe)
```

Or with **Claude Desktop**, by adding the absolute path to the built
binary in `claude_desktop_config.json`
(`%APPDATA%\Claude\claude_desktop_config.json` on Windows,
`~/Library/Application Support/Claude/claude_desktop_config.json` on
macOS):

```jsonc
{
  "mcpServers": {
    "dbboard": {
      "command": "C:\\path\\to\\dbboard-mcp.exe"
    }
  }
}
```

Either way, **restart the agent afterwards.** Each client spawns its own
`dbboard-mcp` process and holds it for the session, so an already-running
agent keeps talking to the old one — and restarting the dbboard *desktop
app* does nothing to it, because they are separate processes.

With no arguments it reads the same per-user `connections.toml` the GUI
uses; pass `--config` (or set `DBBOARD_CONFIG`) to point at a curated,
least-privilege subset instead. That is the only thing the environment
decides: a tool call names a `connection_id`, and the server resolves it
against the store file and the keychain, so a connection the store does
not describe cannot be reached. Full configuration — the per-OS config
paths, running several agents at once, TLS behind a corporate proxy, and
the literal error strings a failed connection produces — is documented in
[`crates/dbboard-mcp/README.md`](crates/dbboard-mcp/README.md).

A walkthrough in Japanese, from downloading the binary to the first
refused `DROP`, is on Zenn:
[Claude Code に自分の DB を触らせる](https://zenn.dev/dokokade/articles/46b8c608715963).

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

## Packaging & distribution

Windows and macOS bundles are produced by the Tauri CLI from
`apps/desktop`. Linux is not built today.

> **Note on trust.** The artifacts are **not code-signed yet**, so Windows
> SmartScreen and macOS Gatekeeper will warn about an unknown publisher.
> Verify a download against the published `SHA256SUMS.txt` (see *Release
> builds & checksums*). Signing is a planned follow-up ([ADR-0044](docs/decisions.md)).
> This is separate from the *updater* signing key, which is already in use and
> is what proves an auto-update came from this project ([ADR-0067](docs/decisions.md)).

### Building a bundle locally

```sh
cd apps/desktop
pnpm install          # first time only
pnpm tauri build
```

The bundles land under `apps/desktop/src-tauri/target/release/bundle/`:

| Platform | Artifact | Built on |
|---|---|---|
| Windows | `nsis/dbboard-desktop_<version>_x64-setup.exe` | Windows |
| macOS | `dmg/dbboard-desktop_<version>_universal.dmg` | macOS (a `.dmg` cannot be produced from Windows) |

App identity — bundle identifier, icons, window defaults, updater endpoint —
lives in [`apps/desktop/src-tauri/tauri.conf.json`](apps/desktop/src-tauri/tauri.conf.json);
the icons are generated from `assets/` (see [DESIGN.md](DESIGN.md)).

Because the frontend is a static SPA, `pnpm tauri build` runs the Vite build
first; a stale `apps/desktop/build/` is never what ships.

### Release builds & checksums

Pushing a `v*.*.*` tag runs [`.github/workflows/release.yml`](.github/workflows/release.yml),
which builds the Windows and macOS bundles on their native runners and attaches
them to the matching GitHub Release together with a combined `SHA256SUMS.txt`
and the signed `latest.json` the in-app updater reads. A manual
`workflow_dispatch` run builds the same artifacts as a smoke test without
publishing. The [download page](https://meta-taro.github.io/dbboard/) then
serves whatever the newest release holds — it reads the Releases API at load,
so it needs no separate deploy. Verify a download:

```sh
sha256sum -c SHA256SUMS.txt        # Linux/macOS
# or on Windows PowerShell:
#   (Get-FileHash .\dbboard-desktop_<version>_x64-setup.exe -Algorithm SHA256).Hash
```

### Handing a build to someone

Three role-specific guides cover the actual handoff:

- **Producing a build** (maintainer): build the installer, optionally export an
  encrypted connection bundle, deliver, and keep artifacts out of the
  public repo — [`docs/maintainer/internal-distribution.md`](docs/maintainer/internal-distribution.md).
- **Trying it as a tester**: install, run, and report feedback —
  [`docs/internal-testing.md`](docs/internal-testing.md).
- **Standing up the three fixed data-collection connections** (operator):
  [`docs/collector-setup/README.md`](docs/collector-setup/README.md).

Connections and their secrets move between machines as a single
passphrase-encrypted `.dbbx` bundle (Export / Import in the connection
window, [ADR-0038](docs/decisions.md)); the passphrase always travels on a
separate channel.

## Contributing

This project follows the rules in [`CLAUDE.md`](CLAUDE.md). In short:

1. Write a failing test before changing behaviour.
2. Keep changes small and focused.
3. Use conventional-style commit messages in English.
4. Record non-trivial decisions in [`docs/decisions.md`](docs/decisions.md).

## License

See [`LICENSE`](LICENSE).
