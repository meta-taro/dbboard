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
│   └── dbboard/            # binary; wires concrete adapters into the UI
└── crates/
    ├── dbboard-core/       # domain: traits, types, errors (no I/O)
    ├── dbboard-turso/      # adapter: Turso / libSQL
    ├── dbboard-neon/       # adapter: Neon (later)
    ├── dbboard-supabase/   # adapter: Supabase (later)
    ├── dbboard-ai/         # optional AI provider trait + adapters
    └── dbboard-ui/         # egui views and view models
```

Phase 1 ships only `dbboard-core`, `dbboard-turso`, `dbboard-ui`, and
`apps/dbboard`. The other crates land in later phases (see
[`roadmap.md`](roadmap.md)).

## Dependency Rules

Strictly enforced via cargo workspace edges:

```
apps/dbboard
   ├──> dbboard-ui ──┐
   ├──> dbboard-turso│
   ├──> dbboard-neon │──> dbboard-core
   ├──> dbboard-supabase
   └──> dbboard-ai ──┘   (dbboard-ai also depends on core)
```

- `dbboard-core` depends on nothing in this workspace.
- Adapter crates depend on `dbboard-core` only.
- `dbboard-ui` depends on `dbboard-core` only — never on a concrete
  adapter.
- `apps/dbboard` is the only place where concrete adapters meet the UI.

This means new DB support is added by writing one crate that implements
the trait, then wiring it in the app. No UI or core changes required.

## Core Trait (sketch)

The exact signature evolves as Phase 1 progresses. Initial intent:

```rust
// crates/dbboard-core/src/lib.rs

#[async_trait::async_trait]
pub trait DatabaseAdapter: Send + Sync {
    /// Identifier used in connection lists and logs.
    fn id(&self) -> &str;

    /// Verify connectivity without running a user query.
    async fn ping(&self) -> Result<(), DbError>;

    /// List schemas / tables / views, suitable for the schema browser.
    async fn introspect(&self) -> Result<SchemaSnapshot, DbError>;

    /// Execute a SQL query and return a typed result.
    async fn query(&self, sql: &str) -> Result<QueryResult, DbError>;
}
```

`SchemaSnapshot`, `QueryResult`, `DbError` are concrete types in
`dbboard-core` so the UI never sees adapter-specific types.

## AI Layer (optional, later)

A separate trait in `dbboard-ai` that mirrors the adapter pattern:

```rust
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    async fn explain_sql(&self, sql: &str) -> Result<String, AiError>;
    async fn suggest_sql(&self, prompt: &str, schema: &SchemaSnapshot)
        -> Result<String, AiError>;
}
```

The UI calls `Option<Arc<dyn AiProvider>>`. When `None`, AI-related
controls are hidden or disabled.

## Async Runtime

- `tokio` (multi-thread). egui runs on the main thread; adapter calls
  are spawned onto tokio and bridged back via channels.

## Error Handling

- `dbboard-core` defines `DbError` with stable variants (`Connection`,
  `Query`, `Schema`, `Timeout`, `Other`).
- Adapter-specific errors are mapped at the adapter boundary; the rest
  of the system never sees driver types.
- `thiserror` for definitions, `anyhow` only at the binary boundary if
  needed.

## Configuration

- Connections are stored in a local file (TBD: `~/.config/dbboard/config.toml`
  or platform equivalent via the `directories` crate).
- Secrets are stored via the OS keychain (TBD: `keyring` crate).

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
