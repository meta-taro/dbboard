# dbboard

A high-performance desktop database client for modern serverless and
distributed databases.

dbboard is a learning and reference project that explores multi-database
integration, local-first tooling, and pluggable AI-assisted workflows. It
exposes a unified, native UI for Neon, Supabase, and Turso/libSQL, with an
adapter-based architecture that makes adding new databases straightforward.

## Status

Early development. See [`docs/roadmap.md`](docs/roadmap.md) for the current
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
- Neon (PostgreSQL)
- Supabase (PostgreSQL + API)

The Turso adapter ships first, followed by Cloudflare D1 (over its REST
API) and CockroachDB (over the PostgreSQL wire protocol, via a generic
`dbboard-postgres` adapter). Neon reuses the same Postgres adapter and
Supabase follows with its REST/auth layer once the adapter trait is
extracted (see [`docs/roadmap.md`](docs/roadmap.md)).

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
with no configuration. The backend is chosen from the environment (see
[`.env.example`](.env.example)):

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

Set a single connection string to connect to CockroachDB (or any
PostgreSQL-wire database, e.g. Neon) via the generic `dbboard-postgres`
adapter:

| Variable | Purpose |
|---|---|
| `DBBOARD_PG_URL` | Full connection string, e.g. `postgresql://user:pass@host:26257/db?sslmode=verify-full` |

For **CockroachDB Cloud**, copy the connection string from the cluster's
**Connect** dialog in the CockroachDB Cloud Console (Basic free tier
works). For a **self-hosted** node started with
`cockroach start-single-node`, use its `postgresql://…` string; the
default SQL port is `26257`. CockroachDB requires TLS, so keep
`sslmode=verify-full` (or the mode your deployment expects).

`DBBOARD_PG_URL` takes precedence over the D1 and Turso variables. The
connection string contains your password — keep it out of version
control (use `.env`, which is gitignored). The app never logs it.

```sh
DBBOARD_PG_URL='postgresql://user:pass@host:26257/db?sslmode=verify-full' \
  cargo run -p dbboard
```

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
