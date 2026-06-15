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
   spinning up egui.

## Crate Map

```
dbboard/
├── apps/
│   └── dbboard/            # binary; boots local server + UI in one process
└── crates/
    ├── dbboard-core/       # domain: traits, types, errors (no I/O; serde only)
    ├── dbboard-config/     # connections.toml + OS keychain (ADR-0013)
    ├── dbboard-i18n/       # Fluent bundles (ADR-0015/0022)
    ├── dbboard-turso/      # adapter: Turso / libSQL
    ├── dbboard-d1/         # adapter: Cloudflare D1 (REST)
    ├── dbboard-postgres/   # adapter: PostgreSQL-wire (CockroachDB + Neon /
    │                       #   Supabase / Aurora DSQL via the flavor field —
    │                       #   ADR-0018/0019/0021)
    ├── dbboard-server/     # local axum HTTP backend (ADR-0006)
    ├── dbboard-ai/         # AI provider trait + value types (ADR-0023)
    └── dbboard-ui/         # egui views; HTTP client of dbboard-server
```

As of the latest `develop`, `dbboard-core`, `dbboard-config`,
`dbboard-i18n`, `dbboard-turso`, `dbboard-d1`, `dbboard-postgres` (with
its three pg-wire flavors), `dbboard-server`, `dbboard-ui`, `dbboard-ai`
(trait crate; landed via PR #20 on 2026-06-15), and `apps/dbboard` all
ship. The UI talks to the server over HTTP rather than calling adapters
directly; `apps/dbboard` boots both in one process. The first concrete
AI provider (`dbboard-anthropic`) and the UI panel that drives it
follow in later PRs against issue 0005 — see [`roadmap.md`](roadmap.md).

## Dependency Rules

Strictly enforced via cargo workspace edges:

```
apps/dbboard
   ├──> dbboard-ui ────────────────┐         (HTTP client of dbboard-server)
   ├──> dbboard-server ────────────┤
   │       ├──> dbboard-turso ─────┤
   │       ├──> dbboard-d1 ────────┤──> dbboard-core
   │       └──> dbboard-postgres ──┤
   └──> (dbboard-anthropic) ───────┤         (concrete AI providers live alongside
            └──> dbboard-ai ───────┘          the binary; in-process, no HTTP)
```

The AI layer sits next to the binary, not under `dbboard-server`:
`apps/dbboard` constructs `Option<Arc<dyn AiProvider>>` at startup and
hands it to the UI worker directly. AI calls do not traverse the HTTP
contract ([ADR-0023](decisions.md)).

- `dbboard-core` depends on nothing in this workspace (it derives
  `serde` for the wire format, which is pure data transformation, not
  I/O).
- Adapter crates depend on `dbboard-core` only.
- `dbboard-server` depends on `dbboard-core` and the concrete adapter
  crates (it is the only place that knows the full adapter set; since
  Phase 1.5 `apps/dbboard` reaches them only transitively through it).
- `dbboard-ui` depends on `dbboard-core` among workspace crates only. It
  talks to the local server **over HTTP** (via external crates `reqwest`
  / `tokio`), not via direct function calls.
- `dbboard-ai` (trait crate) depends on `dbboard-core` only — for
  `TableInfo`, which is re-exported so concrete providers do not need
  a direct `dbboard-core` dep. No I/O, no async runtime at runtime
  (`tokio` is a dev-only dep for trait tests).
- Concrete AI providers (`dbboard-anthropic`, future peers) depend on
  `dbboard-ai` only — never on `dbboard-server`, `dbboard-ui`, or each
  other.
- `apps/dbboard` boots `dbboard-server` (binding to `127.0.0.1:0`,
  reading back the assigned port) and starts `dbboard-ui` with that
  port. On exit it shuts the server down cleanly. When the AI layer
  is enabled, the binary additionally constructs the chosen provider
  and passes `Option<Arc<dyn AiProvider>>` to the UI worker.

This means new DB support is added by writing one crate that implements
the trait, then wiring it into `dbboard-server`. No UI or core changes
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

    /// Execute a SQL query and return a typed result.
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

## AI Layer (optional)

A separate trait in `dbboard-ai` that mirrors the adapter pattern. The
trait crate is in `develop` as of PR #20 (2026-06-15); concrete
providers (`dbboard-anthropic`) and the UI panel follow in subsequent
PRs ([ADR-0023](decisions.md);
`.claude/issues/0005-dbboard-ai-trait-and-anthropic-provider.md`).

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
peers) depend on `dbboard-ai` only — never on `dbboard-ui` or on
each other.

The UI calls `Option<Arc<dyn AiProvider>>`. When `None`, AI-related
controls are hidden or disabled.

## Async Runtime

Two `tokio` runtimes coexist without nesting (ADR-0009):

- **Server runtime** — a multi-thread runtime owned by `apps/dbboard`'s
  `main`. It drives `dbboard-server` for the whole process lifetime and
  is shut down after the UI exits.
- **UI worker runtime** — a current-thread runtime owned by
  `dbboard-ui`'s background worker thread. It runs the `reqwest` HTTP
  client. egui itself runs synchronously on the main thread and never
  blocks on I/O; the worker bridges back via `Command`/`Reply` channels
  and wakes the UI with `egui::Context::request_repaint`.

Because the two runtimes live on different threads, there is no
`block_on`-within-`block_on` hazard.

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

The egui UI and the loopback server communicate over the JSON HTTP API
defined in [`api-contract.md`](api-contract.md) — the canonical contract
shared with `dbboard-web` (ADR-0009). The server is unauthenticated by
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
  on Unix.
- **Secrets** in the OS keychain via the `keyring` crate (Windows
  Credential Manager, macOS Keychain, Linux Secret Service). The TOML
  stores only opaque `keyring_*_ref` keys; tokens and connection
  strings never appear on disk.

`apps/dbboard::main` resolves a backend in this order:
`DBBOARD_PG_URL` → `DBBOARD_D1_*` → `DBBOARD_TURSO_PATH` →
`DBBOARD_CONNECTION=<id>` from `connections.toml` → single-entry
auto-select → default Turso `:memory:`. The config layer is purely
additive; existing env-driven flows are unchanged.

## Testing Strategy

- `dbboard-core`: pure unit tests, no I/O.
- Adapters: integration tests against real local instances where
  feasible (e.g. embedded libSQL). Network-bound tests are gated behind
  an env var.
- `dbboard-ui`: view-model tests; egui rendering is not unit-tested.

## Parity with `dbboard-web`

Where it does not cost us, names and shapes should match the web
counterpart to make documentation reusable:

- Adapter identifiers (`turso`, `neon`, `supabase`) are stable strings.
- Error categories align with the web service's error contract.
- Schema snapshot shape is informally aligned (documented in this file
  and the web repo's equivalent doc).

Breaking changes to any of the above are recorded as ADRs in both
repos.
