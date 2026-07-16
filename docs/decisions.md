# Architectural Decision Records

Append-only log of non-trivial technical decisions. Each entry is short:
context, decision, consequences. Do not rewrite past entries — supersede
them with a new entry referencing the old one.

Status values: `accepted`, `superseded`, `deprecated`.

---

## ADR-0001 — Rust + egui for the desktop stack

- **Date**: 2026-05-19
- **Status**: accepted

### Context

The desktop client must feel native, start fast, and run on a tight
resource budget. Web tech inside an Electron shell would conflict with
the project's "performance first" principle, and would duplicate the
web sibling's stack.

### Decision

Build the desktop client in Rust on top of `egui`. Use `tokio` for
async I/O. Bridge between the egui main thread and tokio via channels.

### Consequences

- Native performance and low memory footprint.
- Smaller ecosystem for UI components than web — we accept this for
  the project's scope.
- Cannot share code with `dbboard-web`; only concepts and contracts.

---

## ADR-0002 — Cargo workspace with strict layer crates

- **Date**: 2026-05-19
- **Status**: accepted

### Context

The architecture calls for clear separation between domain, adapters,
UI, and an optional AI layer. We need cargo to enforce this rather than
relying on convention.

### Decision

Use a cargo workspace with the following crate split:

- `crates/dbboard-core` — domain (no I/O)
- `crates/dbboard-<adapter>` — one per database
- `crates/dbboard-ai` — optional AI provider trait + adapters
- `crates/dbboard-ui` — egui views
- `apps/dbboard` — binary; only place that wires concrete adapters and
  UI together

Adapter crates depend only on `dbboard-core`. `dbboard-ui` depends only
on `dbboard-core`. Concrete adapter selection happens in
`apps/dbboard`.

### Consequences

- Adding a new database is a single new crate plus one line in
  `apps/dbboard`.
- Slightly more boilerplate at the start. Acceptable trade-off.

---

## ADR-0003 — Turso-first vertical slice before extracting the trait

- **Date**: 2026-05-19
- **Status**: accepted

### Context

Designing the `DatabaseAdapter` trait up front from three databases we
haven't yet integrated risks getting the abstraction wrong. Rust traits
are particularly painful to change after consumers exist.

### Decision

Ship a vertical slice against **Turso/libSQL** first
(`connect → introspect → query → render`) with Turso-shaped concrete
types. Extract the `DatabaseAdapter` trait in Phase 2 once we have a
real working implementation to base it on.

### Consequences

- Phase 1 may not compile against Neon/Supabase — by design.
- Phase 2 must re-shape internals; UI shape should stay stable.

---

## ADR-0004 — Two repos, shared API contract, separate implementations

- **Date**: 2026-05-19
- **Status**: accepted (revised from initial "shared concepts only")

### Context

dbboard has a desktop (this repo) and a web
([`dbboard-web`](https://github.com/meta-taro/dbboard-web)) implementation.
The maintainer wants the **same backend design** available in both,
without making the desktop client a thin remote client to the web
deployment.

### Decision

Treat the two repos as **independent codebases that share an HTTP API
contract**:

- The HTTP API (endpoint paths, request and response shapes, error
  categories, status codes) is identical across implementations.
- Web's NestJS implementation is the canonical reference for the
  contract; the desktop ships its own Rust re-implementation (axum) of
  the same surface. See ADR-0006.
- Breaking contract changes are drafted in one repo and mirrored to
  the other before either ships against the change.
- Development pace alternates between repos rather than splitting
  focus on the same layer in both at once.

### Consequences

- Each repo stays idiomatic in its own stack (no Node runtime shipped
  with the desktop binary, no Rust required to run the web).
- Feature parity at the HTTP contract level is enforced by the
  contract itself; below the contract each side is free.
- Two implementations of the same API means duplicated work — accepted
  trade-off in exchange for the desktop staying native and
  offline-capable.

---

## ADR-0005 — GitFlow-style branching with `develop` as default

- **Date**: 2026-05-19
- **Status**: accepted

### Context

Both repos already have `develop` set as the default branch with `main`
also present. We need a documented convention so contributors and
agents know where to commit.

### Decision

- `develop` is the integration branch and the default branch.
- `main` is reserved for tagged releases.
- Feature work happens on `feature/<slug>` branched off `develop` and
  merges back via PR.
- Release PRs merge `develop` into `main` and tag the result.

### Consequences

- Slight overhead for solo work compared to trunk-based development.
- Easier to keep `main` always shippable for OSS users who pin to it.

---

## ADR-0006 — Local HTTP backend in the desktop binary

- **Date**: 2026-05-19
- **Status**: accepted

### Context

ADR-0004 commits both repos to the same HTTP API contract. The desktop
must implement that contract locally rather than reaching out to the
web deployment, so that the application:

- Works offline.
- Has no dependency on a hosted service.
- Does not require Node.js to be installed on the user's machine.

### Decision

Ship a local HTTP backend inside the desktop binary, implemented in
Rust:

- New crate **`crates/dbboard-server`** built on `axum` (tokio-native,
  matches the rest of the async stack).
- Bound to **loopback only** (`127.0.0.1`) — never listens on a
  public interface.
- **Port is auto-selected** at startup (`bind 127.0.0.1:0`, read the
  assigned port back from the listener) so multiple instances do not
  clash.
- The egui UI in `crates/dbboard-ui` is an **HTTP client** of this
  local server. It does not call adapters directly.
- Server endpoints, payload shapes, and error categories mirror the
  web NestJS API one-to-one.

### Consequences

- The egui UI is the same shape as a future browser UI would be —
  switching presentations later costs less.
- An HTTP layer sits on the hot path; we accept loopback overhead in
  exchange for contract parity.
- `apps/dbboard` boots both the local server and the egui UI in the
  same process, and tears the server down on UI exit.
- The API contract becomes a load-bearing document. We will pin a
  canonical location for it once Phase 2 begins (likely
  `docs/api-contract.md` in this repo, with `dbboard-web` linking to
  it or vice versa — to be decided in a follow-up ADR).

---

## ADR-0007 — Cloudflare D1 adapter via the REST `/raw` endpoint

- **Date**: 2026-05-21
- **Status**: accepted

### Context

We want dbboard to connect to Cloudflare D1. Unlike Turso/libSQL, D1
has no native driver that a desktop process can use: Cloudflare exposes
D1 to outside callers only through its HTTP REST API (the Workers
binding is Worker-only). So a D1 adapter is fundamentally an HTTP client
rather than a database driver.

D1 offers two query endpoints. `/query` returns rows as JSON objects
(column name → value), which loses column ordering and drops columns
that are `NULL` for every row. `/raw` returns `results.columns` (ordered
names) and `results.rows` (positional arrays), and uses the same shape
for SELECT and DML.

This is the second concrete adapter. ADR-0003 defers extracting the
`DatabaseAdapter` trait until a second adapter exists; D1 is that second
shape, but we keep it a concrete struct here and leave the trait
extraction to Phase 2.

### Decision

- Add `crates/dbboard-d1` implementing a `D1Adapter` whose method
  surface mirrors `TursoAdapter` (`connect` / `ping` / `list_tables` /
  `query`), with no shared trait yet.
- Talk to the **`/raw`** endpoint so column order is preserved and one
  code path serves SELECT and DML (rows from `results.rows`, affected
  count from `meta.changes`). No statement-kind routing is needed.
- Use **`reqwest`** with **`rustls-tls`** (not native TLS) so the build
  carries no system OpenSSL dependency and stays self-contained on
  Windows. Add `serde`/`serde_json` for the request and response shapes.
- Connection parameters (account id, database id, API token, optional
  base URL) come from `DBBOARD_D1_*` environment variables, resolved in
  `apps/dbboard`. A fully configured D1 environment selects the D1
  backend; otherwise the app falls back to the local Turso default.
- The API token is a secret: it is never logged, never placed in the
  request URL, and never embedded in a `DbError` message.

### Consequences

- `reqwest`, `serde`, and `serde_json` enter the dependency tree. Pure
  mapping functions (envelope → `QueryResult`, JSON cell → `Value`) are
  unit-tested without network; a live round-trip test is gated behind
  `DBBOARD_D1_*`.
- D1 column results carry no declared type (the `/raw` payload omits
  it), so `Column.declared_type` is always `None` for D1 — the same
  convention SQLite expressions already use.
- Every D1 call crosses the network; there is no offline/in-memory mode
  for D1 the way `:memory:` exists for Turso. This is inherent to D1.
- Having a second concrete adapter gives Phase 2 a real second shape to
  base the `DatabaseAdapter` trait on (per ADR-0003).

---

## ADR-0008 — Shared `dbboard-postgres` adapter (sqlx + rustls) for PostgreSQL-wire databases; CockroachDB first

- **Date**: 2026-05-21
- **Status**: accepted (revises the per-database crate rule of ADR-0002)

### Context

We want dbboard to connect to **CockroachDB**. CockroachDB is a
distributed SQL (NewSQL) database that speaks the **PostgreSQL wire
protocol**: ordinary Postgres drivers connect to it with a
`postgresql://…` connection string. The same is true of the Neon and
Supabase adapters already on the roadmap (Phase 3) — all three are
Postgres-wire under the hood.

ADR-0002 says "one crate per database". Taken literally that would mean
near-duplicate `dbboard-cockroach`, `dbboard-neon`, and (partly)
`dbboard-supabase` crates that all wrap the same `sqlx-postgres` driver.

A second tension is the domain model: `dbboard-core`'s `Value` has only
the five SQLite storage classes (`Null`/`Integer`/`Real`/`Text`/`Blob`),
while PostgreSQL has a rich type system (`numeric`, `uuid`,
`timestamptz`, `jsonb`, arrays, user-defined types). Decoding arbitrary
user-SQL results with `sqlx`'s type-checked `try_get` would require
enumerating types and enabling several decode features
(`bigdecimal`/`uuid`/`chrono`/`json`).

### Decision

- Add a single **`crates/dbboard-postgres`** crate that targets the
  PostgreSQL wire protocol generically. CockroachDB is its first
  connection target; Neon (and Supabase's SQL path) reuse the same crate
  later. This **revises ADR-0002**: PostgreSQL-wire-compatible databases
  share one adapter crate rather than getting one crate each.
- The adapter is a concrete `PostgresAdapter` mirroring the existing
  surface (`connect` / `ping` / `list_tables` / `query`). The
  `DatabaseAdapter` trait stays deferred to Phase 2 (ADR-0003).
- Use **`sqlx` 0.8** with **`tls-rustls-ring`** (not native TLS), so the
  build carries no system OpenSSL dependency and stays self-contained on
  Windows — matching the `reqwest`/`rustls` choice in ADR-0007.
- **Dynamic decoding via the simple query protocol.** Run statements
  through `sqlx::raw_sql`, which returns every value in its **text**
  representation. Read each cell as a string (`Value::Text`), NULL as
  `Value::Null`. This keeps `dbboard-core` unchanged, is lossless for
  `int8`/`numeric`, and covers every Postgres type without per-type
  decode features. `Column.declared_type` carries the reported Postgres
  type name (e.g. `INT8`, `TIMESTAMPTZ`).
- Connection parameters come from a single **`DBBOARD_PG_URL`**
  connection string (covers CockroachDB Cloud, self-hosted, and Neon
  uniformly, including `sslmode`). It takes precedence over the D1 and
  Turso selection in `apps/dbboard`. The URL embeds a password and is a
  secret: it is never logged, never stored on the adapter, and never
  echoed in a `DbError` (in particular `sqlx::Error::Configuration`,
  which can wrap the URL, is reduced to a fixed message).
- **TLS is hardened on connect.** sqlx defaults an unspecified `sslmode`
  to `Prefer`, which silently falls back to a plaintext connection (and
  sends the password in the clear) when the server does not offer TLS.
  `connect` parses the URL, and upgrades a `Prefer` mode to `Require`
  before connecting. An explicit choice — including `sslmode=disable` for
  a deliberately insecure local node — is honoured unchanged.
- Schema introspection queries `information_schema.tables`, excluding the
  `pg_catalog`, `information_schema`, and CockroachDB-specific
  `crdb_internal` schemas, and reports tables as `schema.table`
  (`TableInfo::qualified`).

### Consequences

- `sqlx` and `futures-util` enter the dependency tree (a heavier set than
  D1's `reqwest`). Pure mapping/error-classification functions are
  unit-tested without a database; a live round-trip test is gated behind
  `DBBOARD_PG_URL`.
- Values are surfaced as text rather than typed scalars (e.g. `SELECT 1`
  yields `Value::Text("1")`). Acceptable for a read-only viewer and
  lossless; native scalar refinement can come later behind the same
  public surface if needed.
- Neon arrives cheaply: pointing `DBBOARD_PG_URL` at a Neon database
  should work through the same adapter, accelerating Phase 3. Supabase
  still needs its REST/auth hybrid layer on top.
- This is the **third** concrete adapter (Turso, D1, Postgres) and the
  first non-SQLite one, giving Phase 2's `DatabaseAdapter` trait a
  genuinely different shape (schemas, typed columns, connection pool) to
  design against.

---

## ADR-0009 — Canonical API contract location; UI owns the HTTP client; serde in `dbboard-core`

- **Date**: 2026-05-22
- **Status**: accepted (resolves the deferred contract-location question
  at the end of ADR-0006)

### Context

ADR-0006 committed the desktop to a loopback `dbboard-server` (axum) that
the egui UI talks to over HTTP, but left three things open:

1. **Where the API contract lives.** ADR-0006 named `docs/api-contract.md`
   as the likely home "to be decided in a follow-up ADR".
2. **Which crate owns the HTTP client.** The UI had to stop calling
   adapters directly, but egui is synchronous and cannot `await`.
3. **How domain types cross the wire.** `dbboard-core`'s types
   (`Value`, `Row`, `QueryResult`, `TableInfo`, `DbError`) had no
   serialization, and the architecture rule says core does "no I/O".

Phase 1.5 forced all three. This ADR records the decisions taken while
implementing it.

### Decision

- **The canonical contract is [`docs/api-contract.md`](api-contract.md)
  in this (desktop) repo.** It is the source of truth for endpoint
  paths, request/response JSON, the `Value` wire encoding, and the error
  envelope. `dbboard-web` mirrors it; breaking changes are drafted here
  and reflected there before either ships (per ADR-0004).
- **`dbboard-ui` owns the HTTP client.** A background worker thread runs
  a `reqwest` client on its own single-threaded `tokio` runtime and
  bridges to the synchronous egui thread through the existing
  `Command`/`Reply` `mpsc` channels — the channels are kept, only their
  far end changed from a direct adapter call to an HTTP call. `reqwest`,
  `tokio`, `serde`, and `serde_json` become `dbboard-ui` dependencies.
  This does **not** break the layering rule of ADR-0002: that rule
  governs *workspace* edges (`dbboard-ui` still depends on no workspace
  crate but `dbboard-core`); external crates were always allowed.
- **`dbboard-core` gains always-on `serde` derives** (not feature-gated).
  Serialization is pure in-memory data transformation, not I/O, so the
  "no I/O" rule is preserved. `Value` uses a hand-written
  `Serialize`/`Deserialize` mapping to native JSON scalars; since JSON
  has no byte type, `Value::Blob` is encoded as a tagged object
  `{"$blob":"<base64>"}` (base64 standard alphabet). `Row` is
  `#[serde(transparent)]` so it serializes as a bare array. `DbError`
  carries `category()` / `message()` / `from_parts()` helpers so it
  round-trips through the `{category, message}` envelope without doubling
  the `Display` prefix.
- **Two tokio runtimes coexist.** `apps/dbboard`'s `main` owns a
  multi-thread runtime that drives the server; the UI worker owns a
  separate current-thread runtime on its own thread. They never nest, so
  there is no `block_on`-within-`block_on` hazard.
- **The server is unauthenticated by design**, relying on the loopback
  bind and an OS-assigned ephemeral port known only to the spawning
  process. If the bind is ever widened beyond `127.0.0.1` or the port is
  persisted across runs, a per-launch secret (e.g. an `X-DBBoard-Token`
  header) must be added first.

### Consequences

- The contract document is load-bearing: any endpoint or shape change is
  a documented change in `docs/api-contract.md` mirrored to `dbboard-web`.
- `dbboard-core` is now serializable everywhere it is used, at the cost
  of a `serde`/`base64` dependency in the domain crate. The blob
  encoding is a fixed part of the contract.
- The UI keeps working synchronously; a transport failure (server
  unreachable) surfaces as a `Connection` error in the UI rather than a
  hang.
- `apps/dbboard` no longer reads any `DBBOARD_*` database variable or
  links an adapter directly — backend selection moved entirely into
  `dbboard-server` (`backend_config_from_env`), so the desktop and any
  future headless deployment share one source of truth.

---

## ADR-0011 — SemVer for dbboard; tiered DB version support; `compatibility.md` as the runbook

- **Date**: 2026-05-25
- **Status**: accepted

### Context

Two version-related questions were left implicit so far:

1. **How dbboard itself is versioned.** `Cargo.toml` sat at `0.0.0`,
   `main` was reserved for "tagged releases" (`CLAUDE.md`) without
   defining what a tag means, and there is no CHANGELOG. With three
   adapters now in tree and Phase 2 about to extract a trait, we need
   a public-API contract before users can rely on anything.
2. **Which versions of each backing database we support.** The
   `dbboard-turso` / `dbboard-d1` / `dbboard-postgres` crates pin client
   library versions in `Cargo.toml`, but no document says which
   *server-side* versions (CockroachDB v24, Postgres 16/17, etc.) the
   project will keep working. Without a policy, "it broke against my
   Postgres" becomes an open-ended bug class.

### Decision

**Versioning of dbboard itself**

- Adopt **SemVer** (`MAJOR.MINOR.PATCH`).
- The **public API for SemVer purposes is the HTTP contract** in
  [`docs/api-contract.md`](api-contract.md) — nothing else. Internal
  crates stay `publish = false` (ADR-0002 still holds) and their Rust
  surfaces are not covered.
- **`0.x` phase**: cut `0.1.0` when Phase 1 (Turso vertical slice) ships
  end-to-end. Subsequent phase completions and capability additions are
  MINOR bumps; bug fixes are PATCH. Breaking contract changes during
  `0.x` bump MINOR (per SemVer's `0.y.z` carve-out) and are also recorded
  as an ADR.
- **`1.0.0`** is gated on: the HTTP contract being interop-verified
  against `dbboard-web`, the three current adapters being
  production-usable, and the capability model (ADR — to be written
  alongside Phase 2) being in place so per-DB features can be added
  without breaking the contract.
- **Distribution**: GitHub Releases for binaries. No crates.io publish
  for the workspace members.
- **CHANGELOG**: Keep a Changelog format at the repo root, updated in
  the same PR that adds the user-visible change. ADRs remain the
  decision log; CHANGELOG is the user-visible delta.

**Per-database version support**

Each backend is classified into one of three tiers:

- **Tier 1** — covered by a live integration test in CI (or runnable
  locally behind a documented env var until CI gains the credential).
  Regression here blocks release.
- **Tier 2** — expected to work because the wire/REST protocol matches
  Tier 1, but not pinned by an automated test. Issues are fixed on a
  best-effort basis.
- **Best effort** — undeclared versions. No promise; PRs welcome.

For server-side databases with a public version number (Postgres,
CockroachDB), the policy is **current major and previous major as Tier 1
or Tier 2** (e.g. Postgres 16 + 17). Older majors are best effort.
Managed services without a user-visible version (Turso, D1, Supabase)
track the vendor's current API surface and the pinned client crate.

The authoritative matrix lives in [`docs/compatibility.md`](compatibility.md);
README links to it and never duplicates the table.

**Process for moving a version between tiers**

- Promoting / dropping a tier requires a `docs/compatibility.md` edit
  and a CHANGELOG entry.
- Dropping a Tier 1 version is a deprecation: announced in one release,
  removed in the next MINOR (or MAJOR after `1.0`).
- Upgrading a client crate across a breaking change (e.g. `sqlx` 0.8 →
  0.9) requires its own ADR per the "non-trivial crate" rule in
  `CLAUDE.md`.

### Consequences

- A user reading the README can answer "is my Postgres version
  supported?" without grepping `Cargo.toml`.
- The "public API" being only the HTTP contract keeps internal
  refactors (e.g. the Phase 2 trait extraction, the capability model)
  out of SemVer's way — they touch no published surface.
- We accept the cost of maintaining `compatibility.md` and CHANGELOG.md
  by hand until tooling justifies automation.
- `Cargo.toml`'s `version = "0.0.0"` stays until Phase 1 ships; the
  first real bump is `0.1.0` and lands in the same commit that closes
  the Phase 1 checklist.
- `main` continues to mean "tagged releases only" (ADR-0005); the tag
  scheme is now `v<MAJOR>.<MINOR>.<PATCH>`.

---

## ADR-0012 — Capability-based extensibility for the adapter trait

- **Date**: 2026-05-25
- **Status**: accepted

### Context

Phase 2 extracts the `DatabaseAdapter` trait the previous phases
deliberately deferred (ADR-0003). At the same time, the roadmap calls
for surfacing **per-DB features** that have no counterpart on other
backends — PostgreSQL views and functions, Supabase auth and storage,
D1 bindings, etc.

Three structural problems block that today:

1. `dbboard-server::Backend` is a **closed enum**
   (`crates/dbboard-server/src/backend.rs`). Each new adapter forces
   edits to the enum and every `match` over it; per-DB features would
   multiply the match space.
2. `dbboard-core` has **no adapter trait** yet
   (`crates/dbboard-core/src/lib.rs`). Phase 2 is the chance to shape
   it once.
3. The HTTP contract is a **fixed three-endpoint surface**
   (`docs/api-contract.md`). Per-DB endpoints would either bloat the
   shared contract or splinter it.

Adding per-DB features ad hoc would either re-create the enum
explosion inside the trait or push DB-specific concepts up into
`dbboard-core`, where they don't belong (the core is the shared
kernel; DB-specifics are bounded contexts that depend on it, not the
other way round).

### Decision

Adopt a **capability pattern** (Role / Specification in DDD terms).
The Phase 2 trait extraction lands together with the capability model
so the two are designed as one piece.

**Core trait — small, required, stable**

```rust
// dbboard-core/src/adapter.rs (new in Phase 2)
#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    async fn ping(&self) -> DbResult<()>;
    async fn introspect(&self) -> DbResult<SchemaSnapshot>;
    async fn query(&self, sql: &str) -> DbResult<QueryResult>;

    fn views(&self) -> Option<&dyn ViewIntrospection> { None }
    fn functions(&self) -> Option<&dyn FunctionIntrospection> { None }
    fn auth(&self) -> Option<&dyn AuthAdmin> { None }
    fn storage(&self) -> Option<&dyn StorageAdmin> { None }
    fn realtime(&self) -> Option<&dyn RealtimeChannels> { None }
    // New capabilities are added as new methods with `None` defaults.
}
```

Each capability is its own trait in
`dbboard-core/src/capabilities/{views, functions, auth, storage, realtime}.rs`.
Adapters implement whatever subset they natively support; the default
`None` means callers never see "not supported" as a construction-time
special case.

`Capabilities` is a plain `Copy` flag struct, cheap to serialise over
HTTP for discovery. Invariant:
`caps.has_views == adapter.views().is_some()`, enforced by the adapter
author and unit-tested per adapter.

**`async-trait` for the foreseeable future**

AFIT (async fn in trait, stable in 1.75) is not object-safe; the server
needs `Arc<dyn DatabaseAdapter>`. Use the `async-trait` crate until
object-safe async fns land.

**Server — `Backend` enum becomes a trait object**

`crates/dbboard-server/src/backend.rs` collapses to:

```rust
pub(crate) struct Backend {
    adapter: Arc<dyn DatabaseAdapter>,
}
```

`BackendConfig::connect` is the only place that knows the concrete
adapter set; adding an adapter touches one match arm there and zero
match arms anywhere else.

**HTTP contract — additive chapters with capability discovery**

The core stays the three current endpoints. New endpoints are nested
per capability under stable prefixes:

| Capability | Endpoint prefix |
|---|---|
| (core) | `/health`, `/tables`, `/query` |
| views | `/views/...` |
| functions | `/functions/...` |
| auth | `/auth/...` |
| storage | `/storage/...` |
| realtime | `/realtime/...` |

A new `GET /capabilities` returns the `Capabilities` struct so the UI
and `dbboard-web` can render affordances without trial calls. Hitting a
capability endpoint on a backend that doesn't support it returns `404`
with the standard error envelope and a new `capability` category in
`docs/api-contract.md`.

**UI — capability-guarded panels**

```rust
if caps.has_views { show_views_panel(...); }
```

Panels never `unwrap` on a capability. The UI's HTTP client treats
`404 capability` as "this backend does not support X", surfaced as a
greyed control or hidden panel — never as a red error.

### Consequences

- Adding a new capability across the stack = **three places**: a new
  trait in `dbboard-core/src/capabilities/`, an `impl` in the adapters
  that have it, and a UI panel guarded by the flag. Other adapters and
  unrelated UI panels are untouched.
- The `Backend` enum disappears; the adapter set grows with one arm in
  `BackendConfig::connect`.
- `dbboard-core` gains an `async_trait` dependency. The "no I/O"
  property holds (defining an async trait is not I/O).
- SemVer impact (ADR-0011): **adding** a capability is additive on the
  HTTP contract — MINOR. **Removing or reshaping** a capability is
  breaking — MAJOR after `1.0`.
- Trait-object indirection is added on every adapter call. Acceptable
  for I/O-bound code (network dominates vtable dispatch by orders of
  magnitude).
- Phase 2's exit criterion ("nothing in `dbboard-ui` knows the word
  'Turso'") tightens to: nothing in `dbboard-ui` or the HTTP contract
  knows any concrete adapter's name; only capability flags.
- This ADR fixes the design but **defers most implementation** to
  Phase 2 and Phase 3. Only the core trait, the `Capabilities` struct,
  and the `Backend` → `Arc<dyn>` swap are in Phase 2. Concrete
  capability traits land when the adapters that need them do (e.g.
  `auth` arrives with `dbboard-supabase` in Phase 3).

---

## ADR-0013 — Local TOML connection store with OS keychain for secrets

- **Date**: 2026-06-03
- **Status**: accepted

### Context

Phase 2's remaining tasks (connection management UI, persisted query
history) need a durable home for user-defined connections. So far the
desktop has only ever resolved a backend from `DBBOARD_*` environment
variables (`apps/dbboard::main` → `dbboard-server::backend_config_from_env`),
which is fine for single-DB CI runs but cannot hold a list of named
connections a user adds in the UI.

Three constraints shape the design:

1. **`dbboard-core` is "no I/O"** (ADR-0002, reaffirmed by ADR-0009 as
   "serde only"). Filesystem reads and OS keychain calls cannot live
   there.
2. **`apps/dbboard` is "wiring only"** — it must not host reusable
   persistence logic that the future connection-management UI (and any
   headless deployment) would also need.
3. **Secrets must never appear in a file** the user might back up, sync,
   or paste into an issue. Connection metadata (kind, host, ids) is fine
   in a flat file; tokens and connection strings are not.

We also must not regress the Phase 1.6 / 1.7 exit criteria, both of
which guarantee env-driven adapter selection. Whatever we add has to be
additive and inert until populated.

### Decision

Introduce a new crate **`crates/dbboard-config`** that owns both halves
of user-facing configuration:

- A per-user **TOML file** at `directories::ProjectDirs::from("dev",
  "dbboard", "dbboard").config_dir().join("connections.toml")`
  (`%APPDATA%\dbboard\dbboard\config\connections.toml` on Windows,
  `~/Library/Application Support/dev.dbboard.dbboard/connections.toml`
  on macOS, `$XDG_CONFIG_HOME/dbboard/connections.toml` on Linux). The
  file starts with `version = 1` and a list of `[[connections]]`
  entries. A missing file is **not** an error — `load_or_empty` returns
  an empty store and no file is created until the user saves an entry.
  On Unix the file is written with mode `0o600`.
- A **`SecretStore` trait** with two implementations: `KeyringStore`
  (backed by the `keyring` crate, service string `"dbboard"`, account
  string from the TOML's `keyring_*_ref`) and `InMemorySecretStore`
  for tests, CI, and Linux runners without a Secret Service. The
  TOML stores only opaque keychain key references, never secret
  material.

TOML schema (versioned; unknown version is a hard error):

```toml
version = 1

[[connections]]
id = "local-turso"
name = "Local libSQL"
kind = "turso"
path = ":memory:"

[[connections]]
id = "prod-d1"
name = "Prod D1"
kind = "d1"
account_id = "..."
database_id = "..."
base_url = "..."                       # optional
keyring_token_ref = "dbboard.prod-d1.token"

[[connections]]
id = "neon-staging"
name = "Neon Staging"
kind = "postgres"
keyring_url_ref = "dbboard.neon-staging.url"
```

Duplicate `id`, unknown `kind`, and unknown `version` are all hard
parse errors. We surface drift loudly rather than silently dropping
entries.

**Resolution order in `apps/dbboard::main`** becomes:

1. `DBBOARD_PG_URL` → Postgres (existing).
2. `DBBOARD_D1_*` trio → D1 (existing).
3. `DBBOARD_TURSO_PATH` (when set) → Turso (existing).
4. **New**: `DBBOARD_CONNECTION=<id>` selects an entry from
   `connections.toml` by id; its `keyring_*_ref` values are resolved
   through the `SecretStore` and converted into `BackendConfig`.
5. **New**: with `DBBOARD_CONNECTION` unset and exactly one entry in the
   file, that entry is auto-selected (single-user convenience).
6. Default Turso `:memory:` (existing).

The config file therefore stays inert for existing CI and Phase 1.6/1.7
exit criteria; nothing changes until the file is populated or
`DBBOARD_CONNECTION` is set.

`keyring` is chosen over alternatives because it maps uniformly to
Windows Credential Manager, macOS Keychain, and Linux Secret Service,
is `Send + Sync`, and does not drag system OpenSSL into the build
(consistent with the `rustls` discipline in ADR-0007 and ADR-0008).
Alternatives considered: `secret-service` (Linux-only — fails the
cross-platform requirement) and hand-rolled DPAPI / Security.framework
wrappers (re-implementing `keyring` poorly).

Config errors are crate-local (`ConfigError`); they happen at process
startup, before the server binds, and never reach the HTTP envelope.
**No change** to `docs/api-contract.md`, `DbError`, or any wire surface.

### Consequences

- The workspace gains one crate (`dbboard-config`) and two external
  dependencies: `directories` (config-dir resolution) and `keyring`
  (OS secret storage). `serde` / `toml` are already pulled in
  transitively.
- The `dbboard-core` "no I/O" rule (ADR-0002, ADR-0009) is preserved:
  `dbboard-config` owns both filesystem and keychain calls; `core`
  stays serializable-only.
- Connection metadata becomes safely shareable (backup, copy between
  machines, paste into a bug report); secrets stay in the per-machine
  OS keychain.
- A user without a Secret Service available (headless Linux runner,
  some CI configurations) can still boot the app: `KeyringStore`
  reports unavailability at construction, the app falls back to
  `InMemorySecretStore`, and any connection requiring a secret simply
  fails at resolve time with a clear `ConfigError::Secret(...)`.
  The default Turso `:memory:` path (step 6 above) keeps working.
- The next two Phase 2 tasks (connection management UI, persisted query
  history) now have a shared persistence layer to bind against:
  `save_atomic` exists for the UI to call, and the directories crate
  helpers can be reused for the query-history file.
- SemVer impact (ADR-0011): additive. The HTTP contract is unchanged;
  internal crates remain `publish = false`. The TOML schema is itself
  versioned (`version = 1`), so future schema changes will be migrated
  in-place rather than guessed at.

---

## ADR-0014 — Query history (in-memory first, persisted later)

- **Date**: 2026-06-03
- **Status**: accepted

### Context

Phase 2 calls for "query history (in-memory, then persisted)" alongside
the connection store from ADR-0013. The UI today has no recall: every
time the user wants to re-run a recent statement they retype it. A first
pass should make the recent statements visible and clickable to refill
the editor, without committing to a persistence shape that might
constrain the connection-management UI still to come.

The UI lives in `dbboard-ui` and depends only on `dbboard-core` among
workspace crates (ADR-0002). Whatever we add must respect that — and the
HTTP contract must not change, because history is purely a UI concern
(the server has no concept of "previous queries").

### Decision

Land query history in two stages:

1. **Stage 1 — In-memory, this ADR.** A new `HistoryStore` lives entirely
   inside `dbboard-ui`. It is a bounded ring buffer (capacity 100) of
   `HistoryEntry { sql: String }`. `push(sql)` is called whenever the
   editor's Run button fires; consecutive duplicates collapse so a
   double-click on Run does not pollute the list. Iteration is
   newest-first to match how the panel renders. Nothing is written to
   disk. No new dependency.

2. **Stage 2 — Persisted, a later ADR.** When the connection-management
   UI has shipped (and the keyring + TOML pattern from ADR-0013 is
   exercised), revisit persistence with the full picture. The likely
   target is a small SQLite file alongside `connections.toml` (so a
   single per-OS config dir owns both), but the choice is deferred — we
   do not want history's storage shape to leak into connection-
   management decisions.

The HTTP contract (`docs/api-contract.md`) is **not** touched. There is
no `/history` endpoint and no new server state. Should a future feature
(e.g. cross-connection history surfacing) require server involvement, a
dedicated ADR will draft that contract change first.

### Consequences

- `dbboard-ui` gains a `history` module. No new workspace crate, no new
  external dependency. The layered architecture (ADR-0002) is preserved.
- Phase 2's "query history (in-memory)" exit is met by Stage 1; the
  "then persisted" piece is explicitly deferred to a Stage 2 ADR.
- The bound (100) is a UI ergonomics choice, not a correctness one: an
  in-memory list of 100 short SQL strings is well under any meaningful
  resource budget. The cap exists so the panel does not grow unbounded
  during a long session.
- Adjacent dedup (consecutive identical Run clicks collapse) is a
  deliberate ergonomics call: history should reflect distinct attempts,
  not button mash. Non-adjacent repeats are kept (re-running an earlier
  query after exploring is a meaningful event).
- HTTP contract unchanged → no web-side mirror needed (ADR-0004).
- SemVer impact (ADR-0011): additive. Internal `dbboard-ui` API only.

---

## ADR-0015 — Multi-language support (11 locales, Stage 1)

- **Date**: 2026-06-03
- **Status**: Superseded in part by [ADR-0022](#adr-0022--runtime-locale-switcher-revises-adr-0015s-startup-only-resolution) (2026-06-11) for the "startup-only resolution" decision (the runtime switcher mutates the active bundle in place). The locale list, the `fluent-rs` + `i18n-embed` framework choice, the `DBBOARD_LANG` startup precedence, and the CJK font strategy all remain in force.

### Context

The desktop UI ships English-only today. Every visible label, button, and
empty-state message in `dbboard-ui` is a raw string literal. The user
asked to lift this to a multilingual surface covering Japanese, Korean,
Chinese, English, "plus other major economic-zone languages". The
roadmap previously listed "i18n" loosely under Phase 5 (quality of life);
the request promotes it to Phase 2's closing scope because it shapes
later UI work (connection-management dialogs, AI panel) — adding it
after those land would require revisiting every new label.

Three things have to be decided together: which locales to ship now,
what runtime framework carries them, and how text actually paints on
screen (egui's default font stack covers Latin only — Cyrillic is
partial, CJK is `tofu`). Splitting these into separate ADRs would
strand each one waiting on the others.

The HTTP contract (`docs/api-contract.md`, ADR-0009) is shared with the
web sibling. Translating error messages on the wire would create
contract drift; the web side already has its own i18n story. So this
ADR is strictly a `dbboard-ui` (presentation) concern.

### Decision

**Locales (Stage 1, 11 total).** Two tiers, both included now.

| Tier | Locale     | BCP-47    | Rationale                                |
|------|------------|-----------|------------------------------------------|
| 1    | English    | `en`      | Fallback for every missing key.          |
| 1    | Japanese   | `ja`      | Maintainer's first language; OSS reach. |
| 1    | Korean     | `ko`      | Requested; large dev community.          |
| 1    | Simp. CN   | `zh-CN`   | Requested; largest economy + dev base.   |
| 1    | Trad. CN   | `zh-TW`   | Requested; Taiwan / Hong Kong audience.  |
| 2    | German     | `de`      | EU / DACH region.                        |
| 2    | French     | `fr`      | EU / La Francophonie.                    |
| 2    | Spanish    | `es`      | EU + Latin America.                      |
| 2    | Pt. (BR)   | `pt-BR`   | Brazil. Distinguished from European pt.  |
| 2    | Russian    | `ru`      | Cyrillic coverage anchor.                |
| 2    | Italian    | `it`      | EU rounding-out.                         |

Explicitly **rejected for Stage 1**: Arabic (`ar`) and Hindi (`hi`). Both
are major-economic-zone languages by traffic, but Arabic requires RTL
mirroring (egui's layout primitives do not flip cleanly yet, and
review-quality direction-mirroring needs design work), and Hindi needs
Devanagari shaping which the bundled egui glyph cache currently
substitutes with tofu on Windows. A future ADR will lift these once
shaping + RTL are addressed (likely paired with the AI panel work in
Phase 4, where input text fields multiply the surface area).

**Framework: `fluent-rs` + `i18n-embed`.**

- `fluent-bundle` is Mozilla's runtime for ICU MessageFormat-style
  messages with plurals, selectors, and per-locale resource files (`.ftl`).
  It is the de facto Rust choice for full ICU coverage; the alternative
  `gettext` family is simpler but pluralization in CJK is awkward and
  the `.po`/`.mo` tooling is heavier than what an OSS desktop client
  needs.
- `i18n-embed` provides the loader glue (locale fallback chain,
  embedded resources via `rust-embed`, `tr!()` macro, requester pattern).
  Without it, the `fluent_bundle` API requires hand-rolling fallback and
  caching per app.
- Locale identifiers use `unic-langid` (which both crates depend on).
- All three crates are MIT/Apache licensed and have been stable for
  multiple years.

Translation files live at `crates/dbboard-i18n/i18n/<locale>/dbboard.ftl`
and are embedded into the binary at compile time (no runtime file I/O
for the default install — keeps the "single self-contained binary"
property from ADR-0007). Future community-translation workflows can
opt into `i18n-embed`'s file-system requester for live reload during
translation review without affecting release builds.

**Locale resolution at startup.** Priority order (highest first):

1. `DBBOARD_LANG` environment variable (operator override; same idiom as
   `DBBOARD_PG_URL` / `DBBOARD_D1_*` env precedence in `apps/dbboard`).
   Parsed as a BCP-47 tag; invalid values fall through with a warning.
2. OS locale via the `sys-locale` crate (pure Rust, no external deps;
   reads `GetUserDefaultLocaleName` on Windows, `CFLocaleCopyCurrent` on
   macOS, `LC_ALL`/`LC_MESSAGES`/`LANG` on Linux).
3. Hard-coded fallback to `en`.

The resolved locale is fed into `i18n-embed`'s `LanguageRequester`,
which then walks the supported-locales list applying the fallback
chain `zh-CN → zh → en`, `pt-BR → pt → en`, etc. A missing key in any
locale falls back to `en` (which is the source-of-truth for all keys).

**Font strategy.**

- **Latin + Cyrillic**: egui's bundled `Ubuntu-Light` proportional font
  already covers these glyph ranges. No new asset is needed for Stage 1.
- **CJK (`ja` / `ko` / `zh-CN` / `zh-TW`)**: egui does not bundle a CJK
  font (size budget). Instead, `apps/dbboard` registers system fonts at
  startup via `eframe`'s `FontDefinitions` using OS-specific candidate
  lists:
  - Windows: `Yu Gothic UI` / `Microsoft YaHei UI` / `Malgun Gothic`.
  - macOS: `Hiragino Sans` / `PingFang SC` / `PingFang TC` / `Apple SD
    Gothic Neo`.
  - Linux: `Noto Sans CJK JP` / `Noto Sans CJK KR` / `Noto Sans CJK SC`
    / `Noto Sans CJK TC` (typical Noto family install).
  When none are found we log a warning and fall back to the bundled
  font (tofu for CJK glyphs, but the rest of the UI remains usable).
  Bundling Noto CJK ourselves (~20 MB per script) is rejected as a
  Stage 1 cost; revisit if CJK users routinely report missing system
  fonts.

**Scope: `dbboard-ui` only.**

- `dbboard-core` `DbError` variants stay English. They appear on the
  HTTP wire (ADR-0009); changing them would break the contract shared
  with `dbboard-web` (ADR-0004). The UI prefixes a translated category
  label (`Connection error: …`) but the error body remains the
  server-returned text. This is the right boundary: error *taxonomy* is
  contract; error *presentation* is UI.
- `dbboard-config`, `dbboard-server`, and the adapter crates are
  English-only for the same reason — they all participate in the
  contract surface either directly (server) or via error mapping
  (adapters → server).

### Consequences

- A new internal crate `crates/dbboard-i18n` carries the loader, the
  embedded `.ftl` resources, and a thin `t!(...)` re-export. `dbboard-ui`
  depends on it. No other workspace crate does. The layered architecture
  (ADR-0002) is preserved: `dbboard-i18n` depends only on third-party
  crates; `dbboard-ui` depends on `dbboard-core` + `dbboard-i18n`.
- New runtime dependencies: `fluent-bundle`, `i18n-embed` (with the
  `fluent-system` + `desktop-requester` features), `rust-embed`,
  `unic-langid`, `sys-locale`. All MIT or Apache. License compatibility
  for `cargo deny` (ADR-0011) is unchanged — we already permit MIT,
  Apache-2.0, ISC, BSD-2/3, MPL-2.0.
- Binary size grows by ~1.2 MB (release, glibc x86_64) for the fluent
  runtime plus the embedded `.ftl` resources. Cold-start cost is one
  bundle-load per resolved locale; measured at <5 ms on a modern laptop
  and amortised over the session.
- The desktop UI now follows the user's OS locale by default. The
  `DBBOARD_LANG` env override exists for screenshot tests, demo builds,
  and Windows users whose OS locale and preferred review language
  differ.
- HTTP contract unchanged → no web-side mirror needed (ADR-0004).
  Translation drift between desktop and web is acceptable: each surface
  owns its own `.ftl` (or web equivalent).
- SemVer impact (ADR-0011): additive. Internal crates only; the binary
  changes its default copy but not its CLI or wire surface. The
  `DBBOARD_LANG` env var is an opt-in additive surface — documented in
  `docs/connections.md` once landed.
- The roadmap's Phase 5 "i18n" bullet (if any was implied) is
  superseded: i18n now closes Phase 2 rather than waiting for QoL. The
  Stage 2 ADR for `ar` / `hi` (RTL + shaping) remains a Phase 4 / 5
  candidate.

## ADR-0016 — Connection management UI (HeidiSQL model: process-per-connection, Stage 1)

**Status:** Superseded in part by [ADR-0020](#adr-0020--in-process-connection-switching-supersedes-adr-0016s-stage-1-mental-model) (2026-06-04) for the
"process-per-connection / in-app switching out of scope" parts
(decision points 1, 2, and 3). The rest of the ADR — `ConnectionAdmin`
in `dbboard-config`, secrets handling, validation, no HTTP contract
change — remains accepted.

**Context.** ADR-0013 introduced `connections.toml` + OS keychain
through `crates/dbboard-config`, but exposed no UI: the only ways for
a user to add or change a connection were editing the TOML by hand and
seeding secrets through `keyring` CLI. Phase 2's open exit-criteria
item is "Connection management UI (add / edit / delete)" and this ADR
fixes its shape.

**Decision.**

1. **Mental model: process-per-connection (HeidiSQL-style).** Each
   running `dbboard` process owns exactly one active connection,
   resolved at startup by the precedence chain already shipped (env
   vars > `DBBOARD_CONNECTION=<id>` > single-entry auto-select > Turso
   `:memory:`). Working against multiple databases at once is done by
   launching multiple processes, not by swapping inside one. This
   matches the desktop affordance the maintainer actually uses (per the
   2026-06-03 product call) and removes a whole class of contract
   questions ("what happens to a query mid-swap?", "does the cache
   warmup carry over?").

2. **In-app switching is explicitly out of scope for Stage 1.** No
   "active connection" selector, no `POST /admin/switch` endpoint, no
   tabbed multi-connection UI. A future Stage 2 ADR may introduce
   tab-style multi-connection if usage warrants — leaving it out now
   keeps `dbboard-server` adapter-immutable post-startup (it owns one
   `Arc<dyn DatabaseAdapter>` per process lifetime — see ADR-0012) and
   keeps the HTTP contract untouched.

3. **Stage 1 surface: Add, Edit, Delete only.** The UI manages the
   *saved set* of connections, not the *active* one. A passive label
   showing the current process's resolved connection id is acceptable
   for orientation; no button changes which connection the running
   process talks to.

4. **`ConnectionAdmin` use-case lives in `dbboard-config`, not the UI.**
   `dbboard-config` already owns the TOML + keyring surface; we add a
   `ConnectionAdmin` struct that holds a `PathBuf` and an
   `Arc<dyn SecretStore>` and exposes `entries()` / `add()` / `update()`
   / `delete()`. Each mutating call performs the keyring write first,
   then atomically rewrites `connections.toml` (`*.tmp` → `fs::rename`,
   already implemented in `store::save_atomic`); on TOML-write failure
   the keyring write is rolled back so the two stores cannot diverge.
   `dbboard-ui` depends on `dbboard-config` and calls these methods —
   the UI does no direct filesystem or keychain I/O. This matches the
   existing pattern where `apps/dbboard` is the only wirer of
   concrete persistence into `dbboard-server`; `dbboard-ui` stays
   presentation-shaped (`egui` widgets + view-model state).

5. **Secrets handling.**
   - **Add (D1 / Postgres)**: the form captures secret material in a
     `String` field that is never written to disk except via the
     `SecretStore`. On submit, `ConnectionAdmin::add` first calls
     `secrets.set(keyring_ref, value)`, then writes the TOML; on the
     reverse, `delete` writes the TOML first (the file is the source of
     truth) and then best-effort purges the keyring entry. The latter
     ordering means a crashed delete leaves an orphan keyring entry,
     not an orphan TOML entry; orphan keyring entries are harmless
     (the resolver only ever reads what the TOML references).
   - **Edit**: the form prefills everything *except* secret values.
     Leaving the secret field blank keeps the existing keyring entry;
     entering a new value rewrites it. The UI shows "(unchanged)"
     placeholder text so an editor does not assume the field is empty.
   - **Read-back of existing secrets is not provided.** The keychain
     is write-only from the UI — preventing a "Show password" affordance
     keeps shoulder-surfing attacks out of scope and matches how every
     keychain-aware client (1Password, Sequel Ace, HeidiSQL) handles
     stored credentials.

6. **Validation: hard-fail before persistence, not after.** The Save
   button is disabled until every required field for the selected
   `ConnectionKind` is non-empty:
   - `Turso`: `path` non-empty.
   - `D1`: `account_id`, `database_id`, `token` non-empty
     (`base_url` optional, defaults to Cloudflare's REST endpoint).
   - `Postgres`: `url` non-empty.
   `id` must be a unique non-empty slug across the file; duplicates
   are caught client-side and via the existing `ConfigError::Duplicate`
   check in `ConnectionFile::add`. We do *not* attempt to ping the
   database at save time — the resolver already fails loudly at next
   startup if the credentials are wrong, and a synchronous ping in the
   UI thread would block the event loop. A future Stage 2 ADR may add
   an async "Test connection" affordance.

7. **No HTTP contract change.** Every byte the UI writes lands in
   `connections.toml` or the OS keychain; nothing flows to the
   loopback server. The web sibling has its own connection-management
   story and does not consume any of this.

**Alternatives considered.**

- **In-app hot-swap (`POST /admin/switch`).** Rejected for Stage 1:
  introduces an admin surface that conflicts with the
  one-adapter-per-process invariant in ADR-0012, requires a web
  mirror, and the maintainer's HeidiSQL-style workflow does not need
  it. Revisitable as ADR-0017+ if usage data argues otherwise.
- **Tabbed multi-connection in one process.** Rejected for Stage 1:
  needs N adapters in the server (ADR-0012's `Arc<dyn>` would have to
  become a slot map keyed by tab) and changes the UI from
  one-result-table to a tab strip + N panes. Reasonable Stage 2
  feature; not blocking for "manage the saved list".
- **UI talks to `dbboard-config` through a trait.** Rejected as
  premature: there is exactly one production impl
  (`KeyringStore` + filesystem), and `dbboard-config` is already an
  internal crate. Adding a `ConnectionAdminApi` trait now would be
  abstraction-for-its-own-sake; the seam exists at `SecretStore`,
  which is what tests already use.
- **Read-back of stored secrets.** Rejected on security grounds (see
  point 5). Storing credentials write-only is the same model every
  serious DB client uses.

**Consequences.**

- Adds `ConnectionAdmin` to `dbboard-config` with tests covering
  add / update / delete, rollback on TOML-write failure, and the
  "delete orphans keyring, never TOML" guarantee.
- `dbboard-ui` grows a `connections::ConnectionsWindow` module that
  renders an `egui::Window` with the list + an inline form per Add /
  Edit operation. The window is opened from a top-bar "Connections"
  button. Closing the window does not affect the running session.
- `apps/dbboard` constructs the `ConnectionAdmin` in `main` (alongside
  the existing `KeyringStore` + `load_or_empty` flow) and hands it to
  `DbboardApp::connect_with_admin`. Existing `connect` constructor
  stays for tests that do not need the admin surface.
- The `dbboard-web` sibling sees no contract or wire change.
  `dbboard-web-state.md` memory records ADR-0016 in the "non-contract
  desktop changes" list, same shape as ADR-0013 / ADR-0015.
- Roadmap Phase 2 ticks the last `[ ]` item; Phase 2 exit criteria
  ("nothing in `dbboard-ui` knows the word 'Turso'") is preserved —
  the form's `ConnectionKind` dropdown is a presentation detail keyed
  by the existing enum, not adapter-specific logic.

---

## ADR-0017 — Query history persistence (JSON Lines, schema shared with `dbboard-web`, Stage 2)

**Status:** Accepted (2026-06-04). Realises the deferred "Stage 2 ADR"
promised by ADR-0014.

**Context.** ADR-0014 landed Stage 1 of query history: a bounded,
newest-first ring buffer in `dbboard-ui` with no on-disk
representation. The deferred "Stage 2 ADR" had two open questions —
*what format* and *where on disk* — that we deliberately punted until
the connection-management UI (ADR-0016) shipped. Both have now landed,
so the constraints are knowable.

A maintainer call on 2026-06-03 added a third constraint: the on-disk
record shape should be **shared with the `dbboard-web` sibling** so
that the history of a desktop and a web instance can be read by the
same `jq` pipeline. Storage location and write implementation can
diverge between the two repos; the *record schema* cannot.

Survey of comparable tools (also from the 2026-06-03 call):

| Tool | Persistence | Format |
| --- | --- | --- |
| HeidiSQL | Windows registry / `portable_settings.txt` | Delphi INI-style |
| DBeaver | Workspace SQLite | Opaque binary |
| DataGrip | `consoles/db/<dsn>/console.history` | Plain text with comments |
| TablePlus | Per-connection SQLite | Opaque binary |
| Beekeeper Studio | App-data SQLite | Opaque binary |

None of them are friendly to `jq` / `tail -F` / `grep`. Making the
file directly inspectable by standard Unix tools is a deliberate UX
differentiator for `dbboard`, not an accident.

**Decision.**

1. **Format: JSON Lines (`.jsonl`, one JSON object per line, LF-only).**
   The file is appended to in real time; readers can `tail -F` it,
   `jq` it, `grep` it, or feed it to any stream-oriented pipeline
   without an intermediate parse step. Newlines are LF on every
   platform (Windows readers cope with LF; Unix readers do not cope
   with CRLF). Encoding is UTF-8 without BOM.

2. **Record schema (single source of truth, shared cross-repo):**

   ```jsonc
   {
     "v": 1,                              // schema version
     "ts": "2026-06-04T14:22:01.123Z",   // RFC 3339, UTC, ms precision
     "conn": "prod-pg",                   // connection id (TOML primary key)
     "actor": null,                       // desktop null; web populates
     "sql": "SELECT * FROM users LIMIT 10",
     "status": "ok",                      // "ok" | "error"
     "duration_ms": 42,                   // wall-clock from submit to envelope
     "rows": 10,                          // row-returning result; null otherwise
     "rows_affected": null,               // DML result; null otherwise
     "error": null                        // {category, message} when status="error"
   }
   ```

   Field semantics:

   - **`v`**: schema version. Currently `1`. **Renaming or
     repurposing any field is a breaking change and requires a new
     ADR** that bumps `v`. Adding optional fields is additive and
     does not bump `v`.
   - **`ts`**: RFC 3339 with millisecond precision, always UTC
     (trailing `Z`). Local-time conversion is the reader's job —
     `jq` users typically pipe through `fromdateiso8601`.
   - **`conn`**: matches the `id` field of the corresponding
     `connections.toml` entry on desktop (or the equivalent
     server-side connection record on web). Lookup of friendly name,
     kind, etc. is the reader's job — keeping the file
     denormalisation-free makes rotation trivial.
   - **`actor`**: `null` on desktop (single-user, single-process —
     ADR-0016). Web populates from the authenticated session / user
     id. Reserving the field on desktop day-1 prevents a schema bump
     when web's multi-user story matures.
   - **`status`**: lowercase. The only two values are `"ok"` and
     `"error"`. A future "cancelled" or "timeout" addition is
     additive (writers emit the new value, readers default to
     unknown).
   - **`duration_ms`**: wall-clock milliseconds from the moment the
     UI submits the query to the moment the result envelope is
     received. On error, the duration up to the error. Integer.
   - **`rows`** vs **`rows_affected`**: mutually exclusive. SELECT
     returns `rows` non-null and `rows_affected` null; DML returns
     the inverse; DDL/`ok` with no result population returns both
     `null`.
   - **`error`**: when `status="error"`, an object
     `{ "category": "<connection|query|schema|type_conversion|capability>", "message": "<English text>" }`
     matching the categories already shipped in
     `dbboard-core::DbError` (ADR-0009 / ADR-0004 / ADR-0012). The
     message is the raw English `DbError::message()` payload — UI
     translation (ADR-0015) is not applied to logs (the file should
     be locale-agnostic so cross-team `jq` works).

3. **Storage location (desktop).** Resolved via the same
   `directories::ProjectDirs` lookup that `connections.toml` uses,
   so a single OS config dir owns both:

   - Linux: `$XDG_CONFIG_HOME/dbboard/history.jsonl`
     (fallback `~/.config/dbboard/history.jsonl`)
   - macOS: `~/Library/Application Support/dev.dbboard.dbboard/history.jsonl`
   - Windows: `%APPDATA%\dbboard\dbboard\config\history.jsonl`

   A helper `dbboard_config::default_history_path()` returns the
   resolved path so the path policy stays in the same crate that
   already owns `default_path()`. The reader/writer itself lives in
   `dbboard-ui` (UI is the only crate that needs to read it; no other
   workspace crate should grow this dependency surface).

   The file lives next to `connections.toml`, but uses **no atomic
   rename** semantics: it is opened with `O_APPEND` (or the Windows
   equivalent — `OpenOptions::new().append(true).create(true)`) and
   each record is a single `write_all` of `serde_json::to_vec`
   followed by `b"\n"`. POSIX guarantees `O_APPEND` writes ≤ PIPE_BUF
   are atomic vs. concurrent writers; Windows' append handle behaves
   equivalently for the small (< 4 KiB) record sizes we produce. The
   resulting trade-off — a crash mid-write may leave a partial line —
   is accepted: the reader skips lines that fail to parse, logs the
   skip count, and continues.

4. **Rotation: size-based, lazy.** When the active file exceeds
   **50 MiB** *or* **100 000 lines** at startup, it is renamed to
   `history.jsonl.1` (overwriting any existing `.1`) and a fresh
   `history.jsonl` is created. Rotation is **not** triggered
   mid-session — a long-running session can grow the file past the
   cap; the cap only fires the next time the app starts. This keeps
   the write path lock-free and the rotation policy testable as a
   pure function.

   Only one generation (`.1`) is retained. Users who want longer
   retention can `mv history.jsonl ~/dbboard-archive/history-$(date +%F).jsonl`
   from a cron / scheduled task — the file is plain text and self-
   contained, no app cooperation required.

5. **Read policy (startup).** `apps/dbboard` reads the last
   `DEFAULT_CAPACITY` (= 100, unchanged from ADR-0014) lines, parses
   each, drops malformed lines with a startup-log warning that
   includes the count, and pushes the surviving entries into the
   in-memory `HistoryStore` newest-first. The UI sees the same API
   surface as Stage 1 — `HistoryStore::iter()` returns entries in
   newest-first order, the panel renders unchanged.

   The reader **ignores unknown JSON fields** (`serde(default)` +
   `#[serde(deny_unknown_fields)]` is NOT set) so a future schema
   that adds, say, `"user_agent"` does not break a freshly-installed
   client reading an older format.

6. **Write policy (runtime).** On every successful or failed query
   reply received by `DbboardApp`, build a record from the
   already-available metadata (the response envelope already carries
   row count / affected count / error category) and append it. The
   write is best-effort: a failure (disk full, file removed) logs to
   `tracing::warn!` and is otherwise swallowed — the UI must not
   block or fail because the history file is unwritable.

7. **Secret handling: write queries verbatim.** A `SELECT … WHERE
   token = 'sk_live_xxx'` lands in the file as-is. Justification:

   - The file lives at the same trust level as `connections.toml`
     (same per-user config dir, same OS user filesystem permissions).
   - Detecting and redacting "secret-looking" literals would require
     a lexer that understands every dialect — a perpetually wrong
     heuristic. The DBeaver / DataGrip prior art logs queries
     verbatim for the same reason.
   - README and `docs/connections.md` will note "the history file
     contains the literal text of every query you have run,
     including any string literals" so the affordance is not
     surprising.

   Encryption-at-rest is intentionally **not** added in Stage 2:
   adding a keyring-derived key would force every reader (including
   `jq`) to go through `dbboard`, killing the differentiator the
   format choice was made for. If a future privacy-sensitive
   deployment needs it, that is a Stage 3 ADR with its own UX
   trade-offs.

8. **HTTP contract is not touched.** No `GET /history` endpoint, no
   wire shape, no server state. The web sibling implements its own
   reader/writer with the same record schema; it does **not** consume
   any desktop code path. Rejecting an endpoint here is a deliberate
   call so that the file format stays a first-class UX surface and
   web's access-control design is not dragged into the cross-repo
   contract.

9. **Cross-repo coordination.** ADR-0017 is the single source of
   truth for the record schema. The sibling ADR on `dbboard-web` will
   say "schema is identical to desktop ADR-0017" and add only the
   web-specific I/O bits (storage location env var, multi-tenant
   `actor` semantics, NestJS write path). A handoff brief
   (`.claude/issues/0003-web-history-schema-mirror.md`, same format as
   `0001` / `0002`) lands in this PR for the web Claude session to
   pick up.

**Alternatives considered.**

- **SQLite alongside `connections.toml`.** Rejected: the
  differentiator we want is `jq` / `tail -F` / `grep` over the raw
  file. SQLite requires a client (or `sqlite3 ... | jq`), can't be
  tail-followed live, and adds a non-trivial dependency to
  `dbboard-ui` (today it has none beyond `egui` / `reqwest`). The
  prior-art table above is unanimous on SQLite — and unanimous on
  "users do not actually `jq` it".
- **Plain text (one SQL per line, no JSON).** Rejected: drops
  duration / status / connection / error category. The whole point
  of structured logging is structured filtering.
- **One file per connection.** Rejected: the most useful cross-cut
  is "find slow queries across all my databases" — denormalising
  `conn` into one global file keeps that one-liner trivial.
- **Atomic write via temp-file rename per record.** Rejected:
  ~100× slower under typical use, no real safety win (an
  `O_APPEND` write of a < 4 KiB JSON line is atomic on the
  platforms we care about), and would defeat the `tail -F` UX
  (every record would replace the inode).
- **Encryption-at-rest.** Rejected for Stage 2 (point 7). If the
  user is on a multi-tenant machine where the history file leaks,
  `connections.toml` already leaks `keyring_*_ref` pointers and
  any plaintext URL — and the OS keychain protects the actual
  secret material. Encrypting just the history would not raise the
  effective floor.
- **Adding `GET /history` to the HTTP contract.** Rejected (point 8).

**Consequences.**

- `dbboard-config` grows a `default_history_path()` symmetric to
  `default_path()`. No new external dependency (`directories` already
  in.).
- `dbboard-ui::history` grows a `PersistentHistoryStore` that wraps
  `HistoryStore` and owns the append-only writer and a `load_tail`
  associated function for startup. `HistoryStore`'s public API is
  unchanged — Stage 1 callers that only need the in-memory ring
  buffer keep working.
- `HistoryEntry` gains `ts` / `conn` / `status` / `duration_ms` /
  `rows` / `rows_affected` / `error` fields (and the `v=1` / `actor`
  envelope is added at serde-time, not stored in the in-memory
  struct). The in-memory store still keys uniqueness off `sql` for
  adjacent dedup.
- `apps/dbboard` resolves the path at startup, calls `load_tail`, and
  hands the writer to `DbboardApp`. When path resolution fails
  (headless / CI), the app falls back to an in-memory-only store and
  logs the reason — same fallback pattern as `ConnectionAdmin`
  resolution.
- `dbboard-ui` gains a `serde_json` dev-dep usage for tests (the crate
  already pulls it transitively through `reqwest`); no production
  dependency added.
- README and `docs/connections.md` get a short "Query history" section
  noting the file location per OS, the format, and the "queries are
  stored verbatim, including any string literals" warning.
- Web mirror brief at `.claude/issues/0003-web-history-schema-mirror.md`
  lands in the same PR.
- Roadmap Phase 2 history bullet flips from "Stage 1, persistence
  deferred" to "Stage 2 persisted via ADR-0017". Phase 2 exit
  criteria still hold (UI does not know "Turso").
- SemVer impact (ADR-0011): additive. The on-disk format becomes a
  semver-tracked surface — a future `v=2` is a minor bump if reader
  forward-compat holds, major if a `v=1` reader would mis-parse.

---

## ADR-0018 — Neon as a flavored kind over `dbboard-postgres`

**Status:** Accepted (2026-06-04). First Phase 3 ADR. Refines ADR-0008
(one crate for PostgreSQL-wire databases) and discharges the Phase 3
roadmap promise "Connection picker recognises adapter kind" plus the
`docs/architecture.md` invariant that adapter identifiers (`turso`,
`neon`, `supabase`) are stable strings.

**Context.** ADR-0008 collapsed every PostgreSQL-wire database into a
single `dbboard-postgres` crate. CockroachDB shipped first; Neon was
called out as "arriving cheaply" because it accepts the same
`postgresql://…` URL. After Phase 2 closed (PR #10), two unresolved
threads point at the same gap:

1. `docs/architecture.md` § *Parity with `dbboard-web`* promises stable
   adapter id strings — explicitly listing `"neon"` and `"supabase"`
   alongside `"turso"`. The current `PostgresAdapter::id()` always
   returns `"postgres"`, so a Neon connection surfaces as `postgres`
   in `GET /capabilities` and in any future capability-aware label.
2. `docs/roadmap.md` Phase 3 has a checkbox "Connection picker
   recognises adapter kind"; `docs/compatibility.md` defers Neon's
   "connection picker quirks" to Phase 3 explicitly.

A separate `dbboard-neon` crate was considered and rejected: ADR-0008
already justified the consolidation, and there is no Neon-specific
SQL/protocol code to host. What we actually need is a way to label
the same adapter differently when the user said "this is Neon".

**Decision.**

- Add a `flavor: &'static str` field to `PostgresAdapter`, returned
  verbatim from `DatabaseAdapter::id()`. The default constructor
  `PostgresAdapter::connect` keeps `flavor = "postgres"`; a sibling
  constructor `PostgresAdapter::connect_neon` sets `flavor = "neon"`.
  Both go through identical TLS-hardening, pooling, and query paths —
  the flavor is metadata, not behaviour.
- Add `ConnectionKind::Neon { keyring_url_ref }` to the
  `connections.toml` schema. Its shape is byte-identical to
  `ConnectionKind::Postgres`; the only difference is the `kind`
  discriminator. The TOML schema version stays at `v = 1`: this is
  additive — old files with `kind = "postgres"` keep parsing, and a
  `v = 1` reader that does not know about `kind = "neon"` already
  rejects unknown kinds loudly per ADR-0013, which is the correct
  behaviour (a Neon entry should not silently fall through).
- Add a `DBBOARD_NEON_URL` environment variable. Resolution order in
  `dbboard-server::config`:
  1. `DBBOARD_NEON_URL` (PostgreSQL-wire, flavor = `"neon"`).
  2. `DBBOARD_PG_URL` (PostgreSQL-wire, flavor = `"postgres"`).
  3. The `DBBOARD_D1_*` trio, then `DBBOARD_TURSO_PATH`, then
     `DBBOARD_CONNECTION=<id>`, then single-entry auto-select,
     then the in-memory libSQL fallback (unchanged from ADR-0013).
  `DBBOARD_NEON_URL` sits **above** `DBBOARD_PG_URL` because it is the
  *more specific* declaration: a developer who set both clearly meant
  "this Neon instance," and silent demotion to `postgres` would
  contradict ADR-0008's principle that the user's stated intent
  drives labeling.
- `ConnectionAdmin` treats Neon as a peer of Postgres: same secret
  field (`url` → `keyring.<id>.url`), same rollback semantics, same
  `KindMismatch` rule on update (kind cannot change in-place).
- The Connections UI gains a Neon row in the kind dropdown and a
  Fluent key `connections-add-kind-neon` returning `"Neon"`. The
  string is the same in every locale (proper noun); the key still
  goes through `t!()` for layout discipline.

**Alternatives considered.**

- *Reuse `kind = "postgres"` and infer Neon from the URL.* Rejected:
  silent inference would hide misconfiguration (e.g. a self-hosted
  Postgres reached through a Neon-shaped proxy URL), and the user's
  explicit intent is the contract.
- *Bump `connections.toml` to `v = 2`.* Rejected: nothing in the file
  shape changes — only the enum gains a discriminator. ADR-0013's
  strict-unknown-kinds rule already handles forward-compat.
- *New `dbboard-neon` crate.* Rejected (see Context): no Neon-specific
  SQL/protocol code to host; would reintroduce the duplication
  ADR-0008 collapsed.
- *Demote `DBBOARD_NEON_URL` below `DBBOARD_PG_URL`.* Rejected:
  ordering by specificity is the only rule that does not surprise a
  reader of `connections.md`.

**Consequences.**

- `PostgresAdapter::id()` no longer trivially returns `"postgres"`. A
  capabilities consumer that pattern-matches on `"postgres"` will miss
  Neon; web mirror is unaffected because the HTTP contract does not
  enumerate ids — it surfaces whatever string the adapter reports.
- The flavor pattern generalises: when Supabase's pg-wire path lands
  in Phase 3, a `connect_supabase` constructor + `kind = "supabase"`
  follows the same recipe with no further ADR.
- `docs/compatibility.md` drops the Phase 3 callout on the Neon row
  and gains a "live test gated on `DBBOARD_NEON_URL`" note.
- `docs/connections.md` gains a Neon example entry and lists
  `DBBOARD_NEON_URL` in the resolution-order section.
- `crates/dbboard-postgres/README.md` is created with a Neon section
  noting that the connection string must include `sslmode=require` (or
  the wider `verify-full`) — Neon's proxy refuses plaintext.
- No new external crate enters the dependency tree.
- SemVer impact (ADR-0011): additive at every surface (HTTP, TOML,
  trait id strings, env vars). Minor bump on the next release.
- Web mirror: none required. The HTTP contract is unchanged; ADR-0012
  flat capabilities flags are unaffected. The shared per-record
  history schema (ADR-0017) is unaffected — `conn` is the
  connection's `id`, not the adapter id, so flavor labeling never
  bleeds into history records.

---

## ADR-0019 — Supabase as a flavored kind over `dbboard-postgres`

**Status:** Accepted (2026-06-04). Second Phase 3 ADR. Mechanically
applies the ADR-0018 recipe to Supabase and closes out the Phase 3
roadmap row "`dbboard-supabase` (REST + sqlx hybrid)" by **splitting
its scope in two**: the pg-wire SQL path lands now as a flavored
kind; the REST integration (auth / storage / realtime / edge
functions) is deferred to a separate future ADR.

**Context.** Supabase is a managed Postgres service that exposes
two surfaces: a normal pg-wire endpoint (direct or via PgBouncer
session/transaction pooler) and a REST API (PostgREST + GoTrue +
Realtime + Storage + Edge Functions). The pg-wire surface is
indistinguishable from vanilla Postgres at the SQL/protocol layer.
The roadmap row "`dbboard-supabase` (REST + sqlx hybrid)" predates
ADR-0018, when the assumption was that each new adapter would get
its own crate. After ADR-0018, the flavored-kind recipe is the
cheaper and more consistent landing pad for the pg-wire half.

The REST half is a different shape entirely:

1. It would require new `DatabaseAdapter` trait surface (or a sibling
   trait) for non-SQL operations (auth listing, bucket browsing,
   realtime subscriptions, function invocation).
2. The HTTP contract (`docs/api-contract.md`) would have to grow new
   endpoint families — `/auth/users`, `/storage/buckets`,
   `/realtime/channels`, `/functions` — which is exactly the
   per-capability extension ADR-0012 reserved for later.
3. It needs `dbboard-core::Capabilities` flags (`has_auth`,
   `has_storage`, `has_realtime`) to flip true, with matching UI
   surfaces (new panes / tabs) to drive those endpoints.
4. It mandates a cross-repo coordination window: the web mirror
   would need a matching contract delta plus a per-feature web
   implementation, because today's contract pretends those areas
   do not exist on either side.

Bundling all of that into Phase 3 would creep into Phase 4
territory and stall the roadmap closeout the user actually wants
("the trait is proven by three live adapters"). The pg-wire half
alone clears every Phase 3 exit criterion.

**Decision.**

- Add `FLAVOR_SUPABASE = "supabase"` to `crates/dbboard-postgres`
  alongside `FLAVOR_POSTGRES` and `FLAVOR_NEON`. Expose a
  `PostgresAdapter::connect_supabase` constructor that delegates to
  the same internal `connect_with_flavor` path. Wire protocol, SQL
  surface, TLS hardening (`Prefer → Require`), pool config, dynamic
  text decoding, and row cap are byte-identical to the Postgres /
  Neon paths.
- Add `ConnectionKind::Supabase { keyring_url_ref }` to the
  `connections.toml` schema. Byte-identical shape to `Postgres` /
  `Neon`; only the `kind` discriminator differs. Schema version
  stays `v = 1` — additive per the ADR-0018 / ADR-0013 rule. Cross-
  kind edits (Postgres ↔ Neon ↔ Supabase) remain rejected with
  `KindMismatch` to preserve ADR-0016 § 3 rollback story.
- Add `DBBOARD_SUPABASE_URL` to the resolver's env precedence
  ladder, ranked alongside `DBBOARD_NEON_URL` (both above
  `DBBOARD_PG_URL`). Within the two, Supabase sits **below** Neon:
  alphabetical stability is the only tiebreaker that does not
  require ad-hoc justification, and a developer who set **both**
  has either misconfigured or is debugging — either way the noisier
  failure (the env-precedence error path already exists for
  contradictory settings) is better than silent demotion.
  Resolution order becomes:
  1. `DBBOARD_NEON_URL` (PostgreSQL-wire, flavor = `"neon"`).
  2. `DBBOARD_SUPABASE_URL` (PostgreSQL-wire, flavor = `"supabase"`).
  3. `DBBOARD_PG_URL` (PostgreSQL-wire, flavor = `"postgres"`).
  4. The `DBBOARD_D1_*` trio, then `DBBOARD_TURSO_PATH`, then
     `DBBOARD_CONNECTION=<id>`, then single-entry auto-select, then
     the in-memory libSQL fallback.
- `BackendConfig::Supabase { url: String }` variant in
  `dbboard-server`, `Debug`-redacted as `Supabase(<redacted>)`. The
  `connect_adapter` dispatch routes through
  `PostgresAdapter::connect_supabase`. `label_for` returns
  `"env:supabase"` for env-resolved Supabase backends.
- The Connections UI gains a Supabase row in the kind dropdown.
  Reuses the existing `connections-field-pg-url` Fluent key for the
  URL field — no new tier-1 i18n string, all 11 locales stay in
  sync without an i18n bump. A new `connections-add-kind-supabase`
  key returns `"Supabase"` verbatim in every locale (proper noun,
  same shape as the Neon key).
- Capability flags stay at default `false`. `has_auth`, `has_storage`,
  `has_realtime` reporting `true` is a future ADR's job and pairs
  with the REST surface, not the flavor label.
- `docs/compatibility.md` promotes the Supabase row from "Phase 3"
  callout to **Tier 1**: live test gated on `DBBOARD_SUPABASE_URL`,
  same wire/SQL/TLS profile as Neon. Postgres major support
  inherits from the shared Postgres-wire row (`17`, `16` Tier 1;
  `15` Tier 2).
- `docs/connections.md` gains a Supabase example entry and lists
  `DBBOARD_SUPABASE_URL` in the resolution-order section.
- `crates/dbboard-postgres/README.md` flavor table grows a third
  row. Supabase notes: TLS required (Supabase enforces it server-
  side); both **direct** (`db.<ref>.supabase.co:5432`) and **pooler**
  (`aws-0-<region>.pooler.supabase.com:6543`, transaction mode) URLs
  work — pick the same one the project's other tooling uses to
  avoid prepared-statement surprises in transaction-pool mode.
- `docs/roadmap.md` Phase 3 row "`dbboard-supabase` (REST + sqlx
  hybrid)" is split: the pg-wire half is checked off here; the REST
  half is recorded as deferred under a TBD ADR (no Phase change —
  Phase 3 closes on three live adapters per the original exit
  criterion).

**Alternatives considered.**

- *Ship the REST + sqlx hybrid in this ADR.* Rejected (see Context):
  scope-creeps into Phase 4 (trait extension, contract delta, web
  mirror, new UI surfaces). The user's stated Phase 3 goal is to
  prove the trait by three live adapters; the pg-wire half clears
  that on its own.
- *Docs-only ("Supabase pg-wire works through `DBBOARD_PG_URL`").*
  Rejected: asymmetric with ADR-0018's reasoning. The same arguments
  that made Neon a first-class kind (`id()` stability, capability
  surface labelling, Connection picker label) apply verbatim to
  Supabase. Docs-only would require re-flavoring later when the
  REST half lands.
- *Separate `dbboard-supabase` crate.* Rejected (per ADR-0018
  generalisation note line 1507): no Supabase-specific pg-wire
  code to host. The REST surface, if and when it lands, is a
  separate concern that may or may not warrant a new crate
  (depending on whether it shares Postgres metadata calls).
- *Force PgBouncer transaction-pool semantics.* Rejected: the URL
  already encodes the choice (`:6543` vs `:5432`); the adapter must
  not second-guess the operator. Documented in the README instead.

**Consequences.**

- Phase 3 roadmap closes: three live adapters proven (Turso, D1,
  PostgreSQL-wire shared by Postgres / Neon / Supabase), Connection
  picker recognises adapter kind (delivered by ADR-0018 generalised
  by this ADR).
- The REST integration becomes a **future ADR slot**. Likely sequence:
  (a) capability flag extension ADR (specifies which flags flip and
  what they enable in the UI), (b) HTTP contract delta ADR for the
  new endpoint families (with a cross-repo handoff brief in the
  `0001`/`0002` format), (c) per-feature implementation. Realistic
  earliest landing is post-Phase 4, since AI integration (Phase 4)
  is already the next named milestone.
- `docs/compatibility.md` Supabase row gains an explicit
  Postgres-major matrix inherited from the shared row, with the
  service-level commitment that "we follow Supabase's own supported
  Postgres majors."
- No new external crate enters the dependency tree.
- SemVer impact (ADR-0011): additive at every surface (HTTP, TOML,
  trait id strings, env vars). Same minor bump that ADR-0018 already
  earmarked.
- Web mirror: none required. HTTP contract unchanged; ADR-0012
  flat capabilities flags unaffected (all still default-false at
  the server); shared per-record history schema (ADR-0017)
  unaffected — `conn` is the connection's `id`, not the adapter id.
  When the REST integration eventually lands, **that** ADR will
  emit a fresh handoff brief; this one does not.

---

## ADR-0021 — Aurora DSQL as a flavored kind over `dbboard-postgres`

**Status:** Accepted (2026-06-04). Third Phase 3 ADR. Mechanically
applies the ADR-0018 / ADR-0019 recipe to AWS **Aurora DSQL** — a
managed, serverless, distributed Postgres-wire database (AWS GA
2025-05-22). Like ADR-0019, this ADR delivers only the pg-wire SQL
path; the AWS SDK auto-token-refresh integration is explicitly
deferred to a future ADR.

**Context.** Aurora DSQL is AWS's serverless Postgres-wire offering,
positioned alongside Neon and Supabase as a managed-Postgres option
worth surfacing as a first-class connection kind. The SQL/protocol
layer is indistinguishable from vanilla Postgres — sqlx talks to it
through the same wire path, and the existing TLS hardening (`Prefer
→ Require`) covers its TLS-mandatory posture.

Aurora DSQL's *only* notable departure from Neon / Supabase is the
auth mechanism: it does not accept static passwords. The "password"
field in the connection URL must carry a short-lived **IAM
authentication token** (~15 minute lifetime), generated either by
the AWS CLI (`aws dsql generate-db-connect-admin-auth-token` /
`generate-db-connect-auth-token`) or by an AWS SDK call. Two paths
exist for handling this in dbboard:

1. **Static-URL flavor** (this ADR). The user pre-generates a token
   via the AWS CLI and pastes the resulting `postgres://…` URL into
   dbboard, exactly like Neon / Supabase. The token expires after
   ~15 minutes; the user re-pastes a refreshed URL when it does.
   Mechanical, zero new dependencies, ships in one PR.
2. **SDK-integrated adapter** (deferred). dbboard depends on
   `aws-config` + `aws-sdk-dsql`, generates tokens on demand, and
   refreshes them automatically. Better UX but adds a multi-crate
   AWS SDK dependency (with its own TLS / async-runtime fingerprint)
   and is materially more work — exactly the kind of scope creep
   that ADR-0019 dodged by deferring the Supabase REST surface.

For Phase 3 we ship path 1. Path 2 becomes a future ADR slot
analogous to "Supabase REST" — a real ADR with its own deps,
contract impact (capabilities flag for IAM-auth?), and UI affordance
(refresh hint? expiry timer?).

**Decision.**

- Add `FLAVOR_AURORA_DSQL = "aurora-dsql"` to `crates/dbboard-postgres`
  alongside `FLAVOR_POSTGRES`, `FLAVOR_NEON`, and `FLAVOR_SUPABASE`.
  Expose a `PostgresAdapter::connect_aurora_dsql` constructor that
  delegates to the same internal `connect_with_flavor` path. Wire
  protocol, SQL surface, TLS hardening, pool config, dynamic text
  decoding, and row cap are byte-identical to the other flavors.
- Add `ConnectionKind::AuroraDsql { keyring_url_ref }` to the
  `connections.toml` schema. Byte-identical shape to `Postgres` /
  `Neon` / `Supabase`; only the `kind` discriminator differs (TOML
  literal: `kind = "aurora-dsql"`). Schema version stays `v = 1`.
  Cross-kind edits (Postgres ↔ Neon ↔ Supabase ↔ Aurora DSQL) remain
  rejected with `KindMismatch`.
- Add `DBBOARD_AURORA_DSQL_URL` to the resolver's env precedence
  ladder. Among the four pg-wire env vars the order is **alphabetical
  by flavor name** — the same tiebreaker ADR-0019 established. So
  the resolution order becomes:
  1. `DBBOARD_AURORA_DSQL_URL` (flavor = `"aurora-dsql"`).
  2. `DBBOARD_NEON_URL` (flavor = `"neon"`).
  3. `DBBOARD_SUPABASE_URL` (flavor = `"supabase"`).
  4. `DBBOARD_PG_URL` (flavor = `"postgres"`).
  5. The `DBBOARD_D1_*` trio, then `DBBOARD_TURSO_PATH`, then
     `DBBOARD_CONNECTION=<id>`, then single-entry auto-select, then
     the in-memory libSQL fallback.
- `BackendConfig::AuroraDsql { url: String }` variant in
  `dbboard-server`, `Debug`-redacted as `AuroraDsql(<redacted>)`.
  The `connect_adapter` dispatch routes through
  `PostgresAdapter::connect_aurora_dsql`. `label_for` returns
  `"env:aurora-dsql"` for env-resolved Aurora DSQL backends.
- The Connections UI gains an Aurora DSQL row in the kind dropdown.
  Reuses the existing `connections-field-pg-url` Fluent key for the
  URL field — no new tier-1 i18n string. A new
  `connections-add-kind-aurora-dsql` key returns `"Aurora DSQL"`
  verbatim in every locale (proper noun, same shape as the Neon /
  Supabase keys).
- Capability flags stay at default `false`. IAM-token-aware
  capability flags (`has_iam_auth`, etc.) are a future ADR's job and
  pair with path 2, not the flavor label.
- `docs/compatibility.md` adds an Aurora DSQL row: live test gated
  on `DBBOARD_AURORA_DSQL_URL`. Aurora DSQL does not publish a
  user-visible Postgres major like vanilla Postgres does; AWS
  documents it as Postgres-protocol-compatible without committing
  to a specific server version, so the row tracks "AWS GA" as a
  single moving target (the same posture `docs/compatibility.md`
  already uses for Cloudflare D1 and Turso platform).
- `docs/connections.md` gains an Aurora DSQL example entry and
  lists `DBBOARD_AURORA_DSQL_URL` in the resolution-order section.
- `crates/dbboard-postgres/README.md` flavor table grows a fourth
  row. Aurora DSQL notes: TLS required (AWS enforces it
  server-side); the URL's password field carries a short-lived IAM
  auth token; regenerate it with `aws dsql
  generate-db-connect-admin-auth-token --hostname <cluster>.dsql.<region>.on.aws
  --region <region>` (or `generate-db-connect-auth-token` for
  non-admin roles); typical token TTL is ~15 minutes, so the URL
  in `connections.toml` will need periodic refresh until path 2
  lands.
- `docs/roadmap.md` Phase 3 row gains an explicit Aurora DSQL
  bullet alongside Neon (ADR-0018) and Supabase (ADR-0019), making
  Phase 3 close on **four** pg-wire flavors (Postgres / Cockroach,
  Neon, Supabase, Aurora DSQL) plus Turso and D1.
- **Project `README.md` env-vars section gains Aurora DSQL, plus
  the Neon and Supabase entries the previous two ADRs neglected to
  mirror up to the project README.** "Supported Databases" list
  gains Aurora DSQL alongside the existing entries.

**Alternatives considered.**

- *Ship the SDK-integrated adapter in this ADR.* Rejected (see
  Context, path 2): pulls `aws-config` + `aws-sdk-dsql` (and the
  full AWS SDK TLS / runtime stack) into the dependency graph for
  what is structurally a one-line difference at the SQL layer. Best
  handled as its own ADR after `cargo deny` / `cargo audit` review
  of the SDK's transitive deps.
- *Docs-only ("Aurora DSQL works through `DBBOARD_PG_URL`").*
  Rejected, same reasoning ADR-0019 used: `id()` stability,
  capability surface labelling, connection picker label, and history
  attribution all benefit from the flavor being a first-class
  string. Docs-only would force a re-flavoring when path 2 lands.
- *Separate `dbboard-aurora-dsql` crate.* Rejected: no Aurora-DSQL-
  specific pg-wire code to host. If and when path 2 lands, the
  SDK-integration code might warrant its own crate — but that's a
  decision for that ADR, not this one.
- *Rank `DBBOARD_AURORA_DSQL_URL` by recency-of-ADR rather than
  alphabetically.* Rejected: recency is unstable as a tiebreaker
  (every new flavor would shuffle the order), and surprise from a
  changed precedence is worse than from a stable alphabetical rule.

**Consequences.**

- Phase 3 closes on **four pg-wire flavors** plus Turso and D1.
  Exit criterion ("the trait is proven by N live adapters") is
  strictly stronger than the original wording.
- The SDK-integrated path becomes a **future ADR slot**, analogous
  to the deferred Supabase REST ADR. When it lands, its likely
  shape: (a) declare AWS SDK dep + record license / advisory check
  in `deny.toml`, (b) add an `auth_token_provider` trait /
  capability flag so the UI can render an "auto-refresh on" badge,
  (c) optional `dbboard-aurora-dsql` crate if the SDK glue grows
  beyond a single module.
- `docs/compatibility.md` Aurora DSQL row tracks "AWS GA" as a
  moving target, with the service-level commitment that "we follow
  Aurora DSQL's documented Postgres-protocol compatibility" — same
  posture as the D1 row.
- No new external crate enters the dependency tree.
- SemVer impact (ADR-0011): additive at every surface (HTTP, TOML,
  trait id strings, env vars). Same minor bump category as
  ADR-0018 / ADR-0019.
- Web mirror: none required. HTTP contract unchanged; ADR-0012 flat
  capabilities flags unaffected; shared per-record history schema
  (ADR-0017) unaffected — `conn` is the connection's `id`, not the
  adapter id. When the SDK-integrated path eventually lands, **that**
  ADR will emit a fresh handoff brief; this one does not.

---

## ADR-0020 — In-process connection switching (supersedes ADR-0016's Stage 1 mental model)

**Status:** Accepted (2026-06-04). Supersedes ADR-0016 decision points
1, 2, and 3 (process-per-connection mental model, in-app switching
out of scope, list-only Stage 1 surface). The rest of ADR-0016
remains in force.

### Context

ADR-0016 (2026-06-03) shipped Add / Edit / Delete on the connections
window and explicitly deferred in-app switching to a "Stage 2 ADR if
usage warrants." First-real-world-use feedback (2026-06-04) made
clear that usage warrants it now: after saving a connection the user
hits a dead end — the connections window lists `[ Edit | Delete ]`
per row with no obvious way to *use* the connection just saved. The
HeidiSQL multi-process model assumed familiarity that the
maintainer's actual workflow does not have, and every other desktop
DB client the maintainer reaches for (DBeaver, TablePlus, DataGrip,
HeidiSQL itself when used via "open as new tab") swaps the active
connection inside one window. The dead-end UX is the failure mode
ADR-0016's "Alternatives considered" listed under "tabbed
multi-connection in one process" — except it shows up at a far
lower complexity bar: the user does not need *multiple* concurrent
connections, just *the ability to use the one they saved*.

### Decision

1. **The connections window grows a "Connect" affordance per row.**
   Each row's action cluster becomes `[ Connect | Edit | Delete ]`.
   Clicking Connect switches the **running process's** active
   connection to that row's `id`. The currently active row is
   visually marked (highlight + check mark). The window itself stays
   open so the user can confirm the switch and pick another if
   needed.

2. **Switching is in-process, not a new window or process restart.**
   `apps/dbboard` constructs a new `Arc<dyn DatabaseAdapter>` via
   `ConnectionAdmin` (already shipped) and hands it to
   `dbboard-server` through a shared swap point — no admin HTTP
   endpoint, no second loopback bind, no second egui window. The
   HTTP contract (`docs/api-contract.md`) is unchanged.

3. **The server's adapter handle becomes swappable.** The current
   `Arc<dyn DatabaseAdapter>` field on `Backend` becomes
   `Arc<ArcSwap<dyn DatabaseAdapter>>` (or an equivalent
   `Arc<RwLock<Arc<dyn DatabaseAdapter>>>` — the choice is internal).
   Every request handler reads the current adapter through that
   handle at the start of the request and operates on the captured
   `Arc` for the duration of the request. This preserves the
   "one adapter per request" invariant ADR-0012 relied on, while
   letting the *next* request see the swapped-in adapter.

4. **In-flight queries are not interrupted.** A switch issued while
   a query is in flight does not cancel that query; the running
   request keeps the captured `Arc` and finishes against the old
   adapter. The new adapter takes effect for the *next* request the
   UI issues. This is the cheapest correct behaviour and matches
   how users mentally model "I clicked switch, the next thing I
   run goes to the new DB."

5. **No persistence of "last active."** The switch is per-session.
   On next process launch the existing precedence chain (env vars
   > `DBBOARD_CONNECTION=<id>` > single-entry auto-select > Turso
   `:memory:`) decides the startup adapter, same as today. A future
   ADR may persist a "last active connection" hint if usage data
   argues for it.

6. **History recording follows the active connection at write time.**
   `history.jsonl` records each entry with the `conn` field set to
   the active connection's `id` at the moment the query ran. ADR-0017
   already keyed history off `connection.id` rather than adapter id,
   so no schema change.

7. **The wire mechanism for the swap is the existing
   `Command` / `Reply` channel pair, not a new HTTP endpoint.** The
   UI sends `Command::SwitchConnection { id }` over the channel that
   already carries `Command::RunQuery` etc.; `apps/dbboard` resolves
   the connection, builds the adapter, swaps the server's handle,
   and replies with `Reply::ConnectionSwitched { id }` or
   `Reply::Error { ... }`. **No HTTP contract change, no web
   mirror.** The web sibling has its own connection-switching story
   over its own admin surface; this ADR does not constrain it.

### Alternatives considered

- **`POST /admin/switch` HTTP endpoint.** Rejected: adds an admin
  surface that requires a web mirror (HTTP contract policy in
  `CLAUDE.md` and ADR-0009), and the swap is a purely local-process
  concern. The egui UI and the local server live in the same binary;
  channel-based wiring is direct, typed, and doesn't pollute the
  shared contract.
- **Spawn a new `dbboard.exe` process per switch (the original
  "new window" pitch).** Rejected as the primary path: ADR-0016
  already showed this matches the maintainer's HeidiSQL-style
  workflow, but first-use feedback shows it does not match
  *expectations* — users expect "Connect" to act on the current
  window. Multi-process is still available to the maintainer
  (launch another `dbboard.exe` from the command line with a
  different `DBBOARD_CONNECTION=<id>`); this ADR does not remove
  that, it just stops *requiring* it.
- **Tabbed multi-connection in one process.** Still rejected for
  now — same reasoning as ADR-0016. Single-active-connection with
  fast in-place switching covers the actual use case without the
  N-pane UI cost. Revisitable as a future ADR if usage warrants.
- **Block the switch until in-flight queries finish, instead of
  letting them run on the old adapter.** Rejected as user-hostile:
  the existing row cap (`MAX_RESULT_ROWS`) plus the
  fail-fast network paths make a "queries always finish quickly"
  invariant strong enough that simple "switch takes effect on
  next request" wins on both UX and implementation cost.

### Consequences

- `dbboard-server` learns a `swap_backend(new: Arc<dyn DatabaseAdapter>)`
  entry point. Request handlers read the current adapter through an
  `ArcSwap` (or equivalent) and capture an `Arc` for the request's
  lifetime. No HTTP types change.
- `apps/dbboard` learns `Command::SwitchConnection { id }`,
  `Reply::ConnectionSwitched { id }`. The existing connect-at-startup
  flow is unchanged: startup still resolves the adapter once and
  hands it to the server through the same swap point that the
  in-process switch later uses.
- `dbboard-ui` `ConnectionsWindow`:
  - per-row `[ Connect | Edit | Delete ]`,
  - active-row highlight + check mark (`connections-row-active` and
    `connections-button-connect` Fluent keys added to all 11 locales
    — ADR-0015 tier 1+2 stay in sync),
  - removes the "変更は dbboard の次回起動時から有効になります"
    notice on the connections window (it was only true under
    ADR-0016 — under ADR-0020 it's misleading; the *form's* Save
    still requires a Connect to activate, which the row state now
    expresses visibly).
- `ConnectionAdmin` (`dbboard-config`, ADR-0016) is unchanged. The
  only change is who calls it: previously only startup, now also
  the UI-driven switch.
- `dbboard-web` sibling: **no contract or wire change**. ADR-0020
  joins the ADR-0013 / ADR-0015 / ADR-0016 / ADR-0018 / ADR-0019
  category of desktop-side-only changes; `dbboard-web-state.md`
  memory records it the same way. No `0NNN-web-*` issue file.
- ADR-0012's "one `Arc<dyn DatabaseAdapter>` per process lifetime"
  becomes "one `Arc<dyn DatabaseAdapter>` per request"; the trait
  itself is unchanged. The invariant ADR-0012 actually needs
  ("a request operates on a fixed adapter from start to end") is
  preserved through the per-request capture.
- Roadmap: no new phase. This is UX polish on Phase 2 — Stage 2 of
  the "Connection management UI" line item that ADR-0016 left
  half-shipped. `docs/roadmap.md` Phase 2 row gets a short
  parenthetical noting ADR-0020 closes the Stage 1 dead-end.
- Future work: `0004-runtime-locale-switcher.md` queues the
  analogous fix on the i18n side (ADR-0015 chose startup-only
  resolution; once ADR-0020 lands, the same in-process-mutation
  precedent makes a runtime locale switcher trivial — same shape,
  smaller blast radius).

## ADR-0022 — Runtime locale switcher (revises ADR-0015's startup-only resolution)

**Status:** Accepted (2026-06-11). Supersedes ADR-0015's "startup-only
resolution" decision. The Stage 1 locale list, the `fluent-rs` +
`i18n-embed` framework, the `DBBOARD_LANG` startup precedence, and
the CJK font strategy all remain in force.

### Context

ADR-0015 (2026-06-03) shipped 11 locales but resolved them once at
startup: `DBBOARD_LANG` → OS → `en`. Changing language required
restarting the binary with a different env var. First-real-world-use
feedback (2026-06-04, the same review session that produced
ADR-0020): "11 言語に対応したのに切り替えのメニューバーもないですね"
— a multilingual UI without a switcher reads as "shipped capability,
missing UX". Same shape as the ADR-0016 → ADR-0020 dead-end the
connections window had.

The fix was queued as `.claude/issues/0004-runtime-locale-switcher.md`
with one explicit blocker: wait until ADR-0020 lands so the
in-process-mutation precedent (mutate a running process's global state,
no restart) is established. ADR-0020 shipped in PR #14 on 2026-06-11;
this ADR captures the now-unblocked switcher.

### Decision

1. **The menu bar gains a Language submenu** next to the Connections
   button. The submenu label is **translated** (`Language` / `言語` /
   `언어` / `语言` / `語言` / `Sprache` / `Langue` / `Idioma` /
   `Idioma` / `Язык` / `Lingua`) so a user who landed in the wrong
   locale can still recognise the entry point.

2. **Submenu entries are the 11 ADR-0015 locales by their native
   names** (`English`, `日本語`, `한국어`, `中文 (简体)`,
   `中文 (繁體)`, `Deutsch`, `Français`, `Español`,
   `Português (Brasil)`, `Русский`, `Italiano`). The active locale
   gets a `✓` prefix. Order is fixed (Tier 1 then Tier 2 from
   ADR-0015) so the list does not reshuffle as the active locale
   changes.

3. **Switching is in-process and synchronous on the UI thread.**
   Clicking a row calls `dbboard_i18n::set_language(tag)` directly,
   which delegates to the same `i18n_embed::select` the startup path
   uses. No `Command` / `Reply` round trip — unlike ADR-0020's
   connection switch there is no I/O, no adapter rebuild, just a
   reselect against an already-loaded bundle cache. The UI then asks
   egui for `request_repaint()` so the next frame redraws every
   `t!()` against the new bundle.

4. **`DBBOARD_LANG` still wins at startup.** The startup precedence
   from ADR-0015 (`DBBOARD_LANG` → OS → `en`) is unchanged. The
   runtime switcher only mutates the *current session*. Setting
   `DBBOARD_LANG=ja` and then picking `Deutsch` from the menu gives
   you `de` for the rest of the session and `ja` again on next launch.

5. **No persistence of "last picked" locale.** Same shape as
   ADR-0020's "no persistence of last-active connection" — runtime
   selection is per-session. A future ADR may persist a "last
   active locale" hint if usage data argues for it; until then
   `DBBOARD_LANG` is the persistence story.

6. **Native names are constants in `apps/dbboard`, not translation
   keys.** `日本語` is the same string regardless of which locale
   the menu is currently rendering in. Putting native names in
   `.ftl` files would either duplicate them across 11 files
   (wasteful, prone to drift) or pin them to one locale and hide
   the affordance for misrouted users. Native-name-of-self does not
   translate per active locale — by design, it is the recognition
   signal.

7. **No CJK font re-registration.** `apps/dbboard`'s startup
   `install_cjk_font` *appends* a CJK fallback to egui's font stack
   (ADR-0015). The stack covers every CJK locale at once; a
   `ja` → `zh-CN` switch does not need a different font. Latin and
   Cyrillic are covered by the bundled `Ubuntu-Light` regardless of
   locale.

8. **No HTTP contract change, no web mirror.** Same category as
   ADR-0015 / ADR-0016 / ADR-0020: a desktop-side presentation-only
   change. `DbError` text stays English (the ADR-0009 wire contract).
   `dbboard-web-state.md` records this as another "no mirror
   needed" entry.

### Alternatives considered

- **Route the switch through ADR-0020's `Command` / `Reply` channel
  pair.** Rejected: locale switching has no I/O and does not need
  the worker thread. Going through the channel would add a frame of
  UI lag (the worker has to deliver `Reply::LocaleSwitched` before
  the UI repaints), serialise it behind in-flight `RunQuery`
  traffic, and require a new `Command::SwitchLocale` variant for
  no payoff. The mutation is local and synchronous; treat it that
  way.

- **Persist the runtime-picked locale across launches.** Deferred.
  Same reasoning as ADR-0020's "no persistence" decision: ship the
  minimum, watch usage, add persistence later if the data argues
  for it. Until then `DBBOARD_LANG` is the durable override.

- **Restart the process to apply the new locale (a Language
  submenu that re-launches the binary).** Rejected on first
  principles — ADR-0020 already established that "first-use
  feedback shows users expect a Connect button to act on the
  current window". A restart for a label-only change is even less
  defensible than a restart for an adapter change.

- **Translate native names per active locale (`Japanese` /
  `Japonais` / `Japanisch` / …).** Rejected: the recognition
  signal *is* the locale's name in itself. Translating it removes
  the affordance for a user who cannot read the current locale.

- **Add `ar` / `hi` along with the switcher.** Still rejected;
  same Stage 2 deferral as ADR-0015. The switcher does not change
  the Stage 1 locale set.

### Consequences

- `dbboard-i18n` gains `set_language(tag: &str)` and
  `current_language() -> LanguageIdentifier`. Both delegate to the
  global `FluentLanguageLoader`; the existing `init()` already
  supports reselect, so the surface change is only ergonomic. A
  unit test covers a `ja → en → zh-CN` swap and asserts both
  `t!()` output and `current_language()` flip on every step.
- A new translation key `language-menu` is added to all 11
  `.ftl` files for the menu-bar label. No other translation keys
  change. ADR-0015 tier 1 + tier 2 stay in sync (the rule from
  ADR-0020's `Consequences` block).
- `apps/dbboard` gains a `SUPPORTED_LOCALES: &[(&str, &str)]`
  constant table and a `language_menu` UI helper next to
  `install_cjk_font`. The menu bar wiring is one extra call inside
  the existing `egui::MenuBar::new().ui(...)`.
- `dbboard-ui` is unchanged. The switcher lives entirely in
  `apps/dbboard` (the binary) because `dbboard-ui` is
  binary-agnostic by design (ADR-0002, ADR-0009).
- ADR-0015 status block is updated to "Superseded in part by
  ADR-0022 for the startup-only resolution". The rest of ADR-0015
  (locale list, framework, env precedence, font strategy) is
  unchanged.
- Roadmap: no new phase. UX polish on Phase 2 — same row category
  as ADR-0020. `docs/roadmap.md` Phase 2 entry adds a short
  parenthetical noting ADR-0022 closes the runtime-switcher gap
  ADR-0015 left open.
- `.claude/issues/0004-runtime-locale-switcher.md` closes against
  this ADR.
- `dbboard-web` sibling: **no contract or wire change**. ADR-0022
  joins the ADR-0013 / ADR-0015 / ADR-0016 / ADR-0018 / ADR-0019 /
  ADR-0020 / ADR-0021 desktop-side-only category. No `0NNN-web-*`
  issue file.
- SemVer impact (ADR-0011): additive. The
  `set_language` / `current_language` API on `dbboard-i18n` is new;
  nothing existing changes signature.

## ADR-0023 — `dbboard-ai` provider trait and the first Anthropic provider

**Status:** Accepted (2026-06-12). Opens Phase 4 (the optional AI
integration layer) by defining the trait surface and committing to
Claude (Anthropic API) as the first provider. Settings UI, persisted
API-key storage, streaming, and multi-provider switching are
explicitly deferred to a Stage 2 ADR.

### Context

`CLAUDE.md` lists AI integration as a workspace layer from the
beginning: *"Pluggable AI provider trait; no hard dependency on any
specific provider."* `docs/roadmap.md` Phase 4 spells out the exit
shape — `dbboard-ai` crate with an `AiProvider` trait, Claude
(Anthropic API) as the first provider, Explain / Suggest commands,
graceful degradation when no provider is configured, default builds
working without any AI dependency at all.

Phases 1, 2, 2.5, and 3 are now closed (Turso / D1 / Postgres /
CockroachDB / Neon / Supabase / Aurora DSQL adapters all ship, the
runtime locale switcher is live, the connection switcher is live).
The Phase 4 layer can be opened without disturbing any of them.

This ADR commits the **trait-and-first-provider** shape. The
implementation work is queued as
`.claude/issues/0005-dbboard-ai-trait-and-anthropic-provider.md`.

### Decision

1. **Two new crates, mirroring `dbboard-core` + adapter crates.**
   `crates/dbboard-ai` is a pure trait crate — no network I/O, no
   concrete provider — exactly the shape of `dbboard-core`.
   `crates/dbboard-anthropic` is the first concrete implementation,
   talking to the Anthropic Messages API over `reqwest`. Future
   providers land as sibling crates (`dbboard-openai`,
   `dbboard-ollama`, …) following the same pattern. The dependency
   rule is the same one ADR-0002 enforces for DB adapters:
   `dbboard-ai` depends on nothing in the workspace; concrete
   providers depend on `dbboard-ai` only.

2. **`AiProvider` trait shape.** `async_trait` + `Send + Sync` so
   `Arc<dyn AiProvider>` is object-safe. Discovery surface mirrors
   `DatabaseAdapter`:
   - `fn id(&self) -> &'static str` — stable provider id
     (`"anthropic"` for the first provider). Used for history
     labels and a future provider picker.
   - `fn capabilities(&self) -> AiCapabilities` — a flat bool
     struct (`has_streaming`, `has_function_calling`, …) defaulting
     to all-false. Same evolutionary recipe as `Capabilities` in
     `dbboard-core`: add a field as additive change when a new
     capability is introduced.

   Stage 1 surface is two required methods:
   - `async fn explain(&self, req: &ExplainRequest) -> AiResult<AiResponse>`
   - `async fn suggest_sql(&self, req: &SuggestRequest) -> AiResult<AiResponse>`

   Streaming follows the optional-capability-accessor pattern from
   `DatabaseAdapter::views` / `views_full` etc.: when Stage 2 adds
   it, the trait grows `fn streaming(&self) -> Option<&dyn
   StreamingProvider> { None }` and existing providers keep
   working without recompile.

3. **In-process wiring, not HTTP-mediated.** The two AI methods are
   called directly from the UI worker thread via
   `Option<Arc<dyn AiProvider>>` held in `apps/dbboard`. They do
   **not** go through `dbboard-server`'s HTTP surface. Reasons:
   - The HTTP contract is the desktop ↔ web shared surface (ADR-0009).
     The web sibling has its own provider story (NestJS-side) so
     mirroring an AI route between the two would not buy parity.
   - Looping AI calls through localhost adds a serialisation /
     deserialisation hop and a DTO layer for zero benefit — they
     are network-bound on the external API call anyway.
   - The precedent is set by ADR-0020 (`swap_backend`) and ADR-0022
     (`set_language`): mutate the running desktop process directly
     when no wire contract is involved.

4. **Anthropic as the first concrete provider.** `dbboard-anthropic`
   ships a `AnthropicProvider` struct holding a `reqwest::Client`,
   the API key, and the model id. Default model is
   `claude-sonnet-4-6` (per `rules/performance.md`'s
   "best coding model" pick); the model is overridable via env var
   so a user can switch without a code change. The crate uses
   `reqwest` directly — the official Anthropic Rust SDK does not
   exist yet, and the Messages API surface area we need (one POST,
   one JSON envelope) is small enough that a community wrapper
   would be additional surface for zero abstraction win.

5. **Stage 1 configuration is env-var-only:
   `DBBOARD_ANTHROPIC_API_KEY` (required) and
   `DBBOARD_ANTHROPIC_MODEL` (optional override).** The provider is
   constructed at `apps/dbboard` startup *only if* the API key env
   var is present. No `connections.toml` analogue. No keyring.
   Stage 2 will add `ai-providers.toml` + `SecretStore` integration
   (ADR-0013 connections.toml is the template) plus a Settings UI
   for picking a provider and managing keys. Mirroring the
   `DBBOARD_TURSO_PATH` → `connections.toml` evolution path —
   env-var-only first, then persisted store.

6. **Graceful degradation = absence of the panel.** `DbboardApp`
   gains an `Option<Arc<dyn AiProvider>>` field set at construction
   time. When `None`, the UI does not render the AI panel at all —
   no "AI unavailable" stub, no greyed-out button. Same pattern as
   the connections window hiding itself when `ConnectionAdmin` is
   absent (headless / CI fallback path in ADR-0016 wiring). No
   runtime fallback ("provider call failed → silently switch off
   AI") — provider call failures surface as `AiError` in the UI.

7. **Stage 1 commands and request payloads.**
   - **Explain** takes the current SQL only: `ExplainRequest { sql:
     String, dialect: Option<String> }`. `dialect` is a hint like
     `"postgres"` or `"sqlite"` derived from the active adapter's
     `id()` so the provider tailors its explanation. Schema is
     **not** passed; explanations of a known SQL string do not
     need the table list and would inflate every prompt.
   - **Suggest** takes a natural-language prompt plus the current
     adapter's `list_tables()` result: `SuggestRequest { prompt:
     String, dialect: Option<String>, schema: Vec<TableInfo> }`.
     Reusing `TableInfo` from `dbboard-core` keeps `dbboard-ai`
     self-contained for the shape (the trait crate re-exports the
     type rather than redefining it). Full DDL extraction (full
     column types, constraints, indexes) is a Stage 2 concern that
     will need a new `DatabaseAdapter::dump_schema` method, queued
     separately.

   Both methods return `AiResponse { text: String, tokens_in: u32,
   tokens_out: u32 }`. `tokens_in` / `tokens_out` are recorded for
   future cost-meter work but the Stage 1 UI does not display them.

8. **`AiError` is a new enum, independent of `DbError`.**
   Variants: `Configuration` (missing key, malformed config),
   `Network` (HTTP timeout, TLS failure), `Provider` (rate limit,
   model unavailable, content filter), `Quota` (caller-imposed
   budget exceeded — wired for Stage 2 but the enum slot exists
   now), `Cancelled` (user cancelled an in-flight request).
   AI errors never travel over the desktop ↔ web HTTP contract, so
   ADR-0009's English-category-prefix translation rule does not
   apply; `dbboard-ui` translates `AiError` variants directly to
   Fluent keys (the `t!()`-on-an-enum pattern from ADR-0015).

9. **Stage 2 deferrals, recorded explicitly so the Stage 1 review
   does not relitigate them.** Streaming (`AiProvider::streaming`
   accessor + chunked `Reply` variants). Token budget meter and
   cancel button. Multi-provider switcher UI. `ai-providers.toml`
   + keychain. Conversation history (Stage 1 is single-shot).
   Recording AI calls in the query history file (ADR-0017). Full
   DDL extraction on `DatabaseAdapter`. Function-calling /
   tool-use provider capability.

### Alternatives considered

- **Single `dbboard-ai` crate with provider implementations gated
  behind cargo features (e.g. `--features anthropic`).** Rejected.
  Provider crates can pull in heavy or licence-incompatible
  dependencies (vendor SDKs, model-specific tokenizers). Folding
  them under one crate with feature flags couples build time and
  dependency surface for users who only want one provider. The
  separate-crate pattern matches what we already did for DB
  adapters (`dbboard-turso` / `dbboard-postgres` / `dbboard-d1`),
  which is the closest precedent.

- **Route AI calls through `dbboard-server` as new HTTP endpoints
  (`POST /ai/explain`, `POST /ai/suggest`).** Rejected for Stage 1.
  See Decision 3. Would force a DTO layer, a new contract section
  in `docs/api-contract.md`, and a coordination obligation with
  `dbboard-web`, all for no measurable benefit on a single-process
  desktop app. If a future use case (e.g. CLI clients, browser
  extension talking to the local server) needs HTTP-mediated AI,
  the trait can be re-wrapped behind the server then; the trait
  shape does not predetermine the wiring.

- **Ship streaming on day one.** Deferred. Streaming adds a
  channel-based partial-response delivery path, mid-flight cancel
  handling, and per-chunk UI rendering — each of those is a real
  design decision worth a separate ADR. Stage 1 ships the
  non-streaming baseline so the trait and the wiring can be
  proven before the more complex shape.

- **Ship two providers (Claude + OpenAI) on day one.** Deferred.
  The trait was designed to make additional providers cheap, but
  the Stage 1 surface needs to be validated against exactly one
  real implementation before locking it. A multi-provider switcher
  UI is itself a Stage 2 concern (Decision 5).

- **Generic `complete(prompt: &str)` method instead of typed
  `explain` / `suggest_sql`.** Rejected. A typed surface lets the
  provider own its system prompt and response shape. A generic
  `complete` would push prompt construction up into the UI layer,
  forcing every provider crate to expose its prompt template as
  public API and making it easy to forget the dialect hint or the
  schema snapshot at the call site. Adding a new command later is
  a trait-extension cost we accept (one new method per command);
  in exchange the call sites stay simple and provider-agnostic.

- **Persist API keys via `dbboard-config`'s `SecretStore` from day
  one.** Deferred. The env-var-first → persisted-store evolution
  path is the one the connection adapters used (env vars first in
  Phase 1, connection store in Phase 2 via ADR-0013). Doing it the
  same way here keeps the Stage 1 surface auditable and ships
  faster; the Stage 2 ADR re-uses the proven `SecretStore`
  abstraction.

### Consequences

- Two new crates land in the workspace: `dbboard-ai` (trait + value
  types + `AiError`, no I/O) and `dbboard-anthropic` (first
  concrete provider, reqwest-based). Workspace `Cargo.toml` grows
  by two `members` entries. `apps/dbboard` gains a new optional
  dependency on both.
- `dbboard-ai` re-exports `dbboard_core::TableInfo` for the
  `SuggestRequest::schema` field. This is the first time a
  workspace crate publicly re-exports a `dbboard-core` type, but
  it keeps `dbboard-ai`'s public API self-contained for
  downstream provider crates.
- `apps/dbboard` env-var resolution gains
  `DBBOARD_ANTHROPIC_API_KEY` (required to construct the provider)
  and `DBBOARD_ANTHROPIC_MODEL` (optional). README documents both.
- `DbboardApp::new` grows an `Option<Arc<dyn AiProvider>>`
  parameter; UI rendering checks `has_ai_provider()` and only
  renders the AI panel when present.
- `dbboard-ui` gains an AI panel (UI-side state machine + two
  command/reply pairs through the existing worker). New Fluent
  keys for the panel labels in all 11 locales (ADR-0015 tier
  stability is maintained).
- HTTP contract is unchanged. `dbboard-web` mirror is not
  needed. ADR-0023 joins the ADR-0013 / ADR-0015 / ADR-0016 /
  ADR-0018 / ADR-0019 / ADR-0020 / ADR-0021 / ADR-0022
  desktop-side-only category. No `0NNN-web-*` brief.
- Roadmap: Phase 4 row is annotated with "trait + first provider
  shape locked in ADR-0023". Phase 4 bullet checkmarks land as
  the implementation issue 0005 progresses.
- Implementation tracking: `.claude/issues/0005-dbboard-ai-trait-
  and-anthropic-provider.md` opens against this ADR.
- SemVer impact (ADR-0011): additive. Two new crates, two new env
  vars, one new optional UI panel. No existing public API
  changes signature.

## ADR-0024 — At-rest file permissions for `connections.toml` and `history.jsonl`

**Status:** Accepted (2026-06-22). Locks down the per-user config
files dbboard creates against the *"laptop is lost or stolen"* threat
model. Unix gets explicit `0o600` on creation; Windows relies on the
inherited DACL of `%APPDATA%\Roaming\<user>\` (already user-only by
default on every supported Windows version); a startup-time warning
fires when the config dir resolves to a likely cloud-synced path
(OneDrive Known Folder Move, iCloud Drive). The workspace-wide
`unsafe_code = "forbid"` lint is upheld.

### Context

Phase 4 Stage 1 (ADR-0023) wired the first AI provider through
`apps/dbboard`. As part of preparing the next slice (the AI panel),
we ran a focused security audit scoped to **secrets at rest** and
**secrets in memory / leakage paths** under the threat model of
*"the laptop was lost; the disk is the attack surface."* The
in-memory pass came back clean — the `BackendConfig`, `AiProvider`,
and `EnvSnapshot` types all redact secrets in their `Debug` impls;
`reqwest::Error::without_url()` is applied at every HTTP failure
site; `eprintln!` paths surface no secrets; the OS keychain
(Windows Credential Manager / DPAPI, macOS Keychain, Linux Secret
Service) is the only at-rest secret store and remains scoped to a
logged-in session.

The at-rest pass found two real exposures on **Unix**:

1. `crates/dbboard-ui/src/history.rs:486` opens `history.jsonl`
   with `OpenOptions::new().append(true).create(true)`, no
   explicit mode. The first time the file is created its
   permissions are `0o666 & !umask`. The default umask on most
   Linux distributions (`0o022`) and on macOS (`0o022`) leaves
   the file group- and world-readable. SQL queries logged through
   ADR-0017 may contain literal credentials
   (`UPDATE users SET password = '…'`), so this is not just
   metadata — the file can contain real secrets.

2. `crates/dbboard-config/src/store.rs:256-264` covers the same
   gap for the `connections.toml.tmp` sibling on
   `#[cfg(not(unix))]`. The Unix branch already sets
   `mode(0o600)` (correct since ADR-0013); the Windows branch was
   flagged as a parallel concern.

On **Windows**, the practical exposure is much smaller than the
audit's initial framing suggested:

- `%APPDATA%\Roaming\<user>\` is part of the user's profile.
  Its DACL grants `SYSTEM Full`, `Administrators Full`,
  `<user> Full`, and **denies inheritance to other limited-priv
  accounts**. Files created under it inherit that DACL.
- Our config dir resolves via `directories::ProjectDirs` to
  `%APPDATA%\Roaming\dbboard\dbboard\config\`. Every file we
  create there inherits the restrictive ACL by default.
- The "lost laptop, single-user attacker" branch of the threat
  model is therefore mitigated by NTFS inheritance + (when the
  user enables it) BitLocker. The "multi-user shared machine"
  branch is outside the threat model the user asked us to harden
  against.

The audit also surfaced a third concern that is **not** a code bug
but a configuration risk: OneDrive's *Known Folder Move* feature
silently relocates `%APPDATA%\Roaming\` (or `Documents\`) under
`%OneDrive%\`, which then syncs the directory contents to the
Microsoft cloud. A `history.jsonl` containing literal credentials
would propagate to the user's OneDrive replica. This is documented
behaviour of OneDrive, not a dbboard bug, but we can detect it at
startup and warn the user.

Finally, the workspace declares `unsafe_code = "forbid"` in
`Cargo.toml:87`. The cleanest Win32 path for an *explicit*
user-only DACL on each file would be `windows-sys` →
`SetNamedSecurityInfoW`, which requires `unsafe`. The available
no-unsafe alternatives all have material drawbacks:

- `windows-acl` (trailofbits) — last release 2020, abandoned;
  conflicts with `CLAUDE.md`'s "avoid abandoned crates" rule.
- Shell out to `icacls.exe` — works but adds process-spawn cost,
  locale-dependent error parsing, and a runtime dependency on a
  Windows binary path.
- `cap-std` — large dep tree for what would be a single helper.

Given the modest Windows exposure (inherited ACL is already
restrictive) and the cost of every workaround, this ADR upholds
`unsafe_code = "forbid"` and accepts inherited-DACL behaviour on
Windows. If a future threat model (e.g. enterprise multi-user
workstations) demands explicit per-file DACLs, a follow-up ADR
will reopen this decision.

### Decision

1. **New `crates/dbboard-config/src/secure_fs.rs` helper module.**
   Two functions plus a path-classifier:
   - `pub fn create_new_user_only(path: &Path) -> io::Result<File>`
     — `OpenOptions::create_new(true)` everywhere, plus `mode(0o600)`
     under `#[cfg(unix)]`. Replaces both Unix and non-Unix branches
     of `write_new_file`.
   - `pub fn open_append_user_only(path: &Path) -> io::Result<File>`
     — opens append, creating the file if absent. On first
     creation under `#[cfg(unix)]`, a *single* open with the
     combined flags `O_CREAT | O_EXCL | O_APPEND | mode(0o600)`
     returns the handle the file was created with — no
     close-and-reopen window in which a hostile process could
     substitute a symlink. On subsequent opens, calls
     `set_permissions(0o600)` defensively in case the file
     pre-dates this ADR, then opens append. The tightening
     branch retains a narrow `chmod`-then-`open` TOCTOU, accepted
     under this ADR's lost-laptop threat model (which does not
     assume a hostile *active* local attacker). On Windows, no
     ACL manipulation — relies on inheritance.
   - `pub fn is_likely_cloud_synced_path(path: &Path) -> Option<&'static str>`
     — pure string matcher. Returns the cloud provider name
     (`"OneDrive"`, `"iCloud Drive"`, `"Dropbox"`, `"Google Drive"`)
     when the path traverses a directory segment matching a known
     vendor folder. The Google Drive arm recognises the legacy
     `Google Drive` / `GoogleDrive` mount names plus the modern
     `My Drive` root and the macOS `CloudStorage` / `GoogleDrive-*`
     layout introduced by Google Drive for Desktop. No I/O, no
     platform-specific calls. Returns `None` for everything else,
     and silently skips non-UTF-8 path segments (heuristic, not a
     guarantee — NTFS junctions hiding a cloud-sync vendor name
     will produce false negatives).

2. **`crates/dbboard-config/src/store.rs::write_new_file` is
   replaced by `secure_fs::create_new_user_only`.** The Unix
   branch's behaviour (mode 0o600, `create_new`, `sync_all`) is
   preserved exactly. The non-Unix branch picks up the same
   `create_new` semantics — no behavioural change on Windows
   beyond inheriting `sync_all`. The dedicated module makes the
   policy easy to grep for and easy to share with `dbboard-ui`.

3. **`crates/dbboard-ui/src/history.rs::append_record` switches to
   `secure_fs::open_append_user_only`.** First-creation case now
   lands as `0o600` on Unix instead of umask-dependent. Existing
   `history.jsonl` files surviving an upgrade get tightened on
   the next append via the defensive `set_permissions` path.

4. **Startup OneDrive / cloud-sync warning in
   `apps/dbboard/src/main.rs`.** Right after resolving the config
   dir via `default_path()` / `default_history_path()`, the binary
   calls `is_likely_cloud_synced_path` and, on a hit, emits a
   single `eprintln!` warning to stderr naming the provider and
   recommending the user disable Known Folder Move for the dbboard
   config dir. The warning fires at most once per process. No
   panic, no exit — dbboard still runs (the user might genuinely
   want this).

5. **README and `docs/connections.md` document the at-rest
   posture.** A short section explains the threat model, the
   `0o600` policy, the recommendation to enable BitLocker /
   FileVault / dm-crypt (the practical mitigation that Windows
   inherited ACL alone does not provide on a stolen unencrypted
   disk), and the OneDrive caveat with vendor links for disabling
   the relevant cloud-sync feature.

6. **`unsafe_code = "forbid"` is upheld at the workspace level.**
   No new `unsafe` blocks. No `unsafe`-bearing crates added. If a
   future ADR opens explicit Windows DACL manipulation, it must
   gate the unsafety inside one module with an in-module
   `#![allow(unsafe_code)]` and justify the carve-out per
   `CLAUDE.md`'s decision-log requirement.

### Alternatives considered

- **Explicit `SetNamedSecurityInfoW` DACL on every file via
  `windows-sys`.** Rejected. Forces `unsafe`, conflicting with
  the workspace lint. Marginal benefit over inherited ACL on a
  default Windows install; meaningful benefit only on multi-user
  shared workstations, which are outside the stated threat
  model. Re-openable as a follow-up ADR if that threat model
  changes.

- **Shell out to `icacls.exe`.** Rejected. Runtime dependency on
  a Windows binary path, locale-dependent stderr parsing, and a
  process spawn per file create. The benefit (one extra layer
  over inherited ACL) does not justify the operational surface.

- **Move the config dir to `%LOCALAPPDATA%\dbboard\` to escape
  OneDrive Known Folder Move.** Rejected for now.
  `directories::ProjectDirs::config_dir()` returns the per-user
  roaming dir on Windows by design; switching to local-only
  would diverge from the `directories` crate's convention and
  break upgrades for existing users (their `connections.toml`
  would be invisible). A startup warning is cheaper and gives
  the user an informed choice.

- **Encrypt `history.jsonl` at rest with a per-machine key.**
  Rejected. The OS keychain is the right tool for "encrypt small
  secrets at rest" — see ADR-0013's `KeyringStore`. Encrypting a
  log file with rotating-content semantics adds a key-management
  problem (DPAPI on Windows is the cleanest answer, but it again
  requires `unsafe` via `windows-sys::Security::Cryptography`).
  The simpler answer for a log file is "don't let other users
  read it" + "encrypt the whole disk" — both of which this ADR
  delivers via `0o600` + the BitLocker recommendation.

- **Sanitise SQL text in `history.jsonl` to strip likely
  literals.** Rejected as scope creep. The user explicitly
  excluded "history.jsonl content filtering" when scoping this
  audit. The right shape would be a separate ADR with its own
  redaction policy (regex against `password\s*=\s*'…'`,
  `IDENTIFIED BY '…'`, etc.) and a test corpus. Out of scope
  here.

### Consequences

- One new module: `crates/dbboard-config/src/secure_fs.rs` with
  three public functions and tests. No new dependencies.
- `crates/dbboard-config/src/store.rs::write_new_file` is
  replaced by a one-line delegation to `secure_fs`. The two
  cfg-gated branches collapse.
- `crates/dbboard-ui/src/history.rs::append_record` switches to
  `secure_fs::open_append_user_only`. Behaviour change on Unix:
  newly created `history.jsonl` lands as `0o600` (was
  umask-dependent). Existing files get tightened on next write.
- `apps/dbboard/src/main.rs` gains one `eprintln!` warning path
  guarded by `is_likely_cloud_synced_path`. No new env vars.
- README and `docs/connections.md` grow an "At-rest data" /
  "File permissions" section pointing at this ADR.
- `Cargo.toml` workspace `unsafe_code = "forbid"` stays. No
  `#![allow(unsafe_code)]` overrides land.
- HTTP contract unchanged. No `dbboard-web` mirror needed
  (file-permission policy is a desktop-only concern; the web
  sibling is server-side and uses a different storage model).
- SemVer impact (ADR-0011): non-breaking. The public API of
  `dbboard-config` gains a `secure_fs` module (additive). The
  on-disk file permissions get tighter (also additive — users
  who could read the file before still can; users who shouldn't
  no longer can).
- Implementation tracking: this ADR is implemented in-branch
  (`feat/secure-fs-permissions`); no `.claude/issues/` entry,
  since the work is small enough to land in one PR.
- Roadmap: no row change. This is a security hardening pass on
  Phase 2 / Phase 3 artefacts, not a Phase 4 advancement.

## ADR-0025 — Phase 4 Stage 2 Group A: `ai-providers.toml` + Settings UI + runtime provider switcher

**Status:** Accepted (2026-06-24). **Implementation closed
2026-06-29.** Shipped across four PRs over five days: slice a-1
(PR #37, `dbboard-config` layer) on 2026-06-25, slice a-2-α (PR #39,
`dbboard-ui` worker plumbing) on 2026-06-25, slice a-2-β (PR #41,
`apps/dbboard` real `DesktopAiSwitcher` + env > TOML > None
precedence) on 2026-06-26, and slice (b) (`feature/ai-settings-ui`,
this PR) on 2026-06-29 — `AiSettingsView` egui state machine
(List/Add/Edit/ConfirmDelete) + 13 unit tests, 19 `ai-settings-*`
Fluent keys + `ai-active-with-name` across all 11 locales, AI panel
"Active: { $name }" subtitle, and the `apps/dbboard` menu wiring.
The deferred Stage 2 items (streaming, cancel button, AI calls in
`history.jsonl`, conversation history, full DDL extraction,
function-calling) remain deferred per ADR-0023 §9.

Opens Phase 4 Stage 2 by lifting
the AI provider out of the env-var-only construction path
established in ADR-0023 Decision 5 into a versioned per-user TOML
file (`ai-providers.toml`) keyed by opaque keychain references,
adds an in-app Settings UI for managing providers (mirroring the
ADR-0016 connections window), and adds a runtime provider switcher
that swaps the active `Arc<dyn AiProvider>` in-process without
restarting the desktop binary (mirroring ADR-0020's `swap_backend`
for adapters and ADR-0022's `set_language` for locales). Streaming,
cancel button, AI calls in `history.jsonl`, conversation history,
full DDL extraction, and function-calling stay deferred per
ADR-0023 §9.

### Context

Phase 4 Stage 1 (ADR-0023, PRs #18 / #20 / #22 / #24 / #27) shipped
the `dbboard-ai` trait crate, the `dbboard-anthropic` first
concrete provider, env-var-only wiring in `apps/dbboard`, and an
AI panel in `dbboard-ui`. Decision 5 explicitly previewed Stage 2:

> Stage 1 configuration is env-var-only:
> `DBBOARD_ANTHROPIC_API_KEY` (required) and
> `DBBOARD_ANTHROPIC_MODEL` (optional override). [...] **Stage 2
> will add `ai-providers.toml` + `SecretStore` integration (ADR-0013
> connections.toml is the template) plus a Settings UI for picking
> a provider and managing keys.** Mirroring the `DBBOARD_TURSO_PATH`
> → `connections.toml` evolution path — env-var-only first, then
> persisted store.

ADR-0023 §9 also reserved the multi-provider switcher UI as a
Stage 2 concern. Group A of the Stage 2 slate (per the four-group
split agreed in this session's planning) bundles three deferrals
together because they are co-dependent: a Settings UI is not useful
without a persistent store to mutate, the store is not useful
without a switcher to make a saved provider active, and the
switcher is not useful without a UI to drive it. Bundling them in
one ADR keeps the design coherent; bundling them in one PR is a
slicing question left to issue 0008.

Streaming, cancel button, AI calls in `history.jsonl`, conversation
history, full DDL extraction, function-calling, and token budget
meter — the other Stage 2 deferrals from ADR-0023 §9 — are **not**
in this ADR's scope. They group into separate ADRs (Group B
streaming + cancel, Group C history + v:2 schema bump, Group D
capability expansion) which can land in any order after this one.

The infrastructure to reuse already exists:

- **`dbboard-config::store`** (ADR-0013) — TOML schema versioning
  pattern (`version = 1`, hard error on unknown version),
  `default_path()` / `default_history_path()` for the per-user
  config dir, `load_or_empty()` / `save_atomic()` for atomic
  read-modify-write.
- **`dbboard-config::secrets`** (ADR-0013) — `SecretStore` trait,
  `KeyringStore` / `InMemorySecretStore`, `KEYRING_SERVICE = "dbboard"`,
  opaque `keyring_*_ref` strings stored in TOML.
- **`dbboard-config::secure_fs`** (ADR-0024) — `create_new_user_only`
  for `0o600` on Unix / inherited DACL on Windows. The same
  hardening applies unchanged to `ai-providers.toml`.
- **`dbboard-config::ConnectionAdmin`** (ADR-0016) — the use-case
  shape for add / edit / delete / list with secret references
  routed through a `SecretStore`. `AiSettingsAdmin` mirrors this
  exactly.
- **`dbboard-server::swap_backend`** (ADR-0020) — the in-process
  atomic swap pattern. AI provider switching reuses this shape
  inside `apps/dbboard` (no server-side swap because Decision 3 of
  ADR-0023 keeps AI off the HTTP contract).
- **`dbboard-i18n::set_language`** (ADR-0022) — the runtime-switcher
  precedent. AI provider switching is the third "in-process
  mutate-while-running" surface after backend and locale.

The HTTP contract (`docs/api-contract.md`) and the per-record
history JSON schema (ADR-0017) are both **unchanged** by this ADR.
The desktop ↔ web coordination posture established by
`.claude/issues/0007-web-ai-phase6-no-contract-mirror.md` (2026-06-23,
PR #33) holds: web's Phase 6 ships independently with its own
NestJS-side persistence; this ADR adds nothing for web to mirror.

### Decision

1. **New TOML file `ai-providers.toml`, sibling to `connections.toml`
   and `history.jsonl` under the per-user config dir.** Same
   resolution (`directories::ProjectDirs::from("dev", "dbboard",
   "dbboard").config_dir()`), same at-rest hardening
   (`secure_fs::create_new_user_only` → `0o600` on Unix, inherited
   DACL on Windows). New helper
   `dbboard_config::store::default_ai_providers_path()` symmetric
   with `default_path()` / `default_history_path()`. A missing file
   is **not** an error — `load_or_empty` returns an empty store and
   no file is created until the user adds the first entry via the
   Settings UI.

2. **Schema (`AiProviderFile`).** Versioned (`version = 1`,
   unknown version is a hard error — same posture as
   `ConnectionFile`). Two top-level keys plus a list of entries:

   ```toml
   version = 1
   active_id = "anthropic-sonnet"     # optional; absent => no auto-select

   [[providers]]
   id   = "anthropic-sonnet"
   name = "Anthropic (Sonnet 4.6)"
   kind = "anthropic"
   model = "claude-sonnet-4-6"        # optional override
   keyring_api_key_ref = "dbboard.ai.anthropic-sonnet.api_key"

   [[providers]]
   id   = "anthropic-opus"
   name = "Anthropic (Opus 4.7)"
   kind = "anthropic"
   model = "claude-opus-4-7"
   keyring_api_key_ref = "dbboard.ai.anthropic-opus.api_key"
   ```

   `kind = "anthropic"` is the only Stage 2 variant — additional
   providers (`openai`, `ollama`, …) land as additive variants in
   future ADRs, mirroring `ConnectionKind`'s evolution
   (`Turso` → +`D1` → +`Postgres` → +`Neon` → +`Supabase` →
   +`AuroraDsql`). The `model` field is optional; when absent the
   provider crate's compile-time default applies
   (`claude-sonnet-4-6` for Anthropic). Duplicate `id`, unknown
   `kind`, and unknown `version` are hard parse errors —
   `ConnectionFile`'s posture.

   `active_id` is optional. When present it must reference an
   existing `id` (validated at parse time — dangling `active_id`
   is a hard error). When absent, the app does not auto-construct
   a provider from the TOML; the user must either set an env var
   (precedence below) or select a provider through the Settings
   UI (which writes `active_id`).

3. **Resolution order in `apps/dbboard::resolve_ai_provider`,
   in precedence.** Mirrors the connection resolution chain
   established by ADR-0013:

   1. `DBBOARD_ANTHROPIC_API_KEY` (existing Stage 1 env var) —
      when set and non-blank, constructs an ad-hoc Anthropic
      provider using `DBBOARD_ANTHROPIC_MODEL` if set or the
      crate default. **Highest precedence**, preserves Stage 1
      back-compat verbatim — existing CI / scripted users see no
      change.
   2. `ai-providers.toml` `active_id` — when the env var is unset
      and the TOML has a non-null `active_id`, the named entry is
      resolved through `SecretStore` (looking up
      `keyring_api_key_ref`) and constructed into the matching
      concrete provider. The `model` field overrides the crate
      default for that entry.
   3. None — neither env var nor active TOML entry. The AI panel
      stays hidden (graceful degradation = absence, ADR-0023
      Decision 6 unchanged).

   No silent fallback between providers. A configured-but-broken
   `active_id` (missing keychain entry, malformed model, etc.)
   logs to stderr and degrades to `None` — same posture as
   Stage 1's "construction failure → log + None" path in
   `resolve_ai_provider`.

4. **`AiSettingsAdmin` use-case in `dbboard-config::ai_settings`.**
   Mirrors `ConnectionAdmin` (ADR-0016) module-for-module:
   - `entries() -> &[AiProviderEntry]` — read-only snapshot.
   - `add(draft: AiProviderDraft) -> Result<&AiProviderEntry,
     AiSettingsError>` — assigns / validates id, writes the API
     key into the `SecretStore` under
     `dbboard.ai.<id>.api_key`, appends the entry, calls
     `save_atomic`.
   - `update(id, edit_draft)` — preserves existing
     `keyring_api_key_ref` unless the draft carries a new key
     (mirrors `ConnectionEditDraft::SecretField` semantics: leave
     unchanged / replace / clear).
   - `delete(id)` — removes the entry, removes the matching
     keychain entry via `SecretStore::delete` (best-effort —
     surface a soft warning if the keychain delete fails but the
     TOML write succeeded; identical to ADR-0016's posture for
     orphaned secrets when a delete is interrupted), clears
     `active_id` if it pointed at this entry.
   - `set_active(id: Option<&str>)` — writes the `active_id` slot
     and calls `save_atomic`. Passing `None` clears it (returns
     to "no auto-select").

   `AiSettingsError` is crate-local (`Parse` / `Io` /
   `UnsupportedVersion` / `DuplicateId` / `UnknownActiveId` /
   `Secret`), independent of `DbError` and `AiError` — these
   errors happen at process startup or in UI handlers, never
   reach the wire.

5. **`AiProviderSwitcher` trait + `DesktopAiSwitcher` impl, mirroring
   ADR-0020's `ConnectionSwitcher` precedent.** The trait lives in
   `dbboard-server` next to `ConnectionSwitcher` (the worker
   already takes one `Arc<dyn ConnectionSwitcher>` from
   `apps/dbboard`; adding `Arc<dyn AiProviderSwitcher>` is a
   symmetric expansion of the same wiring). One method:
   `fn switch(&self, id: &str) -> Result<(), AiError>`. The
   desktop impl resolves the entry through `AiSettingsAdmin`,
   looks up the secret through `SecretStore::get`, constructs the
   concrete provider (Stage 2: only `AnthropicProvider`), and
   atomically swaps an `Arc<RwLock<Option<Arc<dyn AiProvider>>>>`
   held in `DbboardApp`. A `NullAiSwitcher` (returns
   `AiError::Configuration("no ai store available")`) covers the
   headless / no-config-dir fallback, same shape as
   `NullSwitcher`.

   `DbboardApp` upgrades from `Option<Arc<dyn AiProvider>>` to
   `Arc<RwLock<Option<Arc<dyn AiProvider>>>>` — a single new
   indirection layer. The worker snapshots the current provider
   once per request (same "snapshot at request start" rule as
   ADR-0020 for `AppState`'s adapter slot), so an in-flight
   `Command::AiExplain` completes against the provider it started
   with even if the switcher fires mid-call. `has_ai_provider()`
   becomes `read().is_some()`.

6. **UI: new `AiSettingsView` in `dbboard-ui`, mirroring
   `ConnectionsView` (ADR-0016).** Opens via a new menu entry
   "AI > Settings" (Fluent key `ai-settings-window-title`,
   localised across all 11 locales — ADR-0015 tier stability
   maintained). Lists entries with id / name / kind / model /
   active marker, with inline add / edit / delete forms. The
   active provider is set by clicking a per-row "Use" button —
   the same shape as the connections window's per-row "Connect"
   button (ADR-0020). `AiSettingsView::take_pending_switch()`
   mirrors `ConnectionsView::take_pending_connect()` and routes
   into the worker as `Command::SwitchAiProvider { id }` →
   `Reply::AiProviderSwitched { id }` / `Reply::AiProviderSwitchFailed
   { reason }`. The AI panel's existing dropdown (currently a
   single-provider stub) reflects the active id.

7. **Keychain naming convention.** Following the
   `dbboard.<connection-id>.token` pattern from ADR-0013, AI keys
   land under `dbboard.ai.<provider-id>.api_key`. Service string
   stays `KEYRING_SERVICE = "dbboard"` so a single OS-keychain
   wipe still clears everything dbboard owns. The `ai.` infix
   keeps connection secrets and AI secrets distinguishable in the
   OS UI without needing a separate service string.

8. **Per-provider `model` override semantics.** The TOML's `model`
   field (optional, per entry) is the second-highest precedence
   after `DBBOARD_ANTHROPIC_MODEL`. Combined with Decision 3:
   when `DBBOARD_ANTHROPIC_API_KEY` is the active path, the model
   resolves as env var → crate default (existing Stage 1
   behaviour, unchanged). When the TOML path is active, the model
   resolves as `entry.model` → crate default. This keeps the env
   var path entirely independent of the TOML — explicit override
   stays explicit. **`DBBOARD_ANTHROPIC_MODEL` does not bleed into
   the TOML path** because it would couple two configuration
   channels users would reasonably expect to be orthogonal.

9. **Stage 2 deferrals re-confirmed (out of scope for this ADR,
   queued for separate ADRs).** Streaming
   (`AiProvider::streaming` accessor + chunked `Reply` variants).
   Cancel button + in-flight token budget meter. Multi-provider
   `kind` variants other than `anthropic` — the schema permits
   them but no concrete impl ships in this ADR's slice; a
   follow-up ADR per provider (`dbboard-openai`,
   `dbboard-ollama`, …) lands the matching `kind` variant
   additively. Conversation history (single-turn stays the Stage
   1 / Stage 2 surface). AI calls in `history.jsonl` (still
   blocked behind a v:2 schema bump — coordinates with web per
   `0007-web-ai-phase6-no-contract-mirror`'s explicit guard).
   Full DDL extraction (still needs a new
   `DatabaseAdapter::dump_schema` method). Function-calling /
   tool-use provider capability.

10. **Cross-repo posture: no `0NNN-web-*` brief.** This ADR is
    desktop-only — no contract change, no history schema change.
    The desktop-side AI persistence file (`ai-providers.toml`) is
    not part of any shared surface, and web's Phase 6
    (NestJS-side) ships independently per
    `0007-web-ai-phase6-no-contract-mirror`. Joins ADR-0013 /
    ADR-0015 / ADR-0016 / ADR-0018 / ADR-0019 / ADR-0020 /
    ADR-0021 / ADR-0022 / ADR-0023 / ADR-0024 in the desktop-only
    category.

### Alternatives considered

- **Store AI providers inside `connections.toml` as a new
  `[[ai_providers]]` table.** Rejected. ADR-0017 chose a separate
  `history.jsonl` over a `[[history]]` table in `connections.toml`
  for the same reason: mixing two concerns into one file forces
  every read/write to touch both, and a corrupted AI provider
  parse would block connection loading. Separate file with
  separate version field is the precedent.

- **One big `dbboard.toml` with three top-level sections
  (connections, ai_providers, history-config).** Rejected for now —
  see above. A single combined config file is a reasonable future
  refactor *if* the three files start needing cross-cutting
  invariants (which they do not today), but the cost of splitting
  it later is small enough that we should not pre-pay it.

- **Skip the file entirely; persist via the OS keychain only.**
  Rejected. The keychain holds the *secret*; it does not hold
  the *metadata* (name, kind, model, the user's list of
  configured providers). Trying to encode all that into keychain
  account strings would re-create the worst parts of registry
  programming and would not survive a keychain wipe (the user
  loses the metadata along with the secrets, instead of being
  able to re-paste a key into a still-visible row).

- **Hold the active provider as an env var (`DBBOARD_AI_ACTIVE_ID`)
  instead of a TOML field.** Rejected. Env vars are session-scoped
  (typically per-shell); a Settings UI choice that needed the
  user to also export an env var to make it stick across reboots
  is bad UX. The TOML `active_id` is the natural home — same
  shape as `DBBOARD_CONNECTION`'s relationship to the
  auto-select-single-entry path (ADR-0013).

- **Mutate `apps/dbboard`'s `Option<Arc<dyn AiProvider>>` directly
  without the `Arc<RwLock<...>>` wrapper, by recreating the
  `DbboardApp` whenever the user switches.** Rejected. Recreation
  would lose the existing AI panel state (drafted prompt, scroll
  position, in-flight response), and the worker channel would
  need to be torn down and rebuilt. The lock-wrapped slot is one
  layer of indirection and matches ADR-0020's `AppState` adapter
  swap exactly — proven pattern, no new shape.

- **Allow `DBBOARD_ANTHROPIC_MODEL` to override the TOML's
  `model` field.** Rejected (see Decision 8). Coupling the two
  channels would make it impossible for a user to test "what
  does the TOML entry actually do" without unsetting the env
  var. Orthogonal channels keep the precedence table predictable.

- **Ship a second concrete provider (`dbboard-openai`,
  `dbboard-ollama`, …) in this ADR's slice to validate the
  multi-provider surface end-to-end.** Deferred to a follow-up
  ADR per provider. The TOML schema and switcher infrastructure
  are multi-provider-ready (multiple entries with `kind =
  "anthropic"` already exercise the active-id selection and
  switcher round-trip); a second `kind` value is purely additive
  and slots in without re-litigating any of the Stage 2 Group A
  decisions. Same posture as ADR-0023 Decision 1: validate the
  trait against one real implementation before locking the next
  shape.

- **Encrypt the API key in the TOML directly (passphrase /
  hardware key) instead of routing it through the OS keychain.**
  Rejected. The OS keychain is the right tool — see ADR-0013's
  rejection of self-rolled secret encryption. Reusing the
  existing `SecretStore` abstraction is the cheapest, safest
  path and stays consistent with how connection secrets land.

### Consequences

- Workspace gains one new file (`ai-providers.toml`) and one new
  module (`crates/dbboard-config/src/ai_settings.rs`). No new
  crates. No new external dependencies — `dbboard-config`
  already pulls in `toml` / `serde` / `directories` / `keyring`
  via ADR-0013.
- `dbboard-config`'s public API gains: `default_ai_providers_path`,
  `AiProviderFile`, `AiProviderEntry`, `AiProviderKind`,
  `AiProviderDraft`, `AiProviderEditDraft`, `AiSettingsAdmin`,
  `AiSettingsError`. Re-exported from `lib.rs` next to the
  ADR-0013 / ADR-0016 surfaces. The TOML schema is itself
  versioned (`version = 1`) so future evolution is explicit.
- `dbboard-server` gains an `AiProviderSwitcher` trait (~10 LOC,
  one method) next to `ConnectionSwitcher`. The worker grows a
  second switcher slot. Worker `Command` enum gains
  `SwitchAiProvider { id }`; `Reply` gains
  `AiProviderSwitched { id }` and `AiProviderSwitchFailed
  { reason }`. The HTTP contract is **unchanged** — these are
  in-process channel additions, not wire surface.
- `apps/dbboard` gains: `DesktopAiSwitcher` (concrete impl),
  `NullAiSwitcher` (headless fallback), `ai_provider_for_entry`
  (the AI-provider analogue of `backend_config_for_entry`).
  `DbboardApp::connect` takes
  `Arc<RwLock<Option<Arc<dyn AiProvider>>>>` instead of
  `Option<Arc<dyn AiProvider>>`. `resolve_ai_provider` keeps the
  env-var path verbatim and adds the TOML-active-id fallback as
  step 2 of the precedence chain.
- `dbboard-ui` gains `AiSettingsView` (egui surface),
  `take_pending_switch()`, Fluent keys for the panel labels in
  all 11 locales (the per-locale add cost is ~6 strings —
  `ai-settings-window-title` / `ai-settings-add` /
  `ai-settings-edit` / `ai-settings-delete` / `ai-settings-use` /
  `ai-settings-active-marker`). ADR-0015 tier stability is
  maintained.
- README and `docs/connections.md` (or a new `docs/ai.md` —
  slicing decided in the implementation issue) document the
  precedence chain, the keychain naming, and the migration path
  from Stage 1 env-vars-only to Stage 2 TOML-backed.
- HTTP contract unchanged. Per-record history JSON schema
  unchanged. `dbboard-web` mirror not needed (this is the
  follow-up to `0007-web-ai-phase6-no-contract-mirror` — the
  no-mirror posture established there still holds; web's
  Phase 6 implementation is independent of how desktop persists
  its providers).
- Roadmap: Phase 4 row's currently open box "Settings UI for
  API key, provider choice" — annotated with the ADR-0025
  reference and the issue 0008 link, ticks off when
  implementation lands.
- Implementation tracking: `.claude/issues/0008-ai-provider-settings-ui-and-persistence.md`
  opens against this ADR. Slicing within issue 0008 is left to
  the implementer — natural slices are (a) TOML schema +
  `AiSettingsAdmin` + tests, (b) `AiProviderSwitcher` +
  `DesktopAiSwitcher` + worker plumbing, (c) `AiSettingsView`
  egui surface + Fluent keys + 11-locale translations, (d) README
  + docs sweep. The Stage 1 implementation issue 0005 split into
  two slices (a/b) across PRs #20/22/24 + #27; issue 0008 may
  split similarly or land smaller — the ADR does not prescribe.
- SemVer impact (ADR-0011): additive. New public types in
  `dbboard-config`. New trait in `dbboard-server` (additive
  worker channel variants — existing `Command` / `Reply`
  consumers ignore unknown variants under the `serde` derive,
  but for the in-process channel the variants are exhaustively
  matched, so the worker code change is the additive surface,
  not the serialization). `DbboardApp::connect` signature
  changes — caught at compile time, the only caller is
  `apps/dbboard::main`. No HTTP contract changes. No
  `dbboard-core` changes.

## ADR-0026 — Phase 4 Stage 2 Group B: AI streaming, cooperative cancel, and token meter

**Status:** Accepted (2026-06-30). Implementation tracker:
`.claude/issues/0009-ai-streaming-cancel-tokens.md`. Lands on
`feature/ai-streaming-cancel-tokens` across four commits:

- Slice (a) `2cb012e` — `dbboard-ai` trait extension with
  `stream_explain` / `stream_suggest_sql` returning
  `BoxStream<'static, AiResult<StreamEvent>>`, plus normalized
  `StreamEvent` / `StopReason` enums and the previously-unused
  `AiCapabilities::has_streaming` flag.
- Slice (b) `e5f49d0` — Anthropic SSE wired through
  `dbboard-anthropic` via `reqwest-eventsource` 0.6 with
  `RetryPolicy::Never` (Decision 4 — token-billed POSTs must not
  silently retry).
- Slice (c) `e8f5fd5` — `dbboard-ui` worker rewired with a tokio
  async loop + std-to-tokio mpsc bridge thread + per-request
  `CancellationToken`. `tokio::select!` races the stream against
  the token; the cancel arm emits `Reply::AiCancelled` directly
  rather than synthesising `AiError::Cancelled` (Decision 12).
- Slice (d) `fff669c` — `AiPanel` state machine extended with
  `StreamingAcc` + `streaming` + `cancelled` fields, real
  `on_stream_chunk` / `on_stream_complete` / `on_cancelled`,
  Send↔Cancel button toggle, token meter, and 3 new Fluent keys
  (`ai-cancel-button`, `ai-cancelled-message`, `ai-tokens-meter`)
  in all 11 locales.

Opens Phase 4 Stage 2 Group B by extending the `dbboard-ai`
`AiProvider` trait with **additive** streaming methods, wiring SSE
streaming through `dbboard-anthropic` against Anthropic's
`/v1/messages?stream=true` endpoint, adding a cooperative cancel
path through the `dbboard-ui` worker channel, and surfacing a token
meter sublabel in `AiPanel`. The HTTP contract and per-record
history JSON schema are both **unchanged** by this ADR. Group C
(`history.jsonl` AI records + v:2 schema bump, the one Stage 2
deferral that needs a web brief) and Group D (full DDL extraction +
function-calling) remain deferred per ADR-0023 §9 and can land in
any order after this one.

### Context

Phase 4 Stage 1 (ADR-0023) shipped the `AiProvider` trait with two
methods that return atomic `AiResult<AiResponse>`. Stage 2 Group A
(ADR-0025) shipped runtime provider switching, a per-user TOML
file, and a Settings UI — but kept every AI call atomic.

Three observed friction sources motivate Group B:

1. **No incremental feedback during long generations.** A Claude
   Sonnet 4.6 explanation of a non-trivial SQL statement can take
   8–30 seconds end-to-end. The Stage 1 UI shows a spinner with no
   intermediate output, so the user cannot tell whether the
   request is making progress, has stalled, or has produced a
   wrong direction worth aborting.
2. **No way to abort an in-flight request.** Stage 1 has no cancel
   button. A user who realises mid-generation that the prompt was
   wrong, or that the response is heading in a useless direction,
   has no way to reclaim the tokens that have not yet been
   generated. The only option is to wait for completion (token
   spend already committed) or to close the AI panel (the request
   continues, the response is discarded).
3. **No visibility into token spend.** `AiResponse` already carries
   `tokens_in` / `tokens_out` (Stage 1, ADR-0023), but the
   `AiPanel` does not render them. Without visible cost per
   request, the user cannot calibrate how aggressively to use the
   AI features.

The audit of the existing AI surface (the slice-b PR #43 baseline)
found three pieces of infrastructure that were **already reserved**
in Stage 1 but unused:

- `AiCapabilities::has_streaming` — boolean flag, declared
  Stage 1, set to `false` by every provider so far.
- `AiError::Cancelled` — variant declared Stage 1 with no payload,
  no production code path emits it.
- `AiResponse.tokens_in` / `tokens_out` — `u32` fields populated
  by `dbboard-anthropic` since PR #22 but never read by the UI.

This ADR activates all three rather than introducing parallel
machinery.

### Research summary

The Anthropic Messages API streams via Server-Sent Events when
called with `"stream": true`. The wire format is RFC SSE
(`event: <type>\ndata: <json>\n\n`). Required headers are
unchanged from the non-streaming path (`x-api-key`,
`anthropic-version: 2023-06-01`, `content-type: application/json`).

Event sequence (per Anthropic's streaming reference):

```
message_start                        // initial Message stub + usage.input_tokens
( content_block_start
  ( content_block_delta )+           // delta.type = text_delta (also: input_json_delta,
  content_block_stop )+              //              thinking_delta, signature_delta)
( message_delta )+                   // delta.stop_reason, cumulative usage.output_tokens
message_stop
```

Two cross-cutting concerns: `ping` events can appear at any
point (must be tolerated, never surfaced), and `error` events
(`overloaded_error`, etc.) can interrupt mid-stream and must map
to `AiError::Provider`. **Critical:** the `usage.output_tokens`
field in `message_delta` is **cumulative**, not incremental — the
token meter reads the *last* observed value rather than summing
deltas.

The Rust SSE crate landscape converged on **`reqwest-eventsource`**
(builds on `eventsource-stream`, adds a `RequestBuilder.eventsource()`
extension method and an explicit `.close()`). Production Rust
Anthropic clients — `bosun-ai/async-anthropic`, `spiceai/spiceai`,
`zed-industries/zed`, `microsoft/prompty`, `Kuberwastaken/claurst` —
all return `Pin<Box<dyn Stream<Item = Result<Event, E>> + Send>>`
(equivalent to `futures::stream::BoxStream<'static, _>`) and all
cancel by dropping the stream (reqwest closes the underlying h2
connection on drop, no `unsafe` and no `tokio_util::CancellationToken`
coupling in the trait).

### Decision

1. **Additive trait extension.** Add two methods to `AiProvider`
   alongside the existing `explain` / `suggest_sql`. No existing
   method changes shape:

   ```rust
   pub type AiStream =
       futures::stream::BoxStream<'static, AiResult<StreamEvent>>;

   #[async_trait]
   pub trait AiProvider: Send + Sync {
       fn id(&self) -> &'static str;
       fn capabilities(&self) -> AiCapabilities;
       async fn explain(&self, req: &ExplainRequest)
           -> AiResult<AiResponse>;                              // unchanged
       async fn suggest_sql(&self, req: &SuggestRequest)
           -> AiResult<AiResponse>;                              // unchanged
       async fn stream_explain(&self, req: &ExplainRequest)
           -> AiResult<AiStream>;                                // new
       async fn stream_suggest_sql(&self, req: &SuggestRequest)
           -> AiResult<AiStream>;                                // new
   }
   ```

   Trait stays object-safe under `Arc<dyn AiProvider>`.
   `#[async_trait]` is kept because dropping it for `impl Future`
   would re-break object-safety, and every production Rust
   Anthropic client surveyed uses the same pattern.

2. **Default implementations delegate to the atomic methods.**
   `stream_explain` and `stream_suggest_sql` ship default bodies
   that call `self.explain(...)` (resp. `self.suggest_sql(...)`)
   and yield the full response as a one-shot
   `TextDelta` + `Usage` + `MessageStop` event sequence. This
   means any provider that does **not** override the streaming
   methods (and any future non-Anthropic provider) still satisfies
   the streaming contract — they just stream a single chunk.
   `AiCapabilities::has_streaming` distinguishes the two: `true`
   means "this provider produces token-granularity chunks", `false`
   means "the default delegate is in effect and chunks arrive
   in one piece".

3. **`StreamEvent` is a normalized enum, not a re-export of the
   Anthropic shape.** The trait surface stays
   provider-independent:

   ```rust
   pub enum StreamEvent {
       MessageStart { tokens_in: u32 },          // input usage snapshot
       TextDelta(String),                        // append to accumulated text
       Usage { tokens_in: u32, tokens_out: u32 },// cumulative; replace meter
       MessageStop { stop_reason: StopReason },  // end-of-stream marker
       Error(AiError),                           // mid-stream interruption
   }

   pub enum StopReason {
       EndTurn,
       MaxTokens,
       StopSequence,
       ToolUse,
       Refusal,
       Other(String),
   }
   ```

   `input_json_delta` / `thinking_delta` / `signature_delta` (the
   non-`text_delta` content-block deltas Anthropic emits for
   tool-use / extended thinking) are **dropped** at the provider
   layer for Group B — the UI does not need to render them and
   surfacing them would lock the contract to Anthropic. Group D
   (function-calling) can revisit.

4. **SSE crate: `reqwest-eventsource` with `RetryPolicy::Never`.**
   New dependency on `crates/dbboard-anthropic/Cargo.toml`. Retry
   is disabled because token-billed POSTs must not silently
   retry — a transparent retry doubles the cost and confuses
   token accounting. A 5xx is surfaced as `StreamEvent::Error`
   exactly once.

5. **Cancel is drop-the-stream, never a trait-level token.** The
   `AiProvider` trait does **not** take a `CancellationToken`
   argument. The `dbboard-ui` worker owns the stream future and a
   per-request `tokio_util::sync::CancellationToken`, and uses
   `tokio::select!` to race the stream against the token. On
   cancel the worker drops the `BoxStream`, which drops the
   `EventSource`, which drops the underlying `reqwest::Response`,
   which closes the h2 connection — propagating server-side
   cancellation. No `unsafe`, no manual abort plumbing in the
   trait. (Decision verified against `bosun-ai/async-anthropic`,
   `zed-industries/zed`, `spiceai/spiceai` — none threads a token
   through the trait.)

6. **Worker channel: additive `Command` / `Reply` variants.**
   Existing `Command::AiExplain` / `AiSuggest` and
   `Reply::AiResponded` / `AiFailed` stay verbatim. New variants:

   ```rust
   enum Command {
       // existing variants unchanged
       AiExplainStream  { sql: String, dialect: Option<String> },
       AiSuggestStream  { prompt: String, dialect: Option<String>,
                          schema: Vec<TableInfo> },
       CancelAiRequest,
   }

   enum Reply {
       // existing variants unchanged
       AiChunk          { text_delta: String,
                          tokens_in:  Option<u32>,
                          tokens_out: Option<u32> },
       AiStreamComplete { tokens_in:  u32,
                          tokens_out: u32,
                          stop_reason: StopReason },
       AiCancelled,
   }
   ```

   `AiChunk.tokens_*` are `Option<u32>` because the typical
   `content_block_delta` event carries no usage data — only
   `message_start` and `message_delta` events do. The UI
   replaces the last-known-good value when `Some`, leaves it
   alone when `None`. `Reply::AiFailed` continues to carry
   pre-stream errors; mid-stream errors arrive as
   `Reply::AiChunk` is interrupted, then a `Reply::AiFailed
   { error: AiError::Provider(...) }` closes the stream.

7. **Token meter reads the cumulative value.** The UI keeps a
   `last_tokens_in: Option<u32>` and `last_tokens_out: Option<u32>`
   pair and **replaces** them on each `AiChunk.tokens_*` that
   arrives, rather than summing deltas. This matches the
   Anthropic `message_delta.usage.output_tokens` semantics
   (cumulative within a single message). On `AiStreamComplete`
   the final values are written to `AiResponse.tokens_in` /
   `tokens_out` for the `last_response` field (so the meter
   stays visible after the stream ends).

8. **`AiCapabilities::has_streaming` is now a contract.** A
   provider that returns `has_streaming = true` MUST override
   `stream_explain` / `stream_suggest_sql` with a real streaming
   implementation. A provider that returns `has_streaming = false`
   gets the default delegate (single chunk). `dbboard-anthropic`
   sets `has_streaming = true`. The UI consults this flag to
   gate the streaming-mode toggle in `AiPanel`.

9. **Streaming is opt-in via a `AiPanel` toggle.** Default behavior
   stays atomic (`Command::AiExplain` / `AiSuggest`) so existing
   tests and user flows are unaffected. A new toggle "Stream
   response" appears in `AiPanel` when
   `provider.capabilities().has_streaming == true`. When checked,
   the panel sends the `*Stream` command variants and renders
   chunks incrementally; when unchecked, behavior is bit-for-bit
   the same as before this ADR.

10. **Cancel button policy.** The cancel button is enabled
    whenever `busy == true`, including in the atomic path (it
    sends `Command::CancelAiRequest`, the worker drops the
    in-flight future — same drop-the-future cancel mechanism).
    In the atomic path the worker emits `Reply::AiCancelled` and
    the panel resets to idle. The intent is "cancel is always
    possible while busy", not "cancel only when streaming".

11. **Mid-flight provider swap behavior is unchanged.** ADR-0025
    Decision 6 (the slot snapshot at dispatch time, in-flight
    requests complete on the old provider, next request uses the
    new) carries over for the stream path. A swap during a
    stream does **not** cancel the stream; the user can press
    Cancel explicitly if desired. This keeps the swap behavior
    predictable and avoids needing a swap → cancel coupling.

12. **`AiError::Cancelled` is the only outcome for user-initiated
    cancellation.** A cancelled request does not transition to
    `AiError::Network` or `AiError::Provider` even though the
    underlying reqwest connection closed. The worker sets the
    error variant based on which arm of the `select!` fired (the
    cancel arm → `Cancelled`; the stream-error arm → preserve the
    provider's error). The UI renders `Cancelled` distinctly from
    `Failed` (no error banner, just "Cancelled.").

### Alternatives considered

- **Change the existing methods to return `AiStream`.** Breaking
  change. Would force every future provider to implement
  streaming or wrap a one-shot in a stream. Additive is cleaner
  and matches ADR-0023's "additive only" SemVer posture for
  `dbboard-ai`.

- **Use `eventsource-stream` directly without
  `reqwest-eventsource`.** Saves one direct dep. Loses the
  `RequestBuilder.eventsource()` ergonomics and the explicit
  `.close()`. The dep weight delta is negligible (both crates
  are tiny) and `reqwest-eventsource` is what every surveyed
  production Rust Anthropic client uses.

- **Hand-roll SSE on `reqwest::Response::bytes_stream()` +
  `LinesCodec`.** zed-industries/zed does this. Saves the
  dependency entirely but reimplements the CRLF / `:`-comments /
  multi-line `data:` parsing that the SSE spec requires. The bug
  surface is real (zed has open issues against their parser) and
  not worth the saving.

- **Thread a `CancellationToken` through the trait.** Couples
  `dbboard-ai` to `tokio_util`. None of the surveyed production
  Rust Anthropic clients do this. Drop-the-stream is the
  idiomatic choice and matches how `reqwest` documents
  cancellation.

- **Sum token deltas instead of reading cumulative values.**
  Would produce incorrect totals because Anthropic explicitly
  documents `message_delta.usage.output_tokens` as cumulative
  within the message. Adding deltas would double-count.

- **Add a `Reply::AiStreamProgress` distinct from `AiChunk`.**
  Two reply variants for the same conceptual event ("the stream
  produced data") complicate the panel's `drain_replies` arm.
  One `AiChunk` variant with optional usage fields is enough.

- **Make streaming the default and atomic the opt-in.** Risk: a
  user who has not noticed the new mode toggle would suddenly
  see incremental rendering on every request, which changes the
  feel of the AI panel for everyone. Opt-in keeps the change
  isolated to users who want it.

### Consequences

- **New crate dependency:** `reqwest-eventsource` (latest stable,
  pinned in `crates/dbboard-anthropic/Cargo.toml`). Workspace
  `cargo deny check` must accept it. License (`MIT OR Apache-2.0`)
  matches the existing policy.

- **`dbboard-ai`:** trait gains two methods, one new `AiStream`
  type alias, one new `StreamEvent` enum, one new `StopReason`
  enum. The crate still has no runtime I/O — `BoxStream` is
  `futures::stream` re-exported, no `tokio` runtime dep added.

- **`dbboard-anthropic`:** new module wiring
  `reqwest-eventsource`, new SSE event parser (small — maps
  Anthropic event types into the normalized `StreamEvent`), new
  wiremock tests for happy-path / mid-stream error / cancel-drop.
  `has_streaming = true` capability flag.

- **`dbboard-ui`:** new `Command` variants, new `Reply` variants,
  new worker dispatch arms using `tokio::select!`, new
  `AiPanel` state (`streaming_enabled: bool`, `accumulated_text:
  String`, `last_tokens_in/out: Option<u32>`, cancel signal
  handle). 3 new Fluent keys (`ai-cancel-button`,
  `ai-stream-toggle`, `ai-tokens-meter`) × 11 locales
  (ADR-0022 same-commit sync).

- **`apps/dbboard`:** no change. The `DbboardApp::connect`
  signature does not gain a new argument — streaming flows
  through the existing `Arc<dyn AiProvider>` because the trait
  carries the new methods.

- **HTTP contract (`docs/api-contract.md`):** unchanged. AI
  streaming is in-process, same posture as ADR-0023 Decision 3.
  No new endpoints, no new error categories, no new DTOs.

- **Per-record history JSON schema:** unchanged. Streaming
  responses are not recorded in `history.jsonl` — Group C
  (deferred) is the ADR that lifts that, and Group C is when
  the v:2 schema bump is debated.

- **Cross-repo coordination:** **none required.** ADR-0023
  Decision 3 keeps AI off the HTTP wire, and
  `.claude/issues/0007-web-ai-phase6-no-contract-mirror.md` (PR
  #33, 2026-06-23) already pre-announced that web's Phase 6 AI
  work ships independently. Group B does not change that posture.
  No new `0NNN-web-*-no-mirror.md` brief is needed.

- **Implementation slicing:** issue 0009 may split into (a)
  `dbboard-ai` trait extension + `StreamEvent` types + default
  delegate impls, (b) `dbboard-anthropic` SSE implementation +
  wiremock tests, (c) `dbboard-ui` worker plumbing + `AiPanel`
  toggle + cancel button + token meter + Fluent keys, (d) docs
  sweep. May land as one PR or four; the ADR does not prescribe.

- **SemVer impact (ADR-0011):** additive. New trait methods
  (with default impls, so existing impls do not break — the
  one existing impl in `dbboard-anthropic` will override).
  New public types in `dbboard-ai`. New worker channel
  variants. No removed surface. No HTTP contract changes. No
  `dbboard-core` changes.

## ADR-0027 — Phase 4 Stage 2 Group C: AI calls recorded in `history.jsonl` (schema v:2)

- **Status:** Accepted (2026-07-01). Implementation tracker:
  [`.claude/issues/0010-ai-history-v2.md`](../.claude/issues/0010-ai-history-v2.md).
  Lands on `feature/ai-history-v2` across four commits:
  - Slice (a) `b16537f` — `dbboard-ui::history` v:2 reader + writer
    (`RecordWire` flattened, `kind: "query" | "ai"` discriminator,
    `HistoryEntry::{Query, Ai}` split, 64 KiB write-side truncation,
    v:1 records read transparently as `kind: "query"`, unknown `kind`
    / `intent` drop + counter tick). `emit_history_fixture` extended
    to emit `kind: "ai"` alongside `kind: "query"`.
  - Slice (b) `13f7736` — `dbboard-ai::AiProvider::identity()` +
    `AiResponse { provider, model }` additive fields +
    `dbboard-anthropic` implementation + `dbboard-ui::worker`
    spawn-time identity snapshot stamped on every terminal reply
    (`Reply::AiResponded` / `AiStreamComplete` / `AiFailed` /
    `AiCancelled` gain `provider, model`).
  - Slice (c) `0e76223` — `dbboard-ui::lib` UI-thread AI history
    write point. `PendingAiSubmit` snapshot at Send, terminal-reply
    dispatch composes `HistoryEntry::Ai { … }` from the pending
    record + reply payload + spawn-time identity + streaming
    accumulator peek (peeked before `AiPanel::on_stream_complete`
    drains it). 18 new unit tests covering all four terminal reply
    arms + helper round-trips.
  - Slice (d) `34ad0eb` — docs sweep (this ADR flipped to Accepted,
    `docs/roadmap.md` Phase 4 Stage 2 Group C ticked, `README.md`
    AI section gains the verbatim-logging warning,
    `.claude/issues/0010` closed, brief 0008 Anchors filled in,
    `.claude/project-status.md` records the slice landing).
    All five commits shipped via PR #47, merged to `develop` at
    `768e009` on 2026-07-01.
- **Cross-repo brief:** [`.claude/issues/0008-web-history-v2-mirror.md`](../.claude/issues/0008-web-history-v2-mirror.md) (issued same PR)
- **Supersedes:** ADR-0017 §1 record shape (the v:1 schema). ADR-0017's §3
  storage / §4 rotation / §6 forward-compat / §7 secret-handling stances
  carry over unchanged.
- **Activates:** ADR-0023 §9 deferred "AI calls in history" + ADR-0026
  Out-of-scope item (Group C).

### Context

Three observations after Group A (ADR-0025 provider config) and Group B
(ADR-0026 streaming + cancel + token meter) landed:

1. **No durable record of AI activity exists.** A user can run an
   `explain` against a 200-line SQL block, get a 30-second streamed
   response, and the moment they navigate away the response is gone.
   Token spend was real; the artefact is not.
2. **The existing history surface is exactly the right place to put
   AI activity.** `history.jsonl` is already the project's canonical
   "what happened in this session" record. It already round-trips
   through `jq`. It already has ADR-0024 at-rest hardening. It already
   has rotation, forward-compat, and a cross-repo mirror contract
   (ADR-0017 §1 + brief 0003). Building a parallel `ai-history.jsonl`
   would duplicate all of that and split the user's mental model.
3. **The Group C surface forces a schema bump.** AI records do not
   have `sql`, `rows`, or `rows_affected`. A v:1 reader that
   encountered one would either reject it outright or interpret it as
   a query with an empty SQL string. Adding new top-level fields
   without a discriminator silently breaks the existing schema's
   semantic invariants. The v:1 → v:2 jump is the cheapest forward-
   compatible move because ADR-0017's reader already drops records
   with an unknown `v` (`history.rs:255`) and counts the skip.

The cost of doing nothing is a steady drip of forgotten AI artefacts
and an open `git blame` question every time someone asks "wait, what
did the AI say about that query yesterday?" The cost of bumping
schema versions is well-understood — ADR-0017's forward-compat policy
was designed for this exact moment, and brief 0003 explicitly reserved
v:2 for a "multi-statement results, query plan, etc." class of
extension (multi-record-type is the same shape of change).

### Decisions

**Decision 1 — Discriminator field, not parallel schemas.**

One record shape with a top-level `"kind"` string. `"kind": "query"`
records carry the v:1 fields. `"kind": "ai"` records carry the AI
fields. Reader dispatches on `kind` after the v gate.

Rejected: two parallel files (`history.jsonl` + `ai-history.jsonl`).
Doubles the rotation / permission / cross-repo coordination surface
for no UX win — `jq 'select(.kind == "ai")'` is already the right
filter, and the user wants one chronological feed.

Rejected: serde internally-tagged enum on `RecordWire`. Discriminator
serialisation works, but reader-side back-compat with v:1 (which has
no `kind` field) becomes awkward and the `Option<...>` per-variant
field collisions force a flat struct anyway. Hand-rolled dispatch on
the string is clearer and matches how the existing
`HistoryStatus::from_wire` already handles enum-on-the-wire.

**Decision 2 — Bump `CURRENT_VERSION` from 1 to 2; writers always emit v:2.**

No "stay on v:1 if no AI activity" config switch. A user opening a
mixed v:1 / v:2 file should see one consistent shape after the upgrade
date, not a flag-dependent format.

The writer emits `"v": 2, "kind": "query"` for SQL records (was
`"v": 1` with no kind) and `"v": 2, "kind": "ai"` for AI records.

**Decision 3 — v:2 reader accepts v:1 records as `kind: "query"`
implicitly; v:1 reader skips v:2 records via the existing gate.**

This is the migration path. The desktop binary upgrades first and
becomes a v:2 reader/writer; the web sibling stays on v:1 and skips
v:2 records (counter increments — already wired in ADR-0017 §6).
Web mirrors v:2 at its own pace.

A v:2 reader treats a v:1 record (no `kind`, has `sql`) as a
`Query` variant. A v:2 record with no `kind` is malformed — drop +
counter (same path as unknown `status`).

**Decision 4 — AI record fields (the wire shape).**

```jsonc
{
  "v": 2,
  "kind": "ai",
  "ts": "2026-06-30T05:12:01.456Z",       // RFC 3339 UTC ms (same constraint as v:1)
  "conn": null,                            // optional for AI; null when no DB context
  "actor": null,                           // desktop always null; web populates
  "intent": "explain",                     // "explain" | "suggest_sql"
  "prompt": "SELECT * FROM users …",       // user input verbatim (the `sql` for explain, the prompt for suggest)
  "response": "This query …",              // AI text verbatim; partial-on-cancel is preserved
  "status": "ok",                          // "ok" | "error" | "cancelled"
  "duration_ms": 4231,                     // submit → terminal reply wall-clock
  "tokens_in": 412,                        // null for default-impl 1-shot atomic + unknown
  "tokens_out": 218,                       // null for cancelled-before-first-Usage-event
  "provider": "anthropic",                 // provider id (resolved from AiProviderSlot)
  "model": "claude-sonnet-4-6",            // model id
  "stop_reason": "end_turn",               // "end_turn" | "max_tokens" | "stop_sequence" | "tool_use" | "refusal" | "other:<text>" | null
  "error": null                            // {category, message} when status="error"
}
```

Field constraints specific to AI:

- **`conn`**: `Option<String>` on the wire. Null when the panel was
  used without a connection context (the bind-to-current-connection
  affordance lives in ADR-0023, not here). Populated when the user's
  active connection is the one the AI was asked about.
- **`intent`**: enum on the wire. `"explain"` (AI explains SQL) /
  `"suggest_sql"` (AI generates SQL). Forward-compat: an unknown
  value triggers the skip-with-counter path (same gate as unknown
  `status`).
- **`prompt`**: verbatim user input. For `explain`, this is the SQL
  the user pasted. For `suggest_sql`, this is the natural-language
  request. **Not the schema TableInfo** — that goes into the optional
  `schema_summary` field if logged (deferred to a future ADR).
- **`response`**: verbatim AI text. On cancel, this is the
  accumulator state at cancel time (ADR-0026 Decision 12 — the user
  paid for those bytes, the history record preserves them).
- **`status`**: `"ok"` / `"error"` / `"cancelled"`. `cancelled`
  carries `error: null`. `error` carries an error envelope (see below).
- **`duration_ms`**: submit-time to terminal-reply wall-clock. On
  cancel, the duration up to the cancel signal.
- **`tokens_in` / `tokens_out`**: `Option<u32>`. Null when the
  provider didn't surface them (default-impl 1-shot atomic paths) or
  when cancel landed before the first `Usage` event. Cumulative at
  terminal time (ADR-0026 Decision 7 — replace-not-sum).
- **`provider`**: provider id resolved from the active
  `AiProviderSlot`. Lowercase short name ("anthropic", "ollama" when
  added). Stable identifier suitable for `jq 'select(.provider ==
  "anthropic")'`.
- **`model`**: model id string as the provider reports it
  ("claude-sonnet-4-6", etc.). The writer copies it verbatim.
- **`stop_reason`**: the `StreamEvent::MessageStop` reason string
  (mapped from `StopReason` enum). Null for atomic paths that don't
  surface one. `"other:<text>"` for the `StopReason::Other(String)`
  forward-compat variant.

**Decision 5 — Error envelope reuses v:1's `{category, message}` shape,
new categories for AI.**

```jsonc
"error": { "category": "provider", "message": "401 invalid API key" }
```

Categories for `kind: "ai"` records: `"network"` | `"provider"` |
`"configuration"`. Mirrors the `AiError` variants from ADR-0023 §5.
**`AiError::Cancelled` is NOT an error category** — cancel is a
top-level `status`, not an error (ADR-0026 Decision 12 carries
through to the persisted record).

The web mirror brief (0008) will document that web's AI taxonomy must
match this set. A new web-only category is a contract violation, same
rule as the v:1 DbError taxonomy in brief 0003.

**Decision 6 — Write point is the UI thread, symmetric to SQL records.**

The worker emits per-reply data (provider / model / tokens / stop /
error) as part of the existing terminal reply variants (no new Reply
type). The UI thread composes the `HistoryEntry::Ai { … }` from the
prompt it already holds (`AiPanel::input` snapshot at submit time),
the submit timestamp + duration, the reply payload, and appends to
the `PersistentHistoryStore` exactly the way SQL records flow today
(`record_submit` → `record_completion`).

Rejected: worker emits the record directly. The worker is stateless
wrt the persistent store today and Group A's slot/admin design
deliberately kept it that way. Routing through the UI thread also
keeps the in-memory ring and disk write in lockstep (which is the
ADR-0017 invariant — a disk write failure must not block the
in-memory update).

**Decision 7 — `AiResponse` and the streaming-terminal reply variants
gain provider/model fields.**

`AiResponse` (atomic path) and `Reply::AiStreamComplete` (streaming
path) each pick up `provider: String` + `model: String`. The
provider implements `AiProvider::identity()` returning `(provider,
model)` so the worker can stamp the reply without holding the slot
across the await.

`Reply::AiFailed` and `Reply::AiCancelled` also need
`(provider, model)` so the cancel/error history record can name what
*would* have answered. They become struct variants if they weren't
already.

This is the only change to ADR-0023's trait surface. It is additive
with a default impl (`Unknown` / empty string) so existing tests
compile.

**Decision 8 — Privacy. Verbatim logging. ADR-0024 permissions cover it.**

Same stance as v:1's `sql` field (ADR-0017 §7). AI prompts and
responses are logged byte-verbatim. A redactor would be a
perpetually-wrong heuristic with worse failure modes than verbatim
(redacting a SELECT's password column is harder than just
acknowledging the file's at-rest threat model).

ADR-0024's 0700 directory + 0600 file mode covers the at-rest
protection on Unix. Windows DACL stays the existing fallback.
README's AI section gains a one-sentence warning that AI history is
logged verbatim and lives under the same trust boundary as the rest
of `history.jsonl`.

**Decision 9 — Fixture regeneration is part of the same PR; web brief
is issued in the same PR.**

The `emit_history_fixture` example writes v:2 records once this lands
(at least one `kind: "query"` + one `kind: "ai"` line). The fixture
file delivered to web (`dbboard-web/apps/api/test/fixtures/desktop-history.jsonl`
per the 2026-06-23 handoff) needs a v:2 successor — the brief
documents the handoff procedure mirroring PR #29 + PR #31.

The web mirror brief (0008) lands in the same PR as this ADR so the
cross-repo coordination starts the moment desktop ships, not after
merge — same lead-time rule that made PR #33's explicit-no-op briefs
work for ADR-0021 and ADR-0023.

**Decision 10 — Bounded write size.**

Cap `prompt` and `response` at 64 KiB each at the writer (truncate
with `… [truncated at 64 KiB]` marker text appended). A 30-minute
multi-turn streaming session can in principle produce hundreds of
KiB; that wastes rotation budget for a record nobody reads back
in full anyway. The cap is on the persisted record only — the UI's
live view (`AiPanel::streaming.text`) is unbounded.

64 KiB matches the `dbboard-core::limits` text cap (see ADR-0008).
Future tuning is a config knob, not an ADR.

### Slice plan (suggested, not prescribed)

- **Slice a**: `dbboard-ui::history` v:2 reader + writer
  (`RecordWire` becomes a flat struct with optional fields, `kind`
  discriminator, v:1 back-compat read). Pure refactor with tests.
- **Slice b**: `dbboard-ai` `AiProvider::identity()` + `AiResponse`
  provider/model fields + the four terminal `Reply` variants gain
  `provider, model`. `dbboard-anthropic` impl + worker plumbing.
- **Slice c**: `dbboard-ui::ai` panel + `lib.rs` history write
  point: AI history record composed on terminal reply, appended to
  `PersistentHistoryStore`, in-memory ring updated.
- **Slice d**: docs sweep + `emit_history_fixture` v:2 update +
  README warning + roadmap tick + ADR-0027 status flipped to
  Accepted + brief 0008 status updated to "ready for web pickup".

### Out of scope (intentionally)

- **Schema field for the suggest-mode TableInfo schema.** Logging
  the schema-context blob would be useful and is the natural Group D
  / DDL-extraction follow-up. Skipped here to keep the v:2 surface
  narrow.
- **AI history viewer UI.** The egui history panel already lists
  entries; rendering AI records is a follow-up — Group C ships the
  *record*, not the rich viewer. A future PR adds an icon + a
  collapsible response body.
- **Multi-turn conversation linking.** Each AI call is a standalone
  record; threading is a future ADR.
- **Cost calculation.** `tokens_in * input_price + tokens_out *
  output_price` could be derived but lives outside this ADR — pricing
  tables change without notice and belong in a separate config-driven
  module if at all.
- **Server-side admin view.** Web's "tenant analytics over the AI
  history" is web-side, future, and explicitly out of brief 0008's
  Phase-2 scope.

### Open questions (TBD before slice c)

- For `suggest_sql`, the `prompt` field stores the natural-language
  request; should the `dialect` hint also be persisted? Leaning yes,
  as a separate optional top-level string. Cheap to add; cheap to
  read back.
- Should `intent` carry a `"streamed": bool` flag for grep-ability?
  Leaning no — streaming vs atomic is a transport detail, not a user-
  visible intent.

### Risks

- **Web's v:1 readers see a counter tick on every desktop session
  after the upgrade.** Expected, documented in brief 0008. Mitigation:
  brief 0008 sets a "by date X" target for web to mirror.
- **A user who downgrades desktop after a v:2 record is written
  loses access to that record's content** (v:1 reader skips it).
  Acceptable — desktop downgrades are not a supported flow, the
  upgrade direction is one-way per ADR-0017 §6.
- **Verbatim logging of AI prompts/responses raises the at-rest
  threat surface marginally.** Same mitigation as v:1's `sql` field
  (ADR-0024 permissions + the README warning).
- **`provider`/`model` exposure in the file is intentional but worth
  flagging.** It does not leak credentials; it does name the model
  used. README warning covers it.

### Implementation slicing impact

- `dbboard-ui::history` becomes the load-bearing module (the v:2
  enum / dispatch).
- `dbboard-ai` trait surface gains one method (`identity()`).
- `dbboard-anthropic` implements the new method.
- `dbboard-ui::worker` plumbs provider/model through the four
  terminal reply variants.
- `dbboard-ui::lib` adds the AI write point.
- `dbboard-ui::ai` is unchanged in behaviour but gains snapshot
  helpers for the UI thread to read what it needs to compose the
  record (prompt + intent + start time).

### SemVer impact (ADR-0011)

Additive on the trait + types. The on-disk schema bump (v:1 → v:2)
is a *forward-compatible* change in the reader direction (v:1
records still readable by v:2) and a *backward-incompatible* change
in the writer direction (v:1 readers skip new records, counter
ticks). The HTTP contract is unchanged. The cross-repo coordination
moves through brief 0008.

## ADR-0028 — Phase 4 Stage 2 Group D-1: Full DDL extraction via `DatabaseAdapter::describe_table`

- **Status:** Accepted (2026-07-02). Implementation tracker:
  [`.claude/issues/0011-ddl-extraction.md`](../.claude/issues/0011-ddl-extraction.md)
  (closed). Lands on `feature/ddl-extraction` across four commits:
  - Slice (a) `a42a27c` — `dbboard-core` trait method + `TableSchema` +
    `ColumnInfo` extension + `Capabilities::has_describe_table`
    (review notes addressed in `bba4072`).
  - Slice (b) `b509a36` — `describe_table` in the turso, d1, and
    postgres adapters with `has_describe_table = true` each.
  - Slice (c) `dfdaaca` — `SuggestRequest.full_schema` +
    Anthropic prompt rendering + worker `PrefetchSchema` fan-out
    (semaphore cap 8) + `AiPanel` "Include column details" checkbox +
    warning banner + 11-locale i18n keys. One deviation from the plan
    below: `apps/dbboard` **was** touched after all — the worker
    reaches the live adapter through a new narrow `SchemaSource`
    trait (same injection pattern as `ConnectionSwitcher`), which the
    binary implements over the server's `AppState`
    (`current_adapter()` made `pub`). Chosen over the "no binary
    wiring" assumption because the UI worker has no other in-process
    path to the live adapter; the HTTP contract stays untouched.
  - Slice (d) — this docs sweep.
  - Open questions above resolved as: no prompt-size cap in v1 (the
    toggle is opt-in per session and the ADR-0026 token meter makes
    cost visible; revisit if a friction report lands), and no cancel
    during the prefetch leg (the fan-out is short and bounded; the
    deferred Suggest that follows remains cancellable as before).
- **Activates:** ADR-0023 §9 deferred "Full DDL extraction on
  `DatabaseAdapter`" (Decision 7 said the queued method would be
  called `dump_schema`; this ADR names it `describe_table` for the
  reasons in Decision 1).
- **Prerequisite for:** ADR-0029 (function-calling), which will expose
  `describe_table` as a callable tool. `describe_table` is the concrete
  primitive that makes function-calling worth turning on.
- **No cross-repo brief.** `describe_table` is a desktop-side
  `DatabaseAdapter` trait extension. No HTTP contract change, no
  `history.jsonl` schema bump. Web has its own connection-management
  story (`POST /connections/:id/query`) and would decide its own
  DDL-fetching shape independently.

### Context

Three observations after Group A (ADR-0025 provider config) + Group B
(ADR-0026 streaming + cancel + tokens) + Group C (ADR-0027 AI history
v:2) motivate lifting the `list_tables()` surface:

1. **`list_tables()` returns only `TableInfo { schema, name }`** —
   just table names. When the user hits Suggest in the AI panel with
   a natural-language prompt like "list the top 10 recent orders by
   customer", the AI provider gets 15 table names and hallucinates
   column names half the time. The suggestions read plausibly but do
   not compile against the real schema. The friction is real and
   reported.

2. **`ColumnInfo` already exists in `dbboard-core::schema`** (fields:
   `name`, `declared_type`, `nullable`, `primary_key`) but is
   currently unused by any adapter. Half the type surface is already
   drawn — this ADR closes the loop by adding one required trait
   method that populates it and one new sibling struct
   (`TableSchema`) that carries the per-table result.

3. **Function-calling (ADR-0029, deferred) needs a real tool to
   expose.** The natural first tool for a database AI companion is
   "describe this specific table on demand." Without a
   `describe_table` primitive, ADR-0029 would have to invent one; with
   it, ADR-0029 collapses to trait plumbing + provider mapping. Ship
   `describe_table` first so the primitive is proven before the tool
   surface wraps it.

The scope is narrow on purpose: **columns + primary-key composition
only**. Indexes and foreign keys are deliberately out of scope
(see §Out of scope) — the intent is to close 80% of AI hallucination
with the smallest change, not to build a general-purpose schema
introspection API.

### Decisions

1. **New required trait method:** `async fn describe_table(&self,
   table: &TableInfo) -> DbResult<TableSchema>` on `DatabaseAdapter`.
   Takes the existing `TableInfo` (schema-qualified pair) so callers
   pass what `list_tables()` returned — no new naming ambiguity for
   `"public.users"` vs `"users"`. Returns a rich `TableSchema` (see
   Decision 2). **Default impl returns
   `DbError::Capability("describe_table not supported by this
   adapter")`** so pre-existing adapters compile unchanged and
   signal capability miss at runtime rather than a build break.

   Rejected: `describe_table(name: &str)` — cross-schema ambiguity.
   Rejected: `dump_schema() -> Vec<TableSchema>` (the ADR-0023 §7
   name) — dumps the whole DB in one call, wasteful for large
   schemas and awkward for the function-calling case (ADR-0029)
   which needs single-table lookups. `dump_schema` can be added as
   a batch helper in a future ADR if fan-out becomes a friction
   point.

2. **New `TableSchema` struct in `dbboard-core::schema`:**

   ```rust
   pub struct TableSchema {
       pub table: TableInfo,
       pub columns: Vec<ColumnInfo>,
       pub primary_key: Vec<String>,
   }
   ```

   `table` is the qualified identifier the caller passed. `columns`
   is ordered by ordinal position (each adapter's native ordering).
   `primary_key` is the *composite* primary-key column names in key
   order, empty when the table has no primary key. `ColumnInfo`'s
   existing `primary_key: bool` flag is retained (it stays convenient
   for single-column PKs and never disagrees with the composite
   list — invariant enforced by the adapter and the reader trusts it).

3. **`ColumnInfo` gains `ordinal: u32` and `default_value:
   Option<String>` as additive fields.** `ordinal` matches
   `information_schema.columns.ordinal_position` (Postgres, 1-based)
   / `PRAGMA table_info.cid` (SQLite, 0-based → +1 normalised).
   `default_value` is the raw DDL default expression as the engine
   reports it (e.g. `"nextval('users_id_seq'::regclass)"` on
   Postgres, `"0"` or `"CURRENT_TIMESTAMP"` on SQLite). `NULL`
   default (i.e. no default clause) → `None`. Retained for AI
   prompt fidelity — a column with `DEFAULT CURRENT_TIMESTAMP`
   suggests different SQL than one with no default.

   Rejected: parsing `default_value` into a typed enum. The value is
   engine-specific literal text and typed parsing would be lossy for
   sequence calls, expressions, and `CURRENT_TIMESTAMP` variants.
   The AI reads it as a hint, not as a value.

4. **`Capabilities::has_describe_table: bool` additive flag.**
   Default `false`. Adapters override in `capabilities()`. The UI
   uses the flag to decide whether the "Include column details"
   toggle is available (Decision 8) — greying it out on adapters
   that only ship names is honest, versus letting the user check the
   box and then surfacing `Capability` errors after each Suggest.

5. **Per-adapter SQL:**
   - **`dbboard-postgres`**: one SELECT against
     `information_schema.columns` (schema + name filter, ordered by
     `ordinal_position`) for columns, and one SELECT against
     `information_schema.table_constraints` JOIN
     `information_schema.key_column_usage` filtered on
     `constraint_type = 'PRIMARY KEY'` for the composite PK. Two
     round-trips per `describe_table` call. Ordering the second by
     `ordinal_position` gives the composite key in declaration
     order.
   - **`dbboard-turso`** and **`dbboard-d1`** (both SQLite): one
     `PRAGMA table_info('<name>')` call. That single result carries
     column name, type, nullability, default, ordinal (as `cid`),
     and the per-column `pk` flag (`0` = not PK, `n>0` = position
     in composite PK — we materialise the composite list by
     collecting columns with `pk > 0` sorted by `pk`). One round-trip
     per call. D1's HTTP transport re-uses the existing raw-query
     path (same envelope as `list_tables`).

6. **Missing tables are `DbError::Query`** ("table not found" / "no
   such table") — the natural engine response. This is not a new
   error category; the adapter propagates whatever the engine says.
   The UI reads it as a stale schema situation (user renamed a table
   between `list_tables()` and `describe_table()`) and can prompt a
   refresh.

7. **No caching in `dbboard-core` or the adapters.** Every
   `describe_table` call round-trips to the DB. Callers (the AI
   panel is the only caller for now) may cache above the trait if
   they want to, but the trait itself is transport-only. Rejected an
   in-adapter cache to keep the trait pure and to avoid staleness
   surprises: a schema change on the server should reflect on the
   next Suggest immediately.

8. **`SuggestRequest` gains `full_schema: Option<Vec<TableSchema>>`
   additive field.** When present, the AI provider serialises
   `full_schema` into the prompt (via a formatter the provider
   owns — Anthropic uses a compact `CREATE TABLE`-ish rendering)
   instead of the terse `schema: Vec<TableInfo>`. Both fields may
   be present on the wire; the provider always prefers
   `full_schema` when non-empty. `schema` remains for the
   names-only default and for tests. The existing `schema` field is
   not renamed or removed for one release (Cargo consumer
   back-compat).

9. **AI panel UI: "Include column details" checkbox.** In Suggest
   mode, when `has_describe_table` is true, the panel renders a
   checkbox (default off). When checked, the panel:
   - fans out `describe_table` calls in parallel for every entry in
     `list_tables()` before the Suggest fires (via a new
     `Command::PrefetchSchema { tables: Vec<TableInfo> }` /
     `Reply::SchemaPrefetched { schemas: Vec<TableSchema>, errors:
     Vec<(TableInfo, String)> }` round-trip),
   - shows an indeterminate progress spinner during fan-out,
   - populates `SuggestRequest.full_schema` with the successful
     results,
   - if any table fails, shows a non-blocking warning banner
     (`"3 tables could not be described — Suggest will use partial
     schema"`) but still fires the Suggest with what it got.

   Fan-out is capped at 8 concurrent `describe_table` calls via a
   `tokio::sync::Semaphore` (matches the AI worker's cancel-token
   budget from ADR-0026) so a 200-table Postgres schema does not
   hammer the connection pool. The checkbox state is not persisted
   across sessions (session-local egui state — same treatment as
   the Suggest/Explain radio).

10. **No HTTP contract change and no `history.jsonl` schema
    change.** `describe_table` is desktop-side. `history.jsonl`
    already carries the AI prompt verbatim (ADR-0027 §Decision 8);
    when `full_schema` is used the rendered schema appears inside
    the `prompt` field, which is the correct place for it. No
    schema-context blob is added as a top-level history field
    (would be Group D-2 or later territory if a rich viewer wants
    it structured).

### Alternatives considered

- **`dump_schema() -> Vec<TableSchema>` as the primitive** — see
  Decision 1 rejection. Awkward for function-calling, wasteful for
  large schemas. Adding it as a *batch helper* on top of
  `describe_table` is left to a future ADR if profiling shows
  per-table fan-out is the bottleneck.

- **Include indexes and foreign keys in v1.** Deferred to a future
  ADR. Indexes matter for query-planning suggestions; foreign keys
  matter for JOIN suggestions. Both are worth having but each adds
  a per-adapter SQL query, more struct fields to keep consistent
  across three adapters, and more prompt-formatting decisions on
  the provider side. Ship columns + PK first, watch for
  hallucination patterns that survive, then decide.

- **`ColumnInfo::default_value` as a typed enum** — rejected in
  Decision 3. Engine-specific literal text is the honest
  representation.

- **Cache `describe_table` results in the adapter for N seconds** —
  rejected in Decision 7. Adds a staleness knob for questionable
  benefit; the UI-side caller can memoise if needed.

- **A single trait method returning `Result<TableSchema,
  DbError>` per Some(TableInfo) but batch when input is
  `None`** — rejected as too clever. Two shapes on one method make
  every implementation harder to test and the docstring
  confusing.

- **Emit rendered `CREATE TABLE` DDL text directly (skip
  `TableSchema` struct entirely)** — rejected. AI consumption is
  the near-term use case but the struct is more useful for other
  future callers (schema browser UI, migration diff tooling,
  export). Formatting to CREATE TABLE is a rendering choice, not
  a data-model choice.

### Implementation slicing

Four slices on a single feature branch, one PR (ADR-0026 / ADR-0027
precedent). Each slice green through the pre-commit hook.

- **Slice (a)** — `dbboard-core`: add `TableSchema` struct
  (`schema.rs`), extend `ColumnInfo` with `ordinal` + `default_value`,
  add `describe_table` trait method with default `Capability` impl,
  add `Capabilities::has_describe_table`. Unit tests for the
  `has_describe_table` capability round-trip through JSON and the
  default trait impl surfacing the capability error. **No adapter
  touches yet** (default impl handles them).

- **Slice (b)** — per-adapter `describe_table` implementations plus
  the capability flip:
  - `dbboard-postgres`: `describe_table` + `has_describe_table =
    true`. Integration test against `postgres:16-alpine` via
    testcontainers (Docker-skip guard).
  - `dbboard-turso`: `describe_table` + `has_describe_table = true`.
    Uses `PRAGMA table_info`. Unit test against an in-memory libsql
    DB.
  - `dbboard-d1`: `describe_table` + `has_describe_table = true`.
    Reuses the existing HTTP envelope path with the `PRAGMA` query.
    Test via the mocked-HTTP layer.

- **Slice (c)** — `dbboard-ai` + `dbboard-ui`:
  - `SuggestRequest.full_schema: Option<Vec<TableSchema>>` additive
    field, `AnthropicProvider` renders it into the prompt when
    present (existing `schema` path stays for the names-only case).
  - `Command::PrefetchSchema` + `Reply::SchemaPrefetched` worker
    variants + fan-out with semaphore cap of 8.
  - `AiPanel` "Include column details" checkbox gated on
    `has_describe_table`, prefetch on Send, warning banner on
    partial failure. State machine tests for the toggle-on /
    toggle-off / partial-failure paths.

- **Slice (d)** — docs sweep: ADR-0028 status Proposed →
  Accepted, `docs/roadmap.md` Phase 4 Stage 2 Group D-1 tick,
  `README.md` AI section gains a one-paragraph note about the
  Include-column-details toggle (schema context bytes go into
  the AI provider's context window, cost implications), tracker
  issue `.claude/issues/0011` closed, `.claude/project-status.md`
  slice landing record. `.claude/next-actions.md` regenerated
  for the post-Group-D-1 state.

### Out of scope (intentionally)

- **Function-calling / tool-use.** ADR-0029, sibling ADR under
  Group D. `describe_table` becomes the first exposed tool there.
- **Indexes and foreign keys.** Future ADR when hallucination
  patterns identify the specific gap. Adds one query per adapter
  and prompt-shape decisions.
- **`describe_view()` / `describe_function()`.** The existing
  optional trait accessors (`views()`, `functions()`) can grow
  their own describe methods when there is a use case; the AI
  panel does not currently need them.
- **Batch `describe_tables(&[TableInfo])`.** See Decision 1.
  Fan-out from the UI is enough for the caller sizes we ship
  today (< 100 tables typical).
- **Schema browser UI.** A tree view of tables → columns is a
  natural follow-up that consumes `describe_table` but is not
  gating for the AI use case. Deferred.
- **Persisting the "Include column details" toggle across
  sessions.** Session-local for v1. If the toggle becomes an
  always-on preference for a given user, a future ADR can lift it
  into `ai-providers.toml` or a sibling `ui-preferences.toml`.
- **`CREATE TABLE` text generation.** `TableSchema` is the
  structural primitive; rendering it as SQL is a viewer / exporter
  concern for a later ADR.
- **Caching.** Every call round-trips (Decision 7).

### Open questions (TBD before slice c)

- Should the prefetched schema block be trimmed when it exceeds a
  budget (e.g. 32 KiB of rendered prompt)? Leaning yes with a
  degrade-and-warn path, but the exact cap is worth setting from a
  measured Anthropic context-window cost rather than a guess.
- Should `Command::PrefetchSchema` accept a cancel token so the
  user can back out during a slow fan-out? Leaning yes — the
  existing cancel path from ADR-0026 gives us the machinery
  cheaply.

### Risks

- **Prompt cost.** Full schema for a 200-table DB blows the
  Anthropic context budget. Mitigation: the toggle is off by
  default and the UI shows the raw token count in the meter
  (already shipped in ADR-0026); Decision 9 caps the fan-out for
  DB-side pressure, and the open question above covers a
  prompt-side cap.
- **Fan-out load.** 200 tables × 1-2 queries each is a lot for a
  shared Postgres. Semaphore cap of 8 is Decision 9's mitigation;
  if that is still too much for a shared prod DB, the user can
  keep the toggle off and rely on names-only Suggest.
- **Cross-adapter type drift.** Postgres reports `text` /
  `character varying(N)` / `numeric(p, s)`; SQLite reports
  affinity strings (`INTEGER` / `TEXT` / `REAL` / `BLOB`). We do
  not normalise across adapters — `declared_type` is raw. The AI
  reads dialect via `SuggestRequest.dialect`, so mixed
  interpretations should not surface. Called out here so we
  notice if it does.
- **Stale `TableInfo` between `list_tables` and `describe_table`.**
  Covered by Decision 6 (`DbError::Query` → UI prompts refresh).
  Nothing structurally can prevent this race in a live DB; the
  fallback is graceful.

### Implementation slicing impact

- `dbboard-core` gets one new required-with-default trait method
  (compiles for existing adapters — `Capability` error at runtime
  is the "please implement me" signal, matched by ADR-0028 shipping
  all three adapters in slice (b)).
- `dbboard-ai` `SuggestRequest` gains an `Option` field. Provider
  crates that ignore it keep working (existing tests pass).
- `dbboard-ui` grows the checkbox + prefetch worker plumbing.
- `apps/dbboard` is untouched (no new binary wiring).

### SemVer impact (ADR-0011)

Additive on the trait + types. Existing adapters compile unchanged
(the trait method has a default impl). `SuggestRequest` gains an
optional field. `Capabilities` gains a boolean with a `false`
default. No HTTP contract change. No `history.jsonl` schema
change.

## ADR-0030 — Result grid: `egui_extras::TableBuilder` (sticky header, virtualized rows, column separators)

- **Status:** Accepted (2026-07-10). Lands on `feature/query-ux`
  alongside the query-UX batch (run triggers, auto-LIMIT guard,
  structure tab, long-text popup). UI-only; no crate contract, no
  HTTP contract, no `history.jsonl` change.

### Context

The result table was drawn with `egui::Grid` inside a
`ScrollArea::both()`: every row and every cell was laid out each
frame, the header row scrolled away with the body, and there were
no vertical separators between columns. Three concrete failures
drove this ADR, all reported from real use against a Cloudflare D1
store:

1. **Freeze on large result sets.** A bare `SELECT` with no `LIMIT`
   materialised thousands of rows; `egui::Grid` lays out *all* of
   them per frame, hanging the UI. (The row *count* is separately
   capped by the auto-LIMIT guard, but the grid must not be the
   bottleneck.)
2. **Header scrolls out of view.** Scroll down through a wide table
   full of `NULL`s and you lose track of which column is which.
3. **No column boundaries.** Row striping alone is not enough to
   track a value across a wide row; the user asked for faint
   vertical lines.

`egui::Grid` structurally cannot fix (1) or (2): it has no
virtualization and no frozen header. `egui_extras::TableBuilder` —
egui's official companion crate, same maintainer, same version
cadence — is purpose-built for exactly this and gives all three for
free.

### Decision

Add `egui_extras` (0.34, pinned to the egui version, default
features off) and rebuild `render_result` on `TableBuilder`:

1. **Sticky header** via `.header(height, |h| …)` — the header band
   stays fixed while the body scrolls.
2. **Virtualized body** via `.body(|body| body.rows(row_h, n, …))`
   — only visible rows are laid out, so wall-clock is independent of
   result size.
3. **Column separators** via resizable columns
   (`Column::auto().resizable(true)`), which draw a faint vertical
   line at each boundary and, as a bonus, let the user drag column
   widths.
4. **Striping** retained via `.striped(true)`.

### Consequences

- New workspace dependency. Justified per CLAUDE.md ("non-trivial
  crate → ADR"): it is the first-party companion to a dependency we
  already ship, so maintenance/version risk is minimal.
- `render_result`'s signature is unchanged (`&mut egui::Ui,
  &QueryResult`); the rewrite is internal. Existing behavioural
  tests over `QueryResult` shaping are unaffected.
- Long-text cells (the truncation-with-popup feature) render inside
  the same `TableBuilder` body cell, so the two features share one
  grid rewrite rather than fighting `egui::Grid`.

### SemVer impact (ADR-0011)

None. Presentation-only change inside `dbboard-ui`. No public type,
trait, HTTP envelope, or on-disk schema is touched.

## ADR-0031 — Structure tab: click a table to inspect its columns

- **Status:** Accepted (2026-07-10). Lands on `feature/query-ux`
  with the rest of the query-UX batch. UI + worker-plumbing only;
  reuses the ADR-0028 `describe_table` primitive. No crate contract,
  HTTP contract, or `history.jsonl` change.

### Context

The sidebar listed tables but clicking one did nothing — there was
no way to see a table's columns without hand-writing `PRAGMA
table_info(...)` (SQLite-only) or the Postgres `information_schema`
equivalent. HeidiSQL and every desktop client answers this with a
structure view. ADR-0028 already shipped a cross-adapter
`DatabaseAdapter::describe_table` returning a `TableSchema`
(columns, types, nullability, PK, defaults), used so far only by the
AI prefetch path. The data is already there; only the surfacing is
missing.

### Decision

1. **Tab the lower panel.** A `ResultTab { Results, Structure }`
   toggle sits above the result area. Running a query does not force
   a tab switch; clicking a table does.
2. **Click a sidebar table → describe it.** Sidebar entries become
   `selectable_label`s. A click calls `open_structure`, which flips
   to the Structure tab and sends a new `Command::DescribeTable {
   table }`.
3. **Dedicated command/reply pair.** `Command::DescribeTable` →
   `Reply::TableDescribed { table, result }`, handled by the worker
   through the same injected `SchemaSource` as `PrefetchSchema` but
   scoped to one table. Kept separate from `SchemaPrefetched` so the
   structure view and the AI prefetch flow never contend for one
   reply.
4. **Stale-reply guard.** `TableDescribed` is applied only when its
   `table` still matches the on-screen `StructureView`; a describe
   for a since-reselected table is dropped.
5. **Render via `TableBuilder`** (ADR-0030): ordinal / name / type /
   nullable / key / default, one row per column.

Cross-adapter `describe_table` is used rather than emitting
SQLite-specific `PRAGMA` / `sqlite_master` SQL from the UI, so the
structure tab works uniformly on D1, Turso, and Postgres. The raw
`CREATE TABLE` DDL (a HeidiSQL nicety, and SQLite-specific) is left
for a later slice; the column grid covers the primary need.

### Consequences

- `Command` / `Reply` each gain one variant. Both are `dbboard-ui`
  internal enums (the worker channel), not the public HTTP contract,
  so this is not a SemVer event. Every exhaustive match on them (the
  worker dispatch, the fatal-error dispatcher, `request_for`,
  `pending_ai_from_command`) gains an arm.
- Connections whose adapter lacks `describe_table` surface a
  `DbError::Capability` in the tab rather than silently doing
  nothing.
- `structure-*` / `tab-*` keys added across all 11 locales.

### SemVer impact (ADR-0011)

None on the published surface. The new `Command` / `Reply` variants
are internal to `dbboard-ui`. No adapter trait, HTTP envelope, or
on-disk schema changes.


## ADR-0032 — Windows packaging: console suppression, exe metadata, CRT-static, MSI via cargo-wix

- **Status:** Accepted (2026-07-10). Lands on `feature/windows-packaging`.
  Build/packaging only — no source-behaviour, crate-contract, HTTP-contract,
  or `history.jsonl` change. Windows-only; a no-op on macOS/Linux builds.

### Context

The maintainer wants to hand `dbboard` to internal users on **Windows
only, for now**. A release binary already builds and runs with no config
(`target/release/dbboard.exe`, ~15 MB; libsql/ring statically linked;
falls back to in-memory Turso and configures connections/AI from the UI
with secrets in Windows Credential Manager). But it was not
distribution-ready:

1. **A console window flashed behind the GUI** — no
   `#![windows_subsystem]` anywhere.
2. **Default blank Rust icon, no version/product metadata** — the exe
   looked anonymous in Explorer, the taskbar, and the Details tab.
3. **Dynamic MSVC CRT** — recipients without the Visual C++
   Redistributable would hit a `vcruntime140.dll`-missing error.
4. **No installer** — only a loose exe, no packaging, no
   `.github/workflows/` release automation.

### Decision

Adopt four changes, gated so non-Windows builds are unaffected.

1. **Suppress the console on release builds.**
   `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`
   at the crate root of `apps/dbboard/src/main.rs`. Debug builds keep the
   console so `println!`/panic traces stay visible during development.

2. **Embed icon + metadata via `winresource`.** A new `apps/dbboard/build.rs`
   (Windows-only `build-dependency`) sets the icon and the ProductName /
   FileDescription / CompanyName / LegalCopyright / OriginalFilename
   strings; FileVersion / ProductVersion default from `CARGO_PKG_VERSION`.
   The icon `apps/dbboard/assets/dbboard.ico` is a hand-built
   multi-resolution (16–256 px) PNG-based ICO — an indigo rounded square
   with a white database-cylinder glyph. It was generated with a
   throwaway PowerShell + GDI+ script (checked into scratch, not the
   repo) because no image tooling or brand asset existed; the `.ico`
   itself is committed.

3. **Statically link the CRT.** `.cargo/config.toml` sets
   `-C target-feature=+crt-static` for
   `cfg(all(windows, target_env = "msvc"))`, so the exe is self-contained
   and needs no VC++ Redistributable. Cargo drops the flag for
   proc-macro crates automatically, so the workspace still builds. Verified
   on the release exe: **zero** `vcruntime`/`msvcp`/`ucrtbase`/`api-ms-win-crt`
   references in the import table.

4. **MSI installer via `cargo-wix`.** `apps/dbboard/wix/main.wxs`
   (WiX v3, hand-authored to match cargo-wix's `$(var.Version)` /
   `$(var.CargoTargetBinDir)` variables) + `apps/dbboard/wix/License.rtf`
   (MIT) + a `[package.metadata.wix]` block. It installs to
   `%ProgramFiles%\dbboard`, offers an opt-out PATH sub-feature, wires the
   Add/Remove-Programs icon, and shows the MIT license. The UpgradeCode and
   the PATH component GUID are **fixed** (baked in both `main.wxs` and the
   metadata) so in-place upgrades and uninstall PATH-cleanup work.

MSI was chosen over a bare zip or `cargo-bundle` because internal IT can
push an MSI via GPO/Intune, it registers a clean uninstall entry, and it
is the least surprising format for Windows recipients.

### Consequences

- **New tooling the human must install to *build* the MSI** (not to build
  the exe): the WiX Toolset v3 (candle/light) and `cargo install cargo-wix`.
  Neither is on the maintainer's machine yet, so `cargo wix` is a
  human-run step. The exe hardening (1–3) needs no new tools and is
  verified working here.
- The `.cargo/config.toml` `crt-static` flag invalidates the build cache
  once (full rebuild) and applies workspace-wide on the MSVC target.
- No CI yet: this ADR sets up local packaging only. A release workflow
  (`cargo wix` on a tagged push) is a later, optional follow-up.
- Desktop-only; the dbboard-web sibling is unaffected. No cross-repo brief.

### SemVer impact (ADR-0011)

None. No public surface changes — this is build configuration, a build
script, an icon asset, and installer source.

## ADR-0033 — Enable the keyring OS credential-store backend (secrets were silently non-persistent)

- **Status:** Accepted (2026-07-13). Fixes a runtime defect found during
  the first internal Windows run (ADR-0032). Dependency-feature +
  UI-visibility change; no HTTP-contract, `history.jsonl`, or public-API
  change. Affects every platform, not just Windows.

### Context

The first user to run the packaged Windows exe reported that **no
registered connection could connect, and clicking "Connect" did nothing**.
Two independent defects were behind the single symptom:

1. **Silent switch failures (UI gap).** The in-process connection switch
   (ADR-0020) reports failure via `Reply::SwitchFailed`, which
   `DbboardApp` stored in `last_switch_error` — but *no render path ever
   read it*. A failed Connect updated no marker and showed no message, so
   the click looked inert ("無反応"). The getter's own doc comment
   ("so the UI can render 'could not connect to <id>'") described wiring
   that was never done.

2. **Root cause — the keyring never persisted anything.** `keyring 3.x`
   ships **no `default` feature**, and therefore **no credential-store
   backend** unless one is opted into explicitly. dbboard depended on
   `keyring = "3"` with default features, so on *every* platform it
   silently resolved to the in-memory **mock** store. Every
   `SecretStore::set` returned `Ok` (the mock accepted the write) but the
   value lived only on that one `Entry` object; a fresh `Entry` for the
   same key — which is exactly what the runtime switcher constructs —
   read back `NoEntry`. Net effect: `ConnectionAdmin::add` succeeded and
   wrote the TOML, but `backend_config_for_entry` later failed with
   `config secret failed: no secret stored for reference:
   dbboard.<id>.token`. Windows Credential Manager held zero dbboard
   entries (`cmdkey /list` empty), confirming nothing was ever stored.

   A standalone round-trip reproduced it precisely: with default features,
   `set_password` → `Ok`, then `get_password` on a new `Entry` →
   `No matching entry found`. With `windows-native` enabled, the same
   round-trip returned the stored value. The crate already had a live
   round-trip test (`keyring_store_round_trips_through_the_os_keychain`)
   but it is `#[ignore]`d (it touches the real keychain), so CI and the
   pre-commit hook never exercised the real backend and the mock slipped
   through.

### Decision

1. **Opt into the real OS keychain backend, per target**, in
   `crates/dbboard-config/Cargo.toml`:
   - `cfg(windows)` → `windows-native`
   - `cfg(target_os = "macos")` → `apple-native`
   - `cfg(target_os = "linux")` → `linux-native-sync-persistent` +
     `crypto-rust`

   Target-scoped on purpose: the Linux secret-service backend pulls a
   dbus C binding that must not be built on Windows/macOS. The base
   `[dependencies] keyring` entry is kept so the crate still compiles
   (mock fallback) on any target outside the three cfg blocks.

2. **Surface switch failures in the UI.** `DbboardApp::switch_error_message()`
   formats a localized, display-ready message (localized prefix
   `connections-switch-error` in all 11 locales + the target id + the wire
   error, matching the `ai.rs` error-prefix house style). The Connections
   window renders it red, above the list, next to the Connect buttons.

### Consequences

- **Existing broken entries need their secret re-entered once.** Values
  "stored" before this fix never reached the keychain, so after upgrading
  the user must Edit each secret-bearing connection, tick
  "Replace token"/"Replace URL", paste the secret, and Save. Subsequent
  runs persist correctly.
- `Cargo.lock` gains `windows-sys` + `byteorder` (keyring's Windows
  backend deps). Binary project → lockfile committed.
- The `#[ignore]`d live round-trip test now passes with the backend
  enabled; it would have failed (mock store) before this change. It stays
  `#[ignore]`d for CI but is the manual regression guard
  (`cargo test -p dbboard-config -- --ignored`).
- Desktop-only; the dbboard-web sibling is unaffected. No cross-repo brief.

### SemVer impact (ADR-0011)

None. No public API surface changes — a dependency feature flag, one new
public getter (`switch_error_message`) on the binary's app type, and UI
wiring.

## ADR-0034 — Trust the OS certificate store (rustls native roots) so TLS-inspecting middleboxes don't break DB connections

- **Status:** Accepted (2026-07-13). Fixes a runtime defect found during
  the first internal Windows run, on the same machine as ADR-0032/0033.
  Dependency-feature change only; no HTTP-contract, `history.jsonl`, or
  public-API change. Affects every platform, most visibly Windows.

### Context

With the keyring backend fixed (ADR-0033) and the worker-runtime panic
fixed (see below), the first real D1 Connect finally reached the network
— and failed with `connection failed: error sending request`. The
Postgres-family adapters (Neon / Supabase / Aurora DSQL) would fail the
same way.

`error sending request` is reqwest's bare transport error: DNS resolved
and TCP connected, but the **TLS handshake was rejected**. The machine
runs Norton, which performs HTTPS interception: an
`SSLKEYLOGFILE=\.\nllMonFltProxy\…` env var (Norton LifeLock Monitor
Filter Proxy) was present, and `curl` to `api.cloudflare.com` failed with
`CRYPT_E_NO_REVOCATION_CHECK` unless `--ssl-no-revoke` was passed — proof
that a local middlebox re-signs every HTTPS connection with its own CA.

That CA is installed in the **Windows certificate store** (so browsers
and `curl`/schannel trust it), but dbboard's TLS stack did **not** consult
it:

- `reqwest` used `rustls-tls` → **webpki-bundled Mozilla roots** only.
- `sqlx` used `tls-rustls-ring`, which aliases `tls-rustls-ring-webpki` →
  same webpki-only roots.

rustls therefore saw a certificate chaining to Norton's CA — absent from
the webpki set — and aborted the handshake, surfaced as the contentless
`error sending request`. A webpki-only client is broken behind *any*
TLS-inspecting AV or corporate proxy, which is the common case on a
managed Windows desktop.

A third defect sat between the keyring fix and this one: the ADR-0020
`DesktopSwitcher::switch` built the adapter with
`self.rt.block_on(build_adapter(..))`, but `switch` runs inside the
worker's `current_thread` runtime (it is called from `run_command_loop`).
`Handle::block_on` from within a runtime **panics** ("Cannot block the
current thread from within a runtime"), which silently killed the
command-loop thread and made every later Connect a no-op ("無反応"). It
had been masked because `backend_config_for_entry` previously failed
*ahead* of the `block_on`; once the secret resolved (ADR-0033), the panic
became reachable. Fixed by `build_adapter_on`: spawn the build onto the
multi-thread server runtime and park the worker thread on a channel — no
`block_on`, no panic, switch stays inline. Covered by
`build_adapter_on_does_not_panic_inside_the_worker_runtime`.

### Decision

Trust the **OS certificate store** for all outbound HTTPS, staying on
pure-Rust rustls:

1. `reqwest` → `rustls-tls-native-roots` (was `rustls-tls`). Applies to
   the D1 adapter and the Anthropic AI provider.
2. `sqlx` → `tls-rustls-ring-native-roots` (was `tls-rustls-ring`).
   Applies to Neon / Supabase / Aurora DSQL.

`rustls-native-certs` only *reads* the OS trust store; it pulls in no
OpenSSL, so the "pure-Rust, self-contained Windows build" property from
ADR-0018/0019 is preserved. Verified on the affected machine: a reqwest
client with the exact D1 builder config (`use_rustls_tls().https_only(true)`)
reaches `api.cloudflare.com` and gets a real HTTP status under
native-roots, where webpki roots gave `error sending request`.

### Consequences

- **Security posture:** dbboard now trusts every CA the OS trusts,
  including AV/corporate interception CAs. This matches browser and
  system-tool behavior on the same host and is the expected default for a
  desktop client; it is a deliberate move away from the stricter
  webpki-only pin. A future ADR may add an opt-in "pin to webpki roots"
  toggle for users who want to refuse interception.
- No online revocation checks: rustls does not do OCSP/CRL, so it does not
  hit the `CRYPT_E_NO_REVOCATION_CHECK` that stopped schannel.
- `Cargo.lock` gains `rustls-native-certs` (+ the OS bridge, e.g.
  `schannel` on Windows). Binary project → lockfile committed.
- Desktop-only; the dbboard-web sibling (its own Node TLS stack) is
  unaffected. No cross-repo brief.

### SemVer impact (ADR-0011)

None. Two dependency feature-flag changes plus one internal helper
(`build_adapter_on`) on the binary. No public API surface change.

## ADR-0035 — Export a result set to CSV / TSV (copy to clipboard, save via native dialog)

**Status:** Accepted 2026-07-13

### Context

A query result is often something the operator wants to share or hand
off — the same need HeidiSQL serves with its grid export. Until now the
only way out of dbboard's result grid was a mouse drag-select of the
rendered text, which is fragile over the virtualized `egui_extras`
table (only the on-screen rows exist as widgets) and loses column
structure. Users asked for first-class "copy" and "download" of results.

### Decision

Add a result-export toolbar above the grid (`render_export_toolbar`),
delivered in two slices:

- **Slice 1 (this ADR):** whole-result export.
  - **Copy** → the entire result on the clipboard as **TSV**
    (`ui.ctx().copy_text`), which pastes into Excel / Google Sheets with
    columns intact.
  - **Save CSV…** → a native OS "Save As" dialog (`rfd`) that writes
    **RFC 4180 CSV** to the chosen path.
- **Slice 2 (follow-up):** row selection (click / Ctrl-click /
  Shift-click) plus "copy selected" / "save selected", reusing the same
  serializer over a row subset.

The serialization lives in a pure, I/O-free `export` module
(`to_csv` / `to_tsv` over `&[Column]` + `&[Row]`) so the wire format is
unit-tested without a grid, clipboard, or file dialog. Both formats share
RFC 4180 quoting (quote only when a field carries the delimiter, a quote,
or a line break; double embedded quotes). `NULL` serializes as an empty
field — what a spreadsheet expects — rather than the literal "NULL" the
grid shows. Records are separated, not terminated (no trailing newline),
so pasting TSV does not leave a dangling empty row.

### Consequences

- New dependency **`rfd`** (Rusty File Dialog, MIT/Apache-2.0) for the
  native save + error dialogs. Pure-Rust bindings over the OS pickers
  (Win32 `IFileDialog` / macOS `NSSavePanel` / Linux GTK or xdg-portal).
  On Linux the default backend needs GTK3 dev libraries at build time;
  the maintainer builds on Windows, where no extra system libs are
  required. `rfd`'s dialogs are synchronous — the brief frame stall while
  the OS dialog is open is normal desktop behaviour.
- A failed file write is reported via a native `rfd::MessageDialog`
  rather than swallowed, keeping `render_result` a stateless free
  function (no new app-state field for a transient error).
- The saved `.csv` is written **UTF-8 with a BOM** (`to_csv_with_bom`).
  Excel on Windows assumes the system ANSI code page (Shift-JIS on
  Japanese Windows) for a BOM-less CSV and shows UTF-8 text as mojibake;
  the BOM makes it auto-detect UTF-8. The clipboard TSV stays BOM-less
  (the clipboard carries Unicode natively). Known limit: pasting TSV into
  Excel does not parse RFC 4180 quotes, so a cell with an embedded
  newline spills across rows on paste — opening the CSV file (which is
  quote-parsed) keeps such cells intact.
- Blob cells are exported using their `<blob: N bytes>` display
  placeholder, not their bytes — round-tripping binary through CSV is out
  of scope for slice 1.
- Desktop-only presentation feature; no wire-contract change, so the
  dbboard-web sibling is unaffected and no cross-repo brief is needed.

### SemVer impact (ADR-0011)

None. Additive UI feature plus one new internal `export` module and one
new dependency. No change to any published API surface (the workspace is
unpublished; `dbboard-core`'s contract is untouched).

### Addendum — Slice 2: row selection (2026-07-13)

Row selection ships as designed, with one refinement learned from
hands-on use. The first cut sensed clicks across the **whole row**
(`TableBuilder::sense(Sense::click())`); in practice it felt sluggish and
unreliable because the row-level sense competed with the cells' own
interactive widgets (the expand-affordance from ADR-0030), and it would
also foreclose the cells for future in-cell interaction (edit,
drag-select for a partial copy).

Decision: put row selection behind a dedicated **leading gutter column**
(1-based row numbers, like a spreadsheet row header). Only the gutter
cell is a click target; the data cells stay non-sensing and free for
later use. The gutter uses a full-width `top_down_justified`
`selectable_label` so the whole cell — not just the digits — is
clickable. The whole row still highlights via `TableRow::set_selected`,
so the selection reads across all columns.

The selection state machine is a pure, egui-free `selection` module
(`ResultSelection` + `ClickModifiers`) so the click / Ctrl / Shift rules
are unit-tested without a UI:

- **plain** click → select only that row (anchor there);
- **Ctrl** click → toggle that row (anchor there);
- **Shift** click → inclusive range from the anchor (plain Shift
  replaces, Ctrl+Shift extends); anchor stays put so the range
  re-drags from the same origin.

`ClickModifiers.ctrl` maps to egui's `Modifiers::command`, so the toggle
gesture is ⌘ on macOS and Ctrl elsewhere. `command`/`shift` are read from
`ui.input` at click time. The click is captured into a local and applied
**after** the table body so the selection can't shift mid-iteration and
leave virtualized rows below the click reading a stale highlight.
`DbboardApp::result_selection` is cleared whenever a new `QueryResult`
replaces the grid — the old indices no longer point at the same rows.

Selected-row export reuses slice 1's serializer: `selected_rows` collects
the chosen rows (bounds-checked, ascending order) into an owned `Vec<Row>`
on the copy/save click only (not per frame), then hands it to the same
`to_tsv` / `to_csv_with_bom` path. No new serialization surface. Still a
desktop-only presentation feature; no wire-contract change.

## ADR-0036 — Aurora DSQL with self-minted IAM auth tokens (`aurora-dsql-iam`)

**Status:** Accepted 2026-07-14

### Context

ADR-0021 shipped the `aurora-dsql` connection kind, which stores a
**pre-generated** IAM authentication URL under `keyring_url_ref`. Aurora
DSQL's IAM tokens have a ~15-minute TTL, so that kind only works if the
operator re-pastes a fresh token every quarter hour. That is fine for an
occasional interactive session but unusable for the near-term rollout: a
team wants dbboard connected to several DSQL clusters **24/7** for
continuous multi-database data collection (see project memory,
"Aurora DSQL permanent connection required", 2026-07-13). They cannot
hand-refresh a token every 15 minutes.

The AWS SDK can mint DSQL tokens, but adopting it pulls in `aws-lc-rs` as
a transitive crypto backend, which directly conflicts with ADR-0034's
decision to standardise on rustls + `ring` (no `aws-lc-rs`). We need
token minting **without** the AWS SDK.

### Decision

Add a new connection kind **`aurora-dsql-iam`** that stores long-lived
AWS credentials and derives a fresh SigV4 presigned-URL token itself at
connect time, rather than storing a short-lived token.

- **Config shape** (`ConnectionKind::AuroraDsqlIam`): `endpoint`,
  `region`, `database`, `username`, and `access_key_id` are non-secret
  and live inline in `connections.toml`; only the AWS **secret access
  key** is a secret, referenced through `keyring_secret_key_ref` and
  resolved from the OS keychain. The TOML discriminator is
  `kind = "aurora-dsql-iam"` (kebab-case). Because the AWS access key id
  (`AKIA…`) is a public identifier, not a credential, storing it inline
  keeps the file self-describing while leaking nothing.
- **Hand-rolled SigV4** (`dbboard-postgres/src/dsql_auth.rs`): the token
  is a `GET` presigned URL to `{endpoint}/?Action=DbConnectAdmin` (when
  `username == "admin"`) or `?Action=DbConnect` (otherwise), service
  `dsql`, `SignedHeaders=host`, payload hash `SHA256("")`, with the
  leading `https://` stripped and the result used as the Postgres
  password. It is built from pure-Rust `hmac` + `sha2` + `hex` +
  `percent-encoding` + `time` — all already transitive in `Cargo.lock`,
  so no new supply-chain surface and, crucially, **no `aws-lc-rs`**
  (ADR-0034 stands). The HMAC signing-key chain is validated in-crate
  against AWS's own published test vector.
- **Mint-at-build (段階A)**: v1 mints one token when the adapter is built
  — at startup and on every connection switch. sqlx 0.8 has no
  per-connection password callback, so a live pool cannot re-sign
  mid-flight. Programmatic `PgConnectOptions` construction (not a URL
  string) is used so the token's `%2F` sequences are not double-decoded.
- **Config-file-only in v1**: the kind is created by hand-editing
  `connections.toml`. The connection list shows it and can Connect and
  Delete it, but the Edit button is gated off (there is no Add/Edit form
  yet), to bound scope and avoid an 11-locale i18n lift for a kind whose
  primary operator hand-authors the file anyway.

### Consequences

- **Known v1 limitation (段階A)**: because the token is minted only at
  build time, any *new physical connection* opened more than ~15 minutes
  after the last adapter build fails until the adapter is rebuilt. This
  bites a cold reconnect after the app has idled, **and — confirmed by a
  live smoke test on 2026-07-14 — a long-running 24/7 pool too**: Aurora
  DSQL closes idle server-side connections, and when `sqlx` re-opens one
  it replays the *same* now-expired token as the password, so the server
  answers `unable to accept connection, access denied`. So 段階A does not
  by itself satisfy the unattended 24/7 goal; automatic in-pool token
  refresh (段階B) — a background re-sign before expiry — is the real fix
  and is deferred to a follow-up ADR.
- **Manual recovery path (段階A stopgap)**: the connections window's
  active-row button is relabelled **Reconnect** (previously a disabled
  Connect under ADR-0020) so a single click rebuilds the adapter and
  mints a fresh token when the pool has been rejected. This makes the
  段階A limitation recoverable without an app restart; it does not remove
  the need for 段階B under truly unattended operation.
- **No new dependencies**: `hmac`, `sha2`, `hex`, `percent-encoding`, and
  `time` were already in the lock file; they are promoted to explicit
  `dbboard-postgres` dependencies. `Cargo.toml` gains a workspace entry
  for each.
- **Secret hygiene**: the AWS secret access key never touches a tracked
  file or a `Debug` output. `BackendConfig::AuroraDsqlIam` has a
  hand-written `Debug` that redacts the whole struct;
  `ConnectionKind::AuroraDsqlIam` stores only a keyring *reference*; the
  store's existing "no secret value in serialized TOML" test covers the
  new kind.
- **Reuses the Aurora DSQL flavor**: the adapter connects via
  `FLAVOR_AURORA_DSQL`, so `id()`, capability output, and history records
  label it identically to the ADR-0021 kind — the only difference is
  where the token comes from.
- **Web sibling**: desktop-only (this is a local credential-handling and
  connection concern). No HTTP wire-contract change, so the dbboard-web
  sibling is unaffected and no cross-repo brief is needed.

### SemVer impact (ADR-0011)

None to any published contract (the workspace is unpublished and
`dbboard-core` is untouched). Additive: one new `ConnectionKind` variant,
one new `BackendConfig` variant, one new `PostgresAdapter` constructor,
and one new internal `dsql_auth` module.

## ADR-0037 — Aurora DSQL IAM in-pool token auto-refresh (段階B)

**Status:** Accepted 2026-07-14

### Context

ADR-0036 shipped the `aurora-dsql-iam` kind, which self-mints a SigV4 IAM
token instead of storing a pre-generated one. But it mints the token
**once, at adapter build time** (startup and connection switch). ADR-0036
already recorded the consequence, which a live smoke test on 2026-07-14
then confirmed: Aurora DSQL closes idle server-side connections, and when
`sqlx` re-opens one it replays the *same* now-expired (~15 min TTL) token
as the password, so the server answers
`unable to accept connection, access denied`. The Reconnect button
(ADR-0036 stopgap) recovers this with a manual click, but the near-term
rollout needs several DSQL clusters connected **24/7 unattended** for
continuous data collection (project memory "Aurora DSQL permanent
connection required", 2026-07-13). A human is not present to click
Reconnect. 段階A therefore does not meet the goal on its own; this ADR is
the 段階B follow-up ADR-0036 deferred.

Two constraints shape the mechanism:

- **sqlx 0.8 has no per-connection password callback.** The
  `PoolConnector` trait that would let a live pool re-sign each new
  physical connection is a sqlx 0.9 feature, and 0.9 is unreleased. The
  workspace is pinned to `sqlx = "0.8"` (0.8.6 resolved). So a running
  `PgPool` cannot be told to use a fresh password for its next dial.
- **No AWS SDK** (ADR-0034): the SDK's token minting pulls in `aws-lc-rs`,
  which the workspace forbids. Token minting stays on the hand-rolled
  `dsql_auth` SigV4 path from ADR-0036.

### Decision

Keep the token fresh by **rebuilding and atomically swapping the whole
`PgPool` on a timer**, from a background task the adapter owns. New
physical connections are always dialled by the *current* pool, whose
token is never older than one refresh interval — well inside the TTL.

- **Swappable pool handle.** `PostgresAdapter`'s `pool` field becomes a
  small `PoolHandle` enum: `Static(PgPool)` for every existing flavor
  (unchanged behaviour, no task, no lock) and `Refreshing(Arc<RwLock<PgPool>>)`
  for `aurora-dsql-iam`. Every adapter method takes
  `let pool = self.pool.current();` (a cheap `PgPool` clone — `PgPool` is
  `Arc` inside) and uses `&pool`, so `ping` / `query` / `describe_table`
  change at exactly one line each and no query logic moves. The read lock
  is held only long enough to clone the `Arc`, never across an `.await`.
- **Background refresh task.** `connect_aurora_dsql_iam` builds the first
  pool as today, wraps it in `Arc<RwLock<PgPool>>`, and spawns a Tokio
  task that loops: sleep `refresh_interval`, mint a fresh token from the
  retained `AuroraDsqlIamParams`, build a new `PgPool`, and swap it into
  the lock. The task holds a **`Weak`** to the lock, so when the adapter
  is dropped (process exit or a connection switch under ADR-0020) the last
  `Arc` goes and the task's next `upgrade()` returns `None` and it exits —
  no explicit shutdown channel, no task leak across a switch.
- **Refresh cadence is derived, not magic.** A pure
  `refresh_interval(expires_secs) -> Duration` returns two-thirds of the
  token TTL (600 s for the 900 s `DEFAULT_EXPIRES_SECS`). At any instant
  the live pool's token age is 0–600 s, leaving ≥ 300 s of validity for a
  fresh dial. The function is the unit-tested seam: it is strictly greater
  than zero and strictly less than the TTL for every sane input, which is
  the invariant that keeps a dial from ever racing expiry.
- **Old pool drains, it is not killed.** Swapping overwrites the `Arc<…>`
  the lock holds; an in-flight query that already cloned the previous
  `PgPool` finishes on it, and the old pool closes when its last clone
  drops. A best-effort `old.close().await` after a short grace runs in the
  same task so idle sockets do not linger. Because the collector issues
  one statement at a time, the swap is effectively invisible.
- **Credential source and role are unchanged from 段階A** (maintainer
  decision, 2026-07-14): the token is signed from the **static AWS access
  key / secret key** already stored inline (`access_key_id`) and in the OS
  keychain (`secret_key`) — no `~/.aws` profile or SSO source — and it is a
  **`DbConnectAdmin`** token for the `admin` role. 段階B changes only the
  refresh lifecycle; the `AuroraDsqlIamParams` shape, the
  `connections.toml` schema, and the keychain reference are byte-identical
  to ADR-0036, so no config migration and no setup-pack (#9) change.

### Consequences

- **24/7 unattended operation works**: a new dial after any idle period
  uses a token minted ≤ 10 minutes ago, so the `access denied` recycle
  failure cannot occur. The Reconnect button stays as a manual override
  for the unexpected (e.g. rotated credentials) but is no longer required
  for normal operation.
- **The secret key now lives in memory for the adapter's whole lifetime**,
  inside the refresh task (it must re-sign forever), rather than only
  during a single connect. It is still never logged and never in `Debug`;
  the `AuroraDsqlIamParams` retained by the task carries the same redaction
  posture as 段階A. This is an accepted, documented trade of a longer
  in-memory secret lifetime for unattended correctness.
- **Brief connection churn every ~10 minutes**: the pool is rebuilt on
  each refresh even when idle. For a one-statement-at-a-time collector this
  is negligible; a busier workload would notice the periodic reconnect, and
  a future optimisation could refresh lazily (only when a dial is imminent)
  — out of scope here.
- **`Static` flavors are untouched**: Postgres/Neon/Supabase/`aurora-dsql`
  keep a plain `PgPool` with no lock and no task; the only cost is the
  one-line `self.pool.current()` indirection, which is a move plus an `Arc`
  clone.
- **Web sibling**: desktop-only connection-lifecycle concern, no HTTP
  wire-contract change, so dbboard-web is unaffected and no cross-repo
  brief is needed (same posture as ADR-0036).

### SemVer impact (ADR-0011)

None to any published contract. Internal only: `PostgresAdapter` gains a
private `PoolHandle` field shape and a background task; the public
constructor signatures are unchanged. `dbboard-core` is untouched.

## ADR-0038 — Passphrase-encrypted connection bundle export/import

**Status:** Accepted 2026-07-16

### Context

`connections.toml` is deliberately portable-but-incomplete: it stores
only keyring *references* (`keyring_token_ref`, `keyring_url_ref`,
`keyring_secret_key_ref`), never secret material (ADR-0013). The secrets
themselves live in the local OS keychain. That split is right for the
file's normal life (safe to back up, sync, paste into a bug report), but
it means the TOML alone is **useless on another machine** — the keychain
entries it points at do not exist there.

Moving a whole connection set to another machine is exactly the near-term
need. The collector handoff (#14, project memory "Windows internal
distribution") today requires handing over the exe, a template TOML, and
then seeding three secrets by hand with `cmdkey` on the target machine
(`docs/collector-setup/README.md`), with the real secrets delivered over
a separate secure channel. That is fiddly and error-prone.

We want a single self-contained artifact that carries the connection
metadata **and** its secrets, protected so it can travel over an ordinary
channel, opened with a passphrase delivered out-of-band.

### Decision

Add a **connection bundle**: a `.dbbx` file that is an `age`
passphrase-encrypted blob whose plaintext is a JSON `BundlePayload`:

```jsonc
{
  "version": 1,                 // bundle schema version (BUNDLE_VERSION)
  "connections": { ... },       // a full ConnectionFile (refs only)
  "secrets": {                  // keyring_ref -> secret material
    "dbboard.store-a.token": "…",
    "dbboard.store-c.url":   "…"
  }
}
```

**Crypto: the `age` crate, passphrase (scrypt) mode.** age gives a
battle-tested authenticated envelope — scrypt KDF + `ChaCha20-Poly1305`
AEAD + a versioned file format — in one dependency, so dbboard hand-rolls
no cryptography. `default-features = false` drops the optional
`armor`/`async`/`plugin`/`ssh` surface; the bundle is a binary blob
written straight to a user-chosen path. The alternative — a hand-rolled
`argon2id` + `XChaCha20-Poly1305` envelope on the RustCrypto primitives
the tree already pulls transitively — was rejected: it is more code and a
larger crypto-review surface for no user-visible benefit over age's
vetted format.

**Layering.** The crypto core (`encrypt_bundle` / `decrypt_bundle` over
`BundlePayload`) lives in `dbboard-config::bundle`. The orchestration that
resolves every keyring reference on export and seeds the keychain on
import — tying the `ConnectionFile` and the `SecretStore` to the payload —
lives alongside it in `dbboard-config`. `dbboard-ui` only adds the menu
items, the passphrase dialog, the `rfd` file dialog, and the result
surfacing; no business logic in the UI layer (per CLAUDE.md Architecture).

**Import conflict policy: skip-and-report.** On import, an entry whose
`id` already exists in the live store is **not** overwritten; the import
proceeds for the rest and reports the skipped ids. This is the safe
default: importing onto a fresh machine (the handoff case) has no
conflicts, and importing onto a populated machine never silently mutates
an existing connection's secret. Overwrite/merge modes are a later
refinement if needed.

**Export scope v1: all connections.** The first cut bundles the entire
`connections.toml` plus every secret it references. A "pick which
connections" UI is deferred; the handoff use case wants everything.

**Passphrase policy.** Export refuses a passphrase shorter than
`MIN_PASSPHRASE_LEN` (8) — a floor against an empty/accidental
passphrase, not a strength meter. Decrypt imposes no minimum so a bundle
made elsewhere still opens.

**Memory hygiene.** The JSON plaintext (which briefly holds every secret
in the clear) is `zeroize`d after the age boundary on both export and
import. age already zeroizes its own `SecretString` key material. The
plaintext is never written to disk unencrypted.

### Consequences

- **The collector handoff collapses to two items**: the exe and one
  `.dbbx` file, with the passphrase spoken/messaged over a separate
  channel. No manual `cmdkey` seeding, no per-secret side channel. The
  `docs/collector-setup/` flow gains an "import a bundle" path.
- **Bundle security reduces to passphrase strength + passphrase
  channel.** The `.dbbx` is safe at rest and in transit (authenticated
  AEAD; tampering is detected as corruption, a wrong passphrase is
  detected distinctly). Anyone with both the file and the passphrase has
  every secret — the same trust boundary as handing over the secrets
  directly, but now in one step.
- **Dependency footprint grows by `age` (+ `zeroize` promoted to a direct
  dep).** age pulls `curve25519-dalek` / `x25519-dalek` for its X25519
  recipient path even though only the scrypt path is used; all pure Rust,
  MIT/Apache-2.0, no system OpenSSL, so ADR-0034's TLS constraints are
  untouched. The workspace `unsafe_code = "forbid"` still applies to
  dbboard's own crates; dependency-internal `unsafe` (curve25519 field
  arithmetic) is unaffected, as with every other crate we vendor.
- **A decrypt cannot always tell a wrong passphrase from a corrupted key
  stanza** — age reports both as the same AEAD failure. The bundle layer
  resolves that ambiguity toward "incorrect passphrase" (the action the
  user should try first) and reserves "corrupt" for structural failures
  and tampered payload bodies.
- **Web sibling**: desktop-only feature, no HTTP wire-contract change, so
  dbboard-web is unaffected and no cross-repo brief is needed (same
  posture as ADR-0036/0037).

### SemVer impact (ADR-0011)

None to any published contract. Internal only: `dbboard-config` gains a
`bundle` module (`BundlePayload`, `encrypt_bundle`, `decrypt_bundle`,
`validate_passphrase`, `BundleError`) and two new direct dependencies
(`age`, `zeroize`). `dbboard-core` is untouched.

### Implementation hardening (2026-07-16)

Two hardenings surfaced in review of the import path and are now part of
the accepted design:

- **Reference-collision refusal.** A keyring reference is
  `dbboard.<id>.<field>`, derived from the connection id. A crafted
  bundle could carry a *new* id whose secret ref nonetheless points at an
  *existing* connection's keychain slot (e.g. new id `attacker` with
  `keyring_url_ref = "dbboard.victim.url"`), which the seed step would
  write — overwriting the victim's secret even though skip-and-report
  protects the victim's *entry*. The importer now collects every ref
  already claimed by a live entry and **skips (reports) any incoming
  entry whose ref collides**, across all kind variants including
  hand-authored `AuroraDsqlIam`. Id-conflict skip and ref-conflict skip
  are both reported through `ImportReport`.
- **Decrypted-secret scrubbing.** `BundlePayload` zeroizes its `secrets`
  values on `Drop`, and the import seed loop zeroizes its cloned
  `secret_writes` buffer on both the error-return and success paths, so
  resolved secret material does not linger past the keychain write. This
  complements the plaintext-JSON zeroize already specified under Memory
  hygiene.

## ADR-0040 — Startup update check against the GitHub Releases API

**Status:** Accepted 2026-07-16

### Context

dbboard now ships as a hand-delivered `dbboard.exe` to internal testers
and collector operators (ADR-0032, project memory "Windows internal
distribution"). There is no installer, no package manager, and no
auto-update channel: once someone has a copy, nothing tells them a newer
build exists. In practice a maintainer cuts a new exe, and the people
holding the old one keep running it because they have no signal to
re-download.

The ask is narrow: when a newer version is published, the app should let
the user *know*, show them *what changed*, and let them decide whether to
update. Explicitly **not** in scope: forced upgrades, in-app download, or
silently replacing the running binary. The exe is unsigned and delivered
by hand; automatic self-replacement would be both hard to do safely and
contrary to the "the human moves the bits" posture of the whole handoff.

A tension has to be named. The tester guide promises "nothing here needs
the internet except the database connections themselves." An update check
is, by definition, a network call the app makes on its own behalf. That
promise has to be reconciled, not ignored.

### Decision

On startup, fire a single best-effort GET against the GitHub Releases API
for the public repo's **latest** release, compare its tag against this
binary's own `CARGO_PKG_VERSION`, and surface a notice in the Help menu
only when the published version is strictly newer.

- **Detection basis: GitHub Releases API.**
  `GET https://api.github.com/repos/meta-taro/dbboard/releases/latest`
  returns `tag_name`, `body`, and `html_url`. GitHub excludes drafts and
  pre-releases from this route, so a 200 is always a real published
  version. `tag_name` (e.g. `v0.2.0`) drives the comparison; `body` is the
  changelog; `html_url` is where "get the new version" points. No API
  token — the endpoint is public and the unauthenticated rate limit is
  irrelevant for a once-per-launch call.

- **Comparison is pure and total.** Tags are normalised (a leading `v`
  stripped, pre-release/build metadata dropped) and parsed into
  `major.minor.patch`; an update is offered only when the latest tuple is
  strictly greater. Anything unparseable on either side yields "no
  update" — a malformed tag must never manufacture a phantom notice. This
  logic lives in a pure `is_newer` / `classify` pair and is unit-tested
  without any network I/O.

- **Updating stays fully manual.** The notice names the new version, links
  to its release page, and offers the release notes as a collapsible,
  **selectable (copyable)** changelog — matching the copyable-error
  convention (ADR-0039). There is deliberately no download-and-install
  button.

- **Non-blocking, silent on failure.** The check runs as a task on the
  existing server runtime (`apps/dbboard` clones a `tokio::runtime::Handle`
  before the eframe closure, since `rt` must stay in `main` to drive
  `server.shutdown()`). The UI thread never blocks. Every failure —
  offline, HTTP error, rate-limited, malformed JSON — folds to a logged,
  swallowed `Failed` state that renders **nothing**. A failed or offline
  check is indistinguishable from "up to date"; the feature informs, it
  never nags and never errors.

- **Opt-out honours the privacy promise.** Setting
  `DBBOARD_NO_UPDATE_CHECK` to any non-empty value skips the request
  entirely — the state stays `Idle` and no network call is made. This is
  the reconciliation of the tester guide's "no network but the databases"
  wording: the one outbound call the app makes on its own behalf is
  documented, best-effort, and switchable off. `README.md` documents this
  and the opt-out env var; the tester guide's "no network but the
  databases" line must be reconciled to name this call in the doc-sync that
  lands once `docs/internal-testing.md` reaches `develop` (it is on a
  parallel branch at time of writing).

### Layering

The comparison logic, the fetch, and the shared state type live in a
self-contained `apps/dbboard/src/update_check.rs`. The binary is already
the wiring layer that owns cross-cutting startup concerns (locale, clock,
CJK fonts, server bootstrap); a once-per-launch update probe belongs with
them. The result flows to the UI as an `Arc<Mutex<UpdateState>>` the Help
menu reads each frame — the same shared-slot pattern the connection and AI
switchers already use (ADR-0020 / ADR-0025). `dbboard-core` and the
adapters are untouched; this is desktop-only and web-neutral (the web
sibling has its own deploy channel), so no cross-repo brief is needed.

### Consequences

- One new outbound network dependency (`api.github.com`), off by a single
  env var, silent when unreachable. `reqwest` + `serde` become direct
  dependencies of `apps/dbboard` (both were already transitive via
  `dbboard-ui`), naming the binary's own network use explicitly.
- New i18n keys `help-update-available` / `help-update-link` /
  `help-update-notes` in `en` + `ja` (other locales fall back to `en`).
- The Help menu gains a version-aware row without changing the existing
  version line (`about_line`) or its test.
- Release hygiene now matters: the notice is only as good as the tags. A
  published release must carry a clean `vMAJOR.MINOR.PATCH` tag and useful
  notes for the changelog to read well.
