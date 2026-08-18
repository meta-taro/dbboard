# Architecture

This document describes the layered architecture of the **desktop**
dbboard implementation. The web sibling
([`dbboard-web`](https://github.com/meta-taro/dbboard-web)) mirrors the
same conceptual layering in TypeScript.

## Goals

1. Keep database-specific code behind a single trait so adding a new DB
   is an isolated change.
2. Keep the AI integration optional and pluggable so the core works
   without it.
3. Keep the UI free of business logic so logic stays testable without
   starting a window.

## Crate Map

```
dbboard/
├── apps/
│   └── dbboard/            # binary; boots local server + UI in one process
└── crates/
    ├── dbboard-core/       # domain: traits, types, errors (no I/O; serde only)
    ├── dbboard-config/     # connections.toml + OS keychain (ADR-0013)
    ├── dbboard-turso/      # adapter: Turso / libSQL
    ├── dbboard-d1/         # adapter: Cloudflare D1 (REST)
    ├── dbboard-postgres/   # adapter: PostgreSQL-wire (CockroachDB + Neon /
    │                       #   Supabase / Aurora DSQL via the flavor field —
    │                       #   ADR-0018/0019/0021)
    ├── dbboard-mysql/      # adapter: MySQL / MariaDB (new SqlDialect —
    │                       #   ADR-0068)
    ├── dbboard-firestore/  # adapter: Cloud Firestore (REST, read-only —
    │                       #   ADR-0091/0093/0094)
    ├── dbboard-mongodb/    # adapter: MongoDB (official driver, read-only —
    │                       #   ADR-0091/0095/0096)
    ├── dbboard-tunnel/     # SSH local port-forward over russh (ADR-0069)
    ├── dbboard-connect/    # connection factory: connections.toml entry +
    │                       #   keyring secret -> connected adapter (ADR-0046);
    │                       #   opens a dbboard-tunnel forward first when the
    │                       #   entry carries an ssh block (ADR-0069)
    ├── dbboard-server/     # local axum HTTP backend (ADR-0006)
    ├── dbboard-mcp/        # headless MCP server over stdio (ADR-0046),
    │                       #   read-only unless a connection opts in (ADR-0087);
    │                       #   also screenshots the app's window (ADR-0108)
    │                       #   and drives it over a file channel (ADR-0109)
    ├── dbboard-ai/         # AI provider trait + value types (ADR-0023)
    ├── dbboard-anthropic/  # AI provider: Anthropic Messages API (ADR-0023)
    └── dbboard-openai/     # AI provider: OpenAI Chat Completions (ADR-0052)

apps/
    └── desktop/            # the client (ADR-0089)
        ├── src/            #   SvelteKit frontend (static SPA in a WebView)
        └── src-tauri/      #   Tauri shell (crate: dbboard-desktop); wraps
                            #   McpService + the core crates as IPC commands
```

Everything above ships. `dbboard-postgres` covers CockroachDB plus the three
pg-wire flavors (Neon / Supabase / Aurora DSQL, ADR-0018/0019/0021), and
`dbboard-mysql` (ADR-0068) was the first genuinely new SQL dialect.

`dbboard-firestore` and `dbboard-mongodb` are the first adapters whose query
text is not SQL at all — a Firestore `StructuredQuery` and a MongoDB command
document, both JSON. They join through the *same* trait rather than a parallel
one (ADR-0091): the trait never promised SQL, only that a string of query text
comes back as rows, and a second hierarchy would have forced every caller —
the client, the MCP server, the history store — to branch on which world a
connection lives in. What is genuinely per-store is the read-only decision, so
each of the two carries its own classifier: Firestore has no write path to
close (ADR-0093), while MongoDB allow-lists command names *and* the options
each command may take, then walks the pipeline for a smuggled `$out` or
`$merge` (ADR-0095).

There are two entry points: the desktop client (`apps/desktop`) and the
headless MCP stdio server (`dbboard-mcp`). Both reach a database the same
way — `dbboard-connect` turns a `connections.toml` entry plus its keyring
secret into a connected adapter, opening a `dbboard-tunnel` forward first when
the entry carries an `ssh` block — and both route reads through the one
`McpService`, so the engine-enforced read-only guarantee has a single
implementation (ADR-0046, write policy ADR-0087).

The AI layer stays optional: the shell constructs `Option<Arc<dyn AiProvider>>`
at startup, missing configuration degrades to `None`, and the assistant UI is
hidden entirely when nothing was wired (ADR-0023 Decision 11, ADR-0052).

`dbboard-server` is the odd one out. It implements the loopback HTTP contract
the web sibling mirrors ([api-contract.md](api-contract.md)), and it was the
transport the retired egui client used; the Tauri client calls Tauri commands
instead, so nothing boots it today. It is kept as the executable statement of
that contract — retiring it would be a separate architecture decision
(ADR-0089).

## Dependency Rules

Strictly enforced via cargo workspace edges:

```
apps/desktop/src (SvelteKit)
   │  Tauri IPC
   v
apps/desktop/src-tauri  (crate: dbboard-desktop)
   ├──> dbboard-mcp ───────────────┐         (McpService: the read path)
   ├──> dbboard-connect ───────────┤
   │       ├──> dbboard-config ────┤
   │       ├──> dbboard-tunnel     │         (leaf: russh only)
   │       ├──> dbboard-turso ─────┤
   │       ├──> dbboard-d1 ────────┤──> dbboard-core
   │       ├──> dbboard-postgres ──┤
   │       └──> dbboard-mysql ─────┤
   └──> (dbboard-anthropic,        │         (concrete AI providers live alongside
         dbboard-openai) ──────────┤          the shell; in-process, no HTTP)
            └──> dbboard-ai ───────┘

crates/dbboard-server ──> dbboard-connect     (the HTTP contract; unbooted, ADR-0089)
crates/dbboard-mcp     ──> dbboard-connect    (also a standalone stdio binary)
```

The AI layer sits next to the shell, not under an adapter: the shell
constructs `Option<Arc<dyn AiProvider>>` at startup and calls it directly
([ADR-0023](decisions.md)).

- `dbboard-core` depends on nothing in this workspace (it derives
  `serde` for the wire format, which is pure data transformation, not
  I/O).
- Adapter crates depend on `dbboard-core` only.
- `dbboard-connect` depends on `dbboard-core`, `dbboard-config`,
  `dbboard-tunnel`, and the concrete adapter crates. It is the single
  place that turns a `connections.toml` entry plus its keyring secret
  into a connected `Arc<dyn DatabaseAdapter>` (`backend_config_for_entry`
  + `connect_adapter`), extracted from `dbboard-server` so a second entry
  point can reuse the exact, security-sensitive construction without
  pulling in axum ([ADR-0046](decisions.md)). When the entry carries an
  `ssh` block it opens a `dbboard-tunnel` local forward first and
  rewrites the URL host/port to the tunnel's local end before dialing the
  adapter ([ADR-0069](decisions.md)).
- `dbboard-tunnel` depends on **nothing in this workspace** (only `russh`,
  `tokio`, and its own `thiserror` error type). It wraps `russh` to open
  an authenticated SSH connection to a bastion and forward a local port to
  the DB's `host:port`, verifying the server host key against a pinned
  fingerprint or a `known_hosts` file — never blindly
  ([ADR-0069](decisions.md)).
- `dbboard-server` depends on `dbboard-connect` (and `dbboard-core`) and
  owns the HTTP contract. No client boots it since the egui client was
  retired (ADR-0089); it is kept as the executable statement of the
  contract dbboard-web mirrors.
- `dbboard-mcp` depends on `dbboard-connect`, `dbboard-core`, and
  `dbboard-config`. It is both a **standalone stdio binary** serving a
  read-only tool surface to an external AI agent, and a **library** whose
  `McpService` the desktop shell reuses for its own read path — one
  implementation of the read-only guarantee, two transports
  ([ADR-0046](decisions.md)). Its `capture` module is the one part that
  belongs to neither layer: it holds no service state and talks to the
  windowing system rather than a database, so that `capture_window` can
  photograph the desktop app's own window ([ADR-0108](decisions.md)). It
  also *drives* that window, through two files in the config directory —
  one written by each side, sequence-numbered so an instruction fires once
  and is answered on completion ([ADR-0109](decisions.md)). That channel
  crosses a process boundary the dependency graph does not show: the shell
  is not a dependency of this crate and never becomes one.
- `dbboard-ai` (trait crate) depends on `dbboard-core` only — for
  `TableInfo`, which is re-exported so concrete providers do not need
  a direct `dbboard-core` dep. No I/O, no async runtime at runtime
  (`tokio` is a dev-only dep for trait tests).
- Concrete AI providers (`dbboard-anthropic`, `dbboard-openai`, future
  peers) depend on `dbboard-ai` only — never on `dbboard-server`, on the
  client, or on each other.
- `apps/desktop/src-tauri` is the only crate that knows there is a UI. It
  resolves the optional AI provider at startup; with no key configured it
  holds `None` and the frontend hides the AI controls.
- The frontend depends on no workspace crate at all. It reaches everything
  through the commands the shell exposes, which is what keeps DB logic out
  of it.

This means new DB support is added by writing one crate that implements
the trait, then wiring it into `dbboard-connect`. No UI or core changes
required.

## Core Trait (sketch)

The trait is extracted in Phase 2. The required surface is small;
per-DB features (views, auth, storage, realtime, …) hang off it as
optional capability traits per [ADR-0012](decisions.md).

```rust
// crates/dbboard-core/src/lib.rs (Phase 2)

#[async_trait::async_trait]
pub trait DatabaseAdapter: Send + Sync {
    /// Identifier used in connection lists and logs.
    fn id(&self) -> &str;

    /// Coarse feature flags for HTTP `/capabilities` discovery.
    fn capabilities(&self) -> Capabilities;

    /// Verify connectivity without running a user query.
    async fn ping(&self) -> Result<(), DbError>;

    /// List schemas / tables / views, suitable for the schema browser.
    async fn introspect(&self) -> Result<SchemaSnapshot, DbError>;

    /// Execute a query and return a typed result. The text is SQL for the
    /// SQL engines and JSON for the document stores — see below.
    async fn query(&self, sql: &str) -> Result<QueryResult, DbError>;

    // Optional capabilities — each defaults to `None`.
    fn views(&self)     -> Option<&dyn ViewIntrospection>     { None }
    fn functions(&self) -> Option<&dyn FunctionIntrospection> { None }
    fn auth(&self)      -> Option<&dyn AuthAdmin>             { None }
    fn storage(&self)   -> Option<&dyn StorageAdmin>          { None }
    fn realtime(&self)  -> Option<&dyn RealtimeChannels>      { None }
}
```

`SchemaSnapshot`, `QueryResult`, `DbError` are concrete types in
`dbboard-core` so the UI never sees adapter-specific types. Adapters
that do not implement a given capability simply leave the accessor at
its `None` default — no code changes elsewhere.

The parameter is named `sql` for the eight SQL engines, but the trait only
ever required *query text*. The two document stores pass JSON through it — a
Firestore `StructuredQuery`, a MongoDB command document — and a caller that
needs to know which is which asks the adapter rather than the parameter, so
the client can generate the right *Select top 100* for a table it is looking
at. Nested documents come back in `Value::Json` cells, which is why the value
type gained a `Json` variant before either adapter landed.

## AI Layer (optional)

A separate trait in `dbboard-ai` that mirrors the adapter pattern.
The trait crate is in `develop` as of PR #20 (2026-06-15); the first
concrete provider `dbboard-anthropic` (Anthropic Messages API over
`reqwest`) followed in PR #22 (2026-06-15). OpenAI joined as a second
provider (ADR-0052). The client resolves one from configuration at startup
and hides the AI panel when it gets `None`
([ADR-0023](decisions.md);
`.claude/issues/0005-dbboard-ai-trait-and-anthropic-provider.md`).
The UI panel is registered only when `has_ai_provider()` returns
true; the worker thread routes `Command::AiExplain` /
`Command::AiSuggest` through `tokio::runtime::block_on(provider.*)`
just like ADR-0020's `ConnectionSwitcher`, surfacing results as
`Reply::AiResponded { text, tokens_in, tokens_out }` or
`Reply::AiFailed { error: AiError }`. The menu entry and the panel
both hide entirely when no provider was wired (ADR-0023 Decision 11
graceful degradation = absence).

```rust
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> AiCapabilities;
    async fn explain(&self, req: &ExplainRequest)
        -> AiResult<AiResponse>;
    async fn suggest_sql(&self, req: &SuggestRequest)
        -> AiResult<AiResponse>;
}
```

`AiCapabilities` is the same flat-bool shape as
`dbboard_core::Capabilities` (all-false default, additive flags as
Stage 2 capabilities land). `SuggestRequest::schema: Vec<TableInfo>`
carries the current `list_tables()` result — full DDL extraction is a
Stage 2 concern. `AiError` is a separate taxonomy from `DbError`
(`Configuration` / `Network` / `Provider` / `Quota` / `Cancelled`);
because AI calls never traverse the HTTP contract, the
prefix-translation rule from ADR-0009 does not apply.

Dependency rule: `dbboard-ai` depends on `dbboard-core` only (for
`TableInfo`, re-exported so concrete providers do not need a direct
`dbboard-core` dep). Concrete providers (`dbboard-anthropic`, future
peers) depend on `dbboard-ai` only — never on the client or on
each other.

The UI calls `Option<Arc<dyn AiProvider>>`. When `None`, AI-related
controls are hidden or disabled.

## Async Runtime

The client runs one `tokio` runtime, owned by Tauri. Every command is an
`async fn`, so a slow query is awaited on that runtime rather than blocking
the WebView — the frontend stays responsive because it is a separate process
boundary, not because the Rust side spawns threads by hand.

This is simpler than the two-runtime arrangement ADR-0009 described for the
retired egui client, which had to bridge a synchronous immediate-mode UI to
async I/O over channels. Nothing nests a `block_on` inside a `block_on`.

## Error Handling

- `dbboard-core` defines `DbError` with stable variants: `Connection`,
  `Query`, `Schema`, `TypeConversion`.
- These map onto HTTP statuses and the `{category, message}` error
  envelope as defined in [`api-contract.md`](api-contract.md);
  `DbError::category` / `from_parts` keep that mapping reversible.
- Adapter-specific errors are mapped at the adapter boundary; the rest
  of the system never sees driver types.
- `thiserror` for definitions, `anyhow` only at the binary boundary if
  needed.

## HTTP Contract

`dbboard-server` speaks the JSON HTTP API defined in
[`api-contract.md`](api-contract.md) — the canonical contract shared with
`dbboard-web` (ADR-0009). The desktop client no longer uses it (ADR-0089);
the description below is what any client of that contract gets. The server is unauthenticated by
design, relying on the loopback bind and an OS-assigned ephemeral port;
widening the bind or persisting the port requires adding a per-launch
secret first.

## Configuration

User-facing configuration lives in a dedicated crate
**`crates/dbboard-config`** (added in Phase 2; see
[ADR-0013](decisions.md)). It owns both halves:

- **Connection metadata** in a per-user TOML file
  (`connections.toml`) under the platform's standard config dir,
  resolved through the `directories` crate. The file is `version = 1`
  with a list of `[[connections]]` entries (`kind = "turso" | "d1" |
  "postgres"`). A missing file yields an empty store; the file is
  created lazily when the UI saves the first entry, with mode `0o600`
  on Unix (routed through `dbboard_config::secure_fs::create_new_user_only`
  per ADR-0024). `history.jsonl` lands the same way and re-tightens
  defensively on every append for files that pre-date the ADR.
- **Secrets** in the OS keychain via the `keyring` crate (Windows
  Credential Manager, macOS Keychain, Linux Secret Service). The TOML
  stores only opaque `keyring_*_ref` keys; tokens and connection
  strings never appear on disk. The OS keychain is unaffected by the
  ADR-0024 at-rest hardening — secrets there are encrypted by the OS
  even on a recovered powered-off disk.

The client resolves a backend in this order:
`DBBOARD_PG_URL` → `DBBOARD_MYSQL_URL` → `DBBOARD_D1_*` →
`DBBOARD_TURSO_PATH` → `DBBOARD_CONNECTION=<id>` from `connections.toml` →
single-entry auto-select → default Turso `:memory:` (the pg-wire flavor
vars `DBBOARD_AURORA_DSQL_URL`/`DBBOARD_NEON_URL`/`DBBOARD_SUPABASE_URL`
outrank the generic `DBBOARD_PG_URL`). The config layer is purely
additive; existing env-driven flows are unchanged.

## Testing Strategy

- `dbboard-core`: pure unit tests, no I/O.
- Adapters: integration tests against real local instances where
  feasible (e.g. embedded libSQL). Network-bound tests are gated behind
  an env var.
- Client: the frontend's pure logic (grid formatting, SQL helpers, query
  state) is unit-tested with vitest; the Tauri commands are covered by Rust
  tests in `apps/desktop/src-tauri`. Rendering is not unit-tested.

## Parity with `dbboard-web`

Where it does not cost us, names and shapes should match the web
counterpart to make documentation reusable:

- Adapter identifiers (`turso`, `neon`, `supabase`) are stable strings.
- Error categories align with the web service's error contract.
- Schema snapshot shape is informally aligned (documented in this file
  and the web repo's equivalent doc).

Breaking changes to any of the above are recorded as ADRs in both
repos.
