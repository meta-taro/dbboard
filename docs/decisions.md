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

## ADR-0039 — Unified error display: localized message + original English, both copyable

**Status:** Accepted 2026-07-16

### Context

The app surfaces errors from several layers — `DbError` (adapters),
`ConfigError` / `SecretError` / `BundleError` (connection store),
`AiSettingsError` (AI-provider store), and `AiError` (AI panel). Until now
these reached the UI in two inconsistent shapes:

- `DbError` and `AiError` were rendered through small local
  `error_display` / `ai_error_display` helpers that translated only the
  category *prefix* and kept the body verbatim (ADR-0009 / ADR-0015 /
  ADR-0023 Decision 8).
- Everything from `dbboard-config` was rendered by calling `.to_string()`
  on the error — i.e. the raw English `thiserror` `Display`. A collector
  running the Japanese UI hit a wall of English (the screenshot that
  prompted this: `config secret failed: no secret stored for reference:
  dbboard.<id>.url`). The message was also a plain `ui.colored_label`, so
  it could be neither selected nor copied.

The maintainer asked for one rule across every app-side error: (1) show it
in the active locale, (2) show the original English *alongside* the
translation, and (3) make both **copyable** — selectable text plus a copy
button — so a non-technical user can paste the English into a web search
or an AI assistant. dbboard is a learning/reference project and this is a
cheap, high-value affordance in the AI era.

Scope boundary: **SQL / DB engine error bodies are not translated.** They
originate at the connection target, not in dbboard, so only their category
prefix is localized; the body stays verbatim (unchanged from ADR-0009 /
ADR-0015). The same holds for provider-returned `AiError` bodies.

### Decision

Introduce a single presentation-layer primitive and render path in
`dbboard-ui::errors`:

- **`DisplayError { localized, original }`** — a value carrying both
  halves. `new(localized, original)` for errors that travelled up from a
  lower layer (original = the error's own English `Display`); `plain(text)`
  for UI-side validations with no lower-layer origin (e.g. "passphrases do
  not match"), where the two halves are identical so only one line renders
  and the clipboard is not duplicated.
- **Per-taxonomy producers** — `config_error_display`,
  `ai_settings_error_display`, `db_error_display`, `ai_error_display` —
  each maps its error enum to a Fluent-localized `localized` half and sets
  `original = err.to_string()`. `SecretError` and `BundleError` get shared
  helpers because both the connection and AI stores wrap `SecretError`.
- **`render_error(ui, Option<&DisplayError>)`** — the single inline
  renderer: a Copy button (copies both halves joined by a newline, or just
  the one line for a `plain` error) beside the localized message in red,
  with the original English on a dimmed second line *only when it differs*.
  Both lines are `egui::Label … .selectable(true)` so Ctrl+C works without
  the button too.

The localized half comes from Fluent (`t!` / `t_args!`); new keys were
added to `en` (source of truth) and `ja` only — the other nine locales
fall back to English per the Tier-2 backlog convention (ADR-0015), and
there is no locale-parity test to break.

`dbboard-config` stays **i18n-free**: its `thiserror` `Display` remains
English (it is also the log/`Debug` representation), and translation
happens entirely at the UI boundary. This keeps the domain/config layers
free of presentation concerns (per CLAUDE.md Architecture) and gives the
"original English" half for free.

### Consequences

- Every app-side error now renders identically: Japanese (or fallback
  English) + original English + copyable. The `ConnectionsView` /
  `AiSettingsView` `last_error` fields and the `AiPanel` `last_response`
  error arm changed from `String` to `DisplayError`; the three local
  `render_error` / `*_display` helpers in `lib.rs`, `connections.rs`,
  `ai_settings.rs`, and `ai.rs` were removed in favour of the shared
  module.
- The in-process connection-switch error (`switch_error_message`) already
  embeds the English `DbError` body inline, so it is wrapped as
  `DisplayError::plain` at the render site rather than re-split — it is
  copyable but shows a single line.
- Adding a new error variant now means adding one Fluent key (en + ja) and
  one match arm in the relevant producer; forgetting the key degrades
  visibly (Fluent echoes the key) and is caught by the
  `*_localized_half_resolves_a_real_key` tests.
- Desktop-only, in-process. No HTTP contract change and no web mirror
  (the taxonomies do not cross the desktop ↔ web boundary).

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

## ADR-0041 — Light / Dark / Auto theme with persisted preference

- **Status**: Accepted 2026-07-17
- **Tracks**: issue 0014

### Context

The app shipped a single visual theme (egui's default dark). Light/dark
switching is a baseline expectation, and an **Auto** mode that follows the
OS setting is the modern default. The maintainer asked for all three, with
the choice remembered across restarts.

Two facts shaped the design:

1. egui already models exactly this. `egui::ThemePreference` has
   `Dark` / `Light` / `System`, and `Context::set_theme` applies it —
   `System` makes egui track the OS light/dark preference and update live
   when the user flips it. So the app does not hand-roll OS detection or
   `Visuals` swapping; it maps its own preference onto egui's and lets
   egui do the work.
2. The runtime **language** switcher (ADR-0022) is deliberately *not*
   persisted — it resolves from env/OS at startup and swaps in memory. The
   theme, by contrast, must persist, so it needs a small on-disk settings
   file. There was no general "app settings" store yet; the two existing
   stores (`connections.toml`, `ai-providers.toml`) are domain-specific.

### Decision

- Add a **`ui-settings.toml`** file under the same `ProjectDirs` config
  dir as the other stores, owned by a new `dbboard-config::ui_settings`
  module. It mirrors the existing store shape: a `version` field, TOML
  serde, atomic sibling-`*.tmp`-then-rename writes via `secure_fs`.
- Model the choice as `ThemePreference { Light, Dark, Auto }` (default
  **Auto**). The binary maps it onto `egui::ThemePreference`
  (`Auto → System`) and calls `ctx.set_theme` at startup and whenever the
  user picks a new value from a new **Theme** menu.
- **Loading is non-fatal.** Unlike the connection store, a missing,
  malformed, or version-incompatible `ui-settings.toml` never errors — it
  falls back to the default in memory (logged), because UI chrome must not
  be able to block startup. The next save rewrites the file cleanly.
- Persist on change only (a menu pick), best-effort: a failed write is
  logged, the in-memory choice still applies for the session.

### Consequences

- First general per-user UI-preferences file; future UI prefs (e.g. a
  persisted language, grid density) have an obvious home and pattern.
- Auto correctness rides on egui: `System` tracks the OS and repaints on
  change, so there is no separate OS-theme polling to maintain.
- Desktop-only / in-process. No HTTP contract change, no `history.jsonl`
  change, no `dbboard-web` mirror.
- Custom colours introduced later (e.g. the dirty-cell tint in issue 0013)
  must read from the active `Visuals`, not hard-coded RGB, so they hold up
  in both themes.

## ADR-0042 — Inline cell editing: the first write-back path

- **Status**: Accepted 2026-07-17
- **Tracks**: issue 0013
- **Builds on**: ADR-0028 (`describe_table` supplies columns + primary key)

### Context

Every path in dbboard so far **reads**. The maintainer wants HeidiSQL-style
in-place editing: double-click a result cell to edit it, blur to *stage*
the change (仮登録) with a faint dirty tint, and press a **Save** button
below the grid to commit. Nothing touches the database before Save.

Introducing write-back forces three decisions that outlive the UI:

1. **How a row is identified** for a safe `WHERE`. A blind `UPDATE`
   without a unique key can rewrite many rows.
2. **How the `UPDATE` reaches the database.** The entire stack is
   SQL-string-only: `DatabaseAdapter::query(&self, sql: &str)`, the HTTP
   contract, and the UI's `Command::Query(String)` carry **no bound
   parameters**. Adding a parameterised path would change the adapter
   trait *and the HTTP wire contract* — a cross-repo change requiring a
   `dbboard-web` brief and every adapter to reimplement.
3. **Where the SQL is built.** CLAUDE.md forbids business logic in egui
   event handlers.

### Decision

This ADR is **slice a: the pure write-back core** (SQL generation + dirty
model), fully unit-tested, no UI and no contract change. The egui wiring
(double-click editor, tint, Save button, dialect/PK plumbing to the UI) is
**slice b**, a separate PR that builds on this.

- **Contained, literal-SQL path — no new adapter method, no wire change.**
  Write-back reuses the existing `query(sql)` execution. The `UPDATE` is
  built as a complete SQL string in a new pure module,
  **`dbboard-core::write_back`** (core is "no I/O", and string generation
  is pure — it sits next to the adapter contract per CLAUDE.md). This
  keeps the first write path **desktop-only / in-process**: no HTTP
  contract change, no `dbboard-web` mirror. A typed/parameterised path is
  explicitly deferred (see Alternatives) and can replace the internals
  later without changing the UI.
- **Injection safety by construction.** Identifiers are emitted
  double-quoted with any embedded `"` doubled (`"user""s"`) — identical
  for SQLite and Postgres. Values are emitted as **single-quoted string
  literals with `'` doubled**, or the bare keyword `NULL`. No user text is
  ever concatenated unescaped.
- **Type fidelity via engine coercion, not UI-side parsing.** The editor
  works on text, so every non-null value is written as a quoted string
  literal and the engine coerces it by the target column's type/affinity:
  `SET n = '123'` lands an integer, `SET b = 'true'` a boolean, `SET d =
  '2026-01-01'` a date, on both SQLite (type affinity) and Postgres
  (assignment cast from an `unknown` literal). This dodges lossy UI-side
  type parsing. **NULL is the one value that is not text** and gets an
  explicit affordance (a distinct staged state emitting the `NULL`
  keyword), never "empty string".
- **Row identity is adapter-specific** (mirrors the issue's coverage
  table). A `RowIdentity` is required to edit; without one the cell never
  enters edit mode:
  - **Declared primary key** (any family): key the `WHERE` on the PK
    columns from `describe_table`.
  - **SQLite family** (Turso/libSQL, D1) with no declared PK: use the
    implicit **`rowid`** — *except* `WITHOUT ROWID` tables, which have no
    rowid and are refused.
  - **Postgres family** (Supabase, Neon, Aurora DSQL) with no PK/unique
    key: **refuse** (`ctid` is not stable, so there is no safe implicit
    key).
- **Concurrency: PK-only `WHERE` + report rows-affected** (the simplest
  safe default). Save confirms the `UPDATE` matched exactly one row; a
  count of 0 or >1 is surfaced as an error and leaves the edit staged.
  Optimistic "WHERE also matches the original values" is deferred.
- **Object kind gates editability.** Only a plain `SELECT` from a single
  base **table** is editable. **Views, materialised views, joins,
  computed/multi-table, and CTE/derived results are read-only** — no
  updatable-view support in this ADR (SQLite needs `INSTEAD OF` triggers;
  Postgres only auto-updates simple views). Editability is decided in the
  pure core from the resolved target; the UI only offers editing when the
  core says the target is updatable.
- **Failure handling.** A Save error uses the unified copyable error
  display (ADR-0039) and leaves every edit **staged** (not dropped) so the
  user can retry. Staged edits are revertible (per-cell and discard-all)
  before Save.

### Slice-a surface (`dbboard-core::write_back`, pure)

- `enum SqlDialect { Sqlite, Postgres }` — drives schema qualification
  (Postgres qualifies `"schema"."table"`; SQLite does not) and which
  implicit identity is allowed.
- `enum RowIdentity { PrimaryKey(Vec<String>), SqliteRowid }` and a
  resolver `RowIdentity::resolve(schema: &TableSchema, dialect,
  without_rowid: bool) -> Option<RowIdentity>` returning `None` (=refuse)
  per the rules above.
- `enum CellValue { Null, Text(String) }` — a staged new value.
- `enum RowKey { Columns(Vec<(String, Value)>), Rowid(i64) }` — the
  concrete `WHERE` key for one row: named identity columns paired with the
  row's *original* typed `Value`s, or a SQLite `rowid`. (`RowIdentity`
  above is the *capability*; `RowKey` is the *filled-in* key the UI builds
  from the selected row.)
- `struct UpdatePlan { table, key: RowKey, edits: Vec<(String, CellValue)> }`
  and `build_update_sql(&UpdatePlan, dialect) -> Result<String,
  WriteBackError>` producing the fully-escaped `UPDATE … SET … WHERE …`.
  Identity values encode by their real type (bare number / quoted text /
  `IS NULL`); edited values are always quoted string literals coerced by
  the engine.
- `enum WriteBackError { NoEdits, EmptyKey, UnsupportedKeyType(String) }`
  for the refusable cases (nothing edited, an unkeyed update, or a blob
  identity value that has no safe literal form).

### Alternatives considered

- **Parameterised execute path** (bind values, `?`/`$n`). Safer typing and
  the "proper" long-term design, but changes the adapter trait *and the
  HTTP wire contract*, dragging in a `dbboard-web` coordination brief and
  every adapter. Rejected for the first cut in favour of the contained
  literal-SQL path; the pure core hides SQL construction so this can be
  swapped in later behind the same `UpdatePlan` without touching the UI.
- **`WHERE` on all original column values** (no PK needed). Fragile and
  ambiguous on duplicate rows; can update multiple rows. Rejected — hence
  the refuse-without-identity rule.
- **UI-side type parsing** (decide int/bool/date before building SQL).
  Lossy and dialect-specific; engine coercion of a quoted literal is
  simpler and more faithful.

### Consequences

- First mutation path in the app, but contained: **desktop-only /
  in-process, no HTTP contract change, no `history.jsonl` change, no
  `dbboard-web` mirror.** If slice b later adds a parameterised wire path,
  *that* would need a cross-repo brief.
- The dirty-cell tint (slice b) reads from the active egui `Visuals`
  (ADR-0041) so it holds up in both themes.
- Editing is deliberately narrow (single-table `SELECT`, real identity,
  tables-not-views). Widening — updatable views, composite/unique-key
  fallback, optimistic concurrency — is future ADR work.

## ADR-0043 — Render the update notice's release notes as Markdown

- **Status**: Accepted 2026-07-17
- **Builds on**: ADR-0040 (the startup update check surfaces the notice)

### Context

ADR-0040's update notice shows the newer release's notes under a "変更点"
collapsible in the Help menu. The notes are the **GitHub release body**,
which is authored in CommonMark (`## headings`, `**bold**`, `` `code` ``,
`- bullets`, `[links](url)`). The notice rendered them with a plain
`egui::Label`, so a tester saw literal `**dbboard**` and raw `[text](url)`
markup instead of formatted text — noise exactly where a release summary
should be scannable.

egui has no built-in Markdown renderer. Two ways to fix it: adopt the
ecosystem-standard `egui_commonmark`, or hand-roll a small renderer for the
subset we author.

### Decision

Adopt **`egui_commonmark` 0.23** (the egui-0.34-compatible release) and
render the notes with `CommonMarkViewer`. A `CommonMarkCache` lives on
`DesktopApp` so an open menu re-uses parsed output instead of re-parsing
every frame.

- **`default-features = false, features = ["pulldown_cmark"]`.** The notes
  are short, text-only Markdown, so the image loaders, SVG, syntax
  highlighter, and network `fetch` features stay off. The resolved subtree
  is four crates — `egui_commonmark`, `egui_commonmark_backend`,
  `pulldown-cmark` (MIT), `unicase` (MIT/Apache) — and adds no advisory or
  license failure of its own (`cargo deny` traced clean through the new
  subtree; the pre-existing failures below are unrelated).
- **MSRV raised 1.75 → 1.92.** egui_commonmark 0.23 requires rustc 1.92.
  dbboard is an internal, never-published binary built on current stable
  (1.95 at time of writing), so the declared floor was aspirational; moving
  it to the real requirement is honest and costs nothing.

### Alternatives considered

- **Hand-rolled subset renderer.** No dependency, MSRV unchanged, but only
  as correct as the cases we code. The release body is free-form GitHub
  Markdown; a battle-tested parser is the faithful choice and matches the
  "prefer libraries over hand-rolled" principle.
- **`comrak` backend.** A heavier GFM parser; pulldown-cmark covers the
  notes and keeps the subtree small.

### Consequences

- **Desktop-only / UI-only.** No HTTP contract change, no `dbboard-web`
  mirror. The notice text is still selectable (Ctrl+C into a report),
  preserving the ADR-0039 copyable affordance.
- **MSRV bump is a maintenance note, not a user-facing change.** No CI
  matrix pins the old floor; the git hooks build on the installed stable.
  It did unlock one MSRV-gated clippy lint (`duration_suboptimal_units`),
  fixed in the same change: a `dsql_auth` test now reads `from_mins(10)`
  instead of `from_secs(600)`.
- **Pre-existing `cargo deny` drift (unrelated to this ADR).** The RustSec
  DB has since flagged crates already in the tree: `proc-macro-error2`
  (unmaintained, via `age` → ADR-0038), `option-ext` (MPL-2.0, via
  `directories` → ADR-0013), and `quick-xml` (via `wayland-scanner` →
  `eframe`, Linux-only). Tracked separately; `cargo deny` is a manual/CI
  gate, not a commit hook, so it does not block this change.

## ADR-0044 — Real distributable installers + release CI with checksums

- **Status**: Accepted 2026-07-17
- **Builds on**: ADR-0032 (Windows MSI sources), ADR-0038 (secret handoff)

### Context

Distribution so far has been a hand-built, hand-carried `dbboard.exe`
(ADR-0032 hardened it to be self-contained, but it stayed a bare exe). For
an OSS project a bare, unsigned exe reads as untrustworthy: SmartScreen and
AV engines flag an "unknown publisher", and a first-time downloader has
nothing to verify the file against. Three gaps, in ascending order of trust
gained per unit of cost:

1. No **installer** — the exe is not a recognizable "install this app"
   artifact. (The MSI *sources* existed since ADR-0032 but had never been
   built.)
2. No **build provenance / checksums** — nothing ties a downloaded file to a
   public, reproducible build.
3. No **code signing** — the OS-level "unknown publisher" warning persists
   regardless of 1–2.

There was also no macOS artifact at all, though the code already compiles
and runs there (Windows-specific bits are `cfg(windows)`-gated; keyring uses
the `apple-native` Keychain backend).

### Decision

Ship the first two gaps now; defer signing (gap 3) as a paid follow-up.

1. **Make the MSI actually build.** The hand-authored `wix/main.wxs` used
   `AbsentDisallow="yes"`, which WiX v3's `candle` rejects (CNDL0004); the
   correct v3 spelling is `Absent="disallow"`. With that fixed, WiX Toolset
   v3.14 + `cargo-wix` 0.3.9 produce `dbboard-<version>-x86_64.msi` (version
   injected from Cargo via `$(var.Version)`). `cargo wix` must run from
   `apps/dbboard` so the linker resolves the `assets\` / `wix\` relative
   `SourceFile` paths against that CWD.
2. **macOS `.app` / `.dmg` via `cargo-bundle`.** A
   `[package.metadata.bundle]` block in `apps/dbboard/Cargo.toml` is the
   source of truth for the bundle identity (`identifier`
   `com.meta-taro.dbboard`, category, icon, min OS version). `cargo bundle
   --release` on a Mac produces the `.app`; the release CI wraps it in a
   compressed `.dmg` with `hdiutil`. This mirrors the Windows split —
   **sources in-tree, the artifact build is a separate native step** — since
   `.app`/`.dmg` cannot be produced (or later signed/notarized) from
   Windows.
3. **Release CI with checksums** (`.github/workflows/release.yml`). A
   `v*.*.*` tag push builds the Windows (exe + MSI) and macOS (.dmg)
   artifacts on their native runners and publishes them to the matching
   GitHub Release alongside a combined `SHA256SUMS.txt`. `workflow_dispatch`
   runs the same build as a smoke test without publishing. Checksums are the
   cheapest strong trust signal — anyone can verify a download against the
   value CI computed.

### Alternatives considered

- **`cargo-dist` (unify everything).** One tool for multi-platform build +
  installers + checksums + CI. Rejected for now: it would replace the
  working `cargo-wix` MSI path (just fixed) and impose its own release
  orchestration — large churn against a project in low-churn, menu-not-
  sequence mode. Revisit if the piecemeal setup grows unwieldy.
- **Third-party release action** (e.g. `softprops/action-gh-release`).
  Rejected to keep the supply-chain surface minimal: the publish step uses
  the runner-bundled `gh` CLI with the built-in `GITHUB_TOKEN`. Only
  first-party `actions/checkout` + `actions/*-artifact` are used, pinned by
  major tag.

### Consequences

- **Not signed → OS warnings remain.** Windows artifacts trip SmartScreen;
  the macOS `.app` trips Gatekeeper. Signing needs paid certs (Authenticode
  / Apple Developer ID) + repo secrets; the workflow leaves commented
  placeholder steps (`codesign` / `notarytool` / `stapler`) marking where
  they slot in. Tracked as the ADR-0044 §Future item.
- **`cargo-bundle` is lightly maintained.** Accepted for a small, stable
  metadata surface; if it rots, the escape hatch is a hand-written `.app` +
  `create-dmg`, or `cargo-packager`. The in-tree metadata (identifier,
  category, min OS) is tool-agnostic and would port.
- **CI is groundwork, not yet proven green.** It was authored on Windows and
  cannot be executed locally; the first tag push (or a `workflow_dispatch`
  smoke run) is expected to shake out runner-specific issues (WiX via choco,
  `cargo-bundle` output path). This is the intended first live test.
- **Least-privilege security posture.** A pre-merge security review of the
  workflow hardened three points: (1) the workflow defaults to
  `contents: read` and `contents: write` is re-granted **only** to the
  `publish` job — the build jobs run untrusted crates.io `build.rs`/proc-macro
  code via `cargo build`/`cargo install`, so they must never hold a
  write-scoped token, and their `actions/checkout` sets
  `persist-credentials: false`; (2) the publish guard is
  `github.event_name == 'push' && github.ref_type == 'tag'`, not `ref_type`
  alone — a manual `workflow_dispatch` aimed at an existing tag would
  otherwise fall through to the `--clobber` upload and silently overwrite a
  released tag's checksummed assets; (3) the asset copy is `cp -n` plus a
  file-count check so a future cross-platform filename collision fails loudly
  instead of dropping a binary. No secrets beyond the built-in token; no
  third-party release action.
- **Icon is 256px.** Enough to ship; a 1024px source would sharpen the
  largest Retina slot (`TODO(icon-1024)`).
- **Desktop-only / no `dbboard-web` mirror.** Packaging and CI are build
  concerns with no HTTP contract surface.

## ADR-0045 — Local column/table annotations (dbboard-side, no DB write)

- **Status**: Accepted 2026-07-17
- **Builds on**: ADR-0028 (`describe_table` full schema), ADR-0031 (Structure
  tab), ADR-0025 (per-user `*.toml` store pattern), ADR-0038 (`.dbbx` — for
  the boundary this ADR deliberately does *not* cross)

### Context

An operator reading an unfamiliar table wants to record what a column *means*
("`status`: 0=pending 1=paid 2=void", "`amt`: minor units, JPY"). The obvious
home for such notes is a database-native column comment, but the primary
targets can't provide one uniformly:

- **SQLite / libSQL (Turso) / Cloudflare D1** have **no first-class comment
  concept** — no `COMMENT ON COLUMN`, no `pg_description`-style catalog, and no
  extension adds one. The single native trick is embedding `-- …` / `/* … */`
  inside the `CREATE TABLE` DDL, which SQLite preserves verbatim in
  `sqlite_master.sql`; but that is unstructured (self-parse the DDL), fragile
  (other tools recreating the table drop it), and **requires write
  permission** — a non-starter for a read-only collector connection, and D1
  constrains DDL further.
- **Postgres (Neon / Supabase / Aurora DSQL)** *does* have first-class
  `COMMENT ON` + `pg_description`, but dbboard's `describe_table` currently
  reads only `information_schema.columns`, so even existing DB comments aren't
  surfaced today.

This asymmetry means a DB-native approach can't serve the actual fleet
(D1 + aurora-dsql + supabase) uniformly, and would demand write access the
operator often lacks. The notes are also *documentation*, not schema — losing
them to someone else's `ALTER TABLE` is unacceptable.

### Decision

Store annotations **on the dbboard side**, in a per-user file, and surface them
as an editable column in the existing Structure tab. Nothing is written to any
database.

1. **Storage — `annotations.toml`.** A new per-user file in the same config
   dir as `connections.toml` / `ai-providers.toml` / `history.jsonl`, resolved
   via the same `ProjectDirs::from("dev", "dbboard", "dbboard")` lookup.
   Written atomically through `secure_fs` (0o600 on Unix, user-only DACL on
   Windows) exactly like `ai_store::save_atomic`. A new
   `crates/dbboard-config/src/annotations.rs` module mirrors the `ai_store` /
   `ai_settings` split: a versioned file type (`version` field, `load_or_empty`
   treats a missing file as empty, forward-compatible parse) plus an admin API
   (`set_table_note` / `set_column_note` for writes — an empty/whitespace
   string clears and prunes the entry, so there is no separate `clear` call —
   and `table_note` / `column_note` for reads) with
   rollback-on-save-failure. Persistence + value types live in
   **`dbboard-config`** (the persistence layer), not `dbboard-core` (which
   stays I/O-free), consistent with `ai_settings`.

2. **Key granularity — table + column.** Keyed `connection id → table → note`,
   where the table key is schema-qualified where the engine has schemas
   (`public.orders`) and the bare name where it doesn't (SQLite/libSQL/D1) —
   reusing `TableInfo`'s qualification. Each table entry carries an optional
   table-level note plus a `column name → note` map. Connection **id** (stable,
   from `connections.toml`) is the anchor, not the display name, so renaming a
   connection keeps its notes.

3. **UI — a "Note" column in the Structure tab.** Extend `render_table_schema`
   (currently ordinal / name / type / nullable / PK / default) with a seventh
   editable **Note** column; clicking a cell opens an inline text field,
   committing on focus-loss/Enter persists via the admin API. This makes the
   Structure render path `&mut self` (or routes the edit through the existing
   worker message/`Reply` pattern like `edit.rs`) since it now mutates state —
   a deliberate, contained change from today's read-only `&self` render. New
   i18n keys (`structure-col-note`, edit hint) added to all locales.

4. **No DB write / read-only safe.** The whole point: annotations require no DB
   privilege, work on a read-only connection, and never touch the wire — so
   they're valid for every adapter including D1 and IAM-scoped aurora-dsql.

### Alternatives considered

- **DB-native comments** (`COMMENT ON`, or DDL comments in `sqlite_master`).
  Rejected as the *primary* store: not uniform across the fleet, fragile on
  SQLite, and write-requiring (§Context).
- **Surfacing Postgres `pg_description`.** Real value, but **out of scope
  here** — it's a *DB-derived* read that belongs in `describe_table`
  (adapter + core change) and would be shown as a separate, read-only "DB
  comment" lane alongside local notes. Deferred to its own ADR so this feature
  stays a focused, uniform, write-free local store and its value can be proven
  on the SQLite-family connections first.
- **Bundling annotations into the `.dbbx` export for cross-machine sharing.**
  Rejected for the first release, and specifically *not* into `.dbbx`. `.dbbx`
  (ADR-0038) is an **encrypted, passphrase-gated secret bundle** for connection
  handoff; annotations are **non-secret documentation**. Merging them mismatches
  intent (a note edit would demand a passphrase; a secret bundle would carry
  docs). If sharing becomes a real need, it should be a **separate plain-text
  annotations export/import** (no passphrase, no secrets), leaving `.dbbx` for
  secrets only. Deferred.

### Consequences

- **New persistent format** (`annotations.toml`, versioned) — additive, lazily
  created, a missing/old file degrades to "no notes". TDD: config module lands
  with parse/roundtrip/save-atomic/version tests first, mirroring `ai_store`.
- **Structure render becomes mutating.** The Structure tab's render path gains
  `&mut self` / a message hop; contained to that tab, no effect on the
  read-only result grid.
- **Notes are per-machine** until the deferred plain-text export ships. On a
  single collector laptop this is fine; the ADR names the escape hatch.
- **`pg_description` stays invisible** until its own ADR — accepted so this
  slice is uniform across all adapters and unblocked by any adapter work.
- **Desktop-only / no `dbboard-web` mirror / no HTTP contract change.** Purely
  local persistence and UI.
- **Ships alongside the AI-provider live test** per the maintainer's wish to
  release both together; the two are independent (this is code, that is a test
  activity) and neither blocks the other.

## ADR-0046 — `dbboard-mcp`: expose dbboard as a read-only MCP server

- **Status**: Proposed 2026-07-21
- **Builds on**: ADR-0023 (AI provider layer — this *inverts* its direction),
  ADR-0028 (`describe_table` full schema), ADR-0029 (function-calling primitive —
  the tool surface it foresaw, exposed outward instead of inward), ADR-0013
  (`connections.toml`), ADR-0025 (per-user `*.toml` store + keyring),
  ADR-0037 (Aurora DSQL IAM token refresh), ADR-0045 (local annotations),
  ADR-0009 (`dbboard-server` in-process backend — source of the connection
  factory this ADR extracts)

### Context

dbboard's AI layer (ADR-0023..0029) makes dbboard the *caller*: the app embeds
an Anthropic provider and asks it to explain/suggest SQL. The maintainer wants
the **inverse** — an external AI agent (Claude Desktop / Claude Code) that can
*operate dbboard*: browse the configured databases, read schema, run read
queries, read the local annotations.

Why route this through dbboard rather than a generic database MCP server:

1. **dbboard already owns the hard parts.** Connection definitions
   (`connections.toml`), OS-keyring secrets (Windows Credential Manager here),
   and a validated adapter per engine — Cloudflare D1 (HTTP REST), Aurora DSQL
   (IAM token + background refresh, ADR-0037), Supabase / Neon / Postgres (sqlx),
   Turso / libSQL (file/remote). An agent driving `dbboard-mcp` names a
   **connection id**; it never sees a raw DSN, password, or IAM credential.
2. **The primitives already exist.** `DatabaseAdapter::{list_tables,
   describe_table, query}` (ADR-0028) and `annotations.toml` (ADR-0045) map
   almost one-to-one onto MCP tools. `describe_table` was explicitly built as
   "the natural first tool for a database AI companion" (ADR-0029 §Context);
   this ADR is where that tool surface finally lands.
3. **The connection factory already exists and is proven.** `dbboard-server`
   exposes `backend_config_for_entry(entry, secrets)` → `build_adapter(config)
   -> Arc<dyn DatabaseAdapter>`, matching on `BackendConfig::{Turso, D1,
   Postgres, Neon, Supabase, AuroraDsql}` with `ping()` validation.
   `apps/dbboard` (`DesktopSwitcher`) already consumes exactly this pair.

But three facts about the *existing* code make a naive implementation unsafe or
broken, and shape the decisions below:

- **The Postgres adapter runs the simple query protocol.**
  `PostgresAdapter::query` uses `sqlx::raw_sql(sql).fetch_many(&pool)`, which
  executes *multiple semicolon-separated statements sequentially*. So
  `SELECT 1; DROP TABLE t;` is **not** a parse error — both run. A
  `starts_with("SELECT")` guard is therefore a data-loss vulnerability on
  Neon/Supabase/Aurora DSQL — the exact connections the unattended collector
  depends on. Postgres also allows DML inside CTEs
  (`WITH x AS (DELETE ... RETURNING *) SELECT ...`, starts with `WITH`),
  `SELECT ... FOR UPDATE`, `nextval()`/`setval()`, `EXPLAIN ANALYZE <dml>`,
  `CALL proc()`. String matching cannot be trusted.
- **Open-per-request is actively wrong for two adapters.** Turso `:memory:` is a
  *fresh empty database on every connect*; Aurora DSQL spawns a **background
  token-refresh task inside the adapter** (ADR-0037 段階B) that keeps a pool
  authenticated 24/7. Reopening per tool call gives the agent a blank DB
  (Turso) and throws away the refresh task + pays full SigV4/TLS/`ping()` each
  call (DSQL).
- **The reusable factory lives in `dbboard-server`, which pulls in `axum` +
  `TcpListener`.** Depending on it from a headless stdio binary would compile an
  HTTP server into the MCP process for no reason and couple two apps.

### Decision

Add a **standalone headless stdio MCP server binary**, `dbboard-mcp`, that an
MCP client spawns. It reuses `dbboard-config` (connections + annotations +
keyring) and a newly-extracted connection factory to serve a **read-only** tool
surface over stdio. No GUI, no loopback socket, no new persistence.

1. **Extract `crates/dbboard-connect` (app-layer library, no `axum`).** Move
   `BackendConfig`, `backend_config_for_entry` / `entry_to_backend`, and
   `connect_adapter` / `build_adapter` out of `dbboard-server` into a lean crate
   that depends only on `dbboard-core` + the adapter crates + `dbboard-config`.
   `dbboard-server` re-exports from it (HTTP contract unchanged); `dbboard-mcp`
   depends on `dbboard-connect` + `dbboard-config` only. One source of truth for
   security-sensitive connection construction across GUI, server, and MCP; no
   axum weight in the stdio binary. A single new `dbboard-mcp` crate alone is
   **not** enough — the shared factory prevents a maintenance fork of
   credential-handling code. Layer rules hold: `dbboard-connect` sits at the
   wiring layer, depends on core/adapters/config, never on ui/server.

2. **SDK — `rmcp` 2.2.0** (official Rust MCP SDK, released 2026-07-08). Features
   `macros` (`#[tool]` / `#[tool_router]`) + `transport-io` (stdio). Server is a
   `ServerHandler` struct launched via `serve_server(handler, stdio())`. Pin the
   exact version in `Cargo.lock`; add a **compile-smoke integration test** so an
   SDK bump can't silently change the tool-registration shape. New dependency →
   security review + `cargo deny` (downloads/maintenance/license) before merge,
   per CLAUDE.md; this ADR entry is the required decision record.

3. **stdout is the transport — hard invariants.** In stdio transport the JSON-RPC
   stream owns stdout; one stray byte corrupts the session.
   - ALL logging/diagnostics to **stderr** only (`tracing_subscriber::fmt()
     .with_writer(std::io::stderr)`); route or silence sqlx's default `Info`
     query log. A test asserts no tool path writes stdout.
   - **Do NOT copy** `windows_subsystem = "windows"` from `apps/dbboard`
     (main.rs:39). The MCP binary must be a **console-subsystem** app or the
     stdio pipes won't attach. Round-trip a framed message in an integration
     test to catch any Windows CRLF text-mode translation.

4. **One multi-thread tokio runtime; keep blocking calls off it.** A single
   `#[tokio::main]` runtime hosts both the rmcp serve loop and all adapter I/O —
   no nested runtime, no `block_on`-in-async, so `apps/dbboard`'s cross-runtime
   `build_adapter_on` dance is unnecessary here. `keyring` reads (Windows
   Credential Manager RPC) and config `std::fs` reads are **synchronous
   blocking**; wrap them in `tokio::task::spawn_blocking` (or resolve at
   first-use behind the cache in Decision 6) so they never stall an executor
   worker under concurrent tool calls.

5. **v1 tool surface — read-only (5 tools):**

   | Tool | Params | Returns |
   |---|---|---|
   | `list_connections` | — | `[{id, name, kind, capabilities, read_only:true}]` — sourced from `ConnectionFile`; **secrets (`keyring_*_ref`) never serialized** (guarded by the existing store.rs redaction test) |
   | `list_tables` | `connection_id` | `Vec<TableInfo> {schema?, name}` |
   | `describe_table` | `connection_id, schema?, table` | `TableSchema {columns:[{name, declared_type, nullable, primary_key, ordinal, default_value}], primary_key}`; adapters whose default returns `DbError::Capability` surface a clean tool error keyed off the `capabilities` flag |
   | `run_read_query` | `connection_id, sql, max_rows?` | `{columns:[{name,type}], rows, row_count, truncated:bool}` |
   | `get_annotations` | `connection_id, table?, column?` | table/column notes via `AnnotationsAdmin` |

   **Row cap truncates, does not error.** The workspace cap
   `MAX_RESULT_ROWS = 10_000` (dbboard-core/limits.rs) *errors* — hostile to an
   agent whose broad `SELECT *` would just fail. `run_read_query` gets its own
   smaller default (e.g. 200–1000), enforced as a real engine-level `LIMIT`
   (inside the read-only transaction of Decision 6, not a naive
   `SELECT (...) LIMIT n` wrap), returning `truncated:true` instead of erroring.
   No cursor exists in the codebase; document offset/limit guidance in the tool
   description rather than building pagination for v1.

6. **Read-only enforced by the engine, not by string matching (resolves the
   Postgres hazard).** Add a read-only execution path to the adapter contract —
   `async fn query_read_only(&self, sql, max_rows) -> DbResult<QueryResult>`
   (default impl = classify-then-`query`) — so each engine enforces it its own
   way:
   - **Postgres family (Neon/Supabase/DSQL):** execute inside a server-side
     `BEGIN READ ONLY; SET LOCAL statement_timeout = '<n>s'; <sql>; ROLLBACK`.
     `READ ONLY` makes the *server* reject INSERT/UPDATE/DELETE/DDL/`nextval`/
     writing `FOR UPDATE`, defeating CTE-DML and multi-statement writes together;
     the `statement_timeout` doubles as the cancellation backstop (Decision 8).
   - **libSQL / Turso (SQLite):** `PRAGMA query_only = ON` on the connection
     before serving (engine-enforced, rejects all writes); open read-only where
     the builder allows.
   - **Cloudflare D1 (HTTP REST):** *no server-side read-only mode exists* — the
     weakest link. Classify with a real parser (`sqlparser`, correct dialect):
     reject anything that is not a single `SELECT`/`WITH ... SELECT`/`EXPLAIN`-of-
     select, reject multi-statement, walk the AST to reject DML-in-CTE. The ADR
     labels D1 explicitly as **"classified, not engine-enforced."**

   The **pure classifier** `is_single_read_only_statement(sql, dialect)` lives in
   `dbboard-core` (no I/O, unit-testable, shareable with the web sibling); the
   per-engine enforcement lives in each adapter's `query_read_only`. **Prefix /
   `starts_with` checks are banned.** The v1 read tools never call the bare
   `query()`.

7. **Per-`connection_id` lazy adapter cache — never open-per-request.** A
   process-lived `Arc<Mutex<HashMap<String, Arc<dyn DatabaseAdapter>>>>` built on
   first use via the Decision 1 factory, mirroring what `AppState` does for the
   GUI's single adapter, generalized to N. Required for correctness: Turso
   `:memory:` (fresh empty DB per open) and DSQL (keep the refresh task warm).
   Adapters are `Send + Sync` and hold their own pools, so caching is safe;
   DSQL should not be idle-evicted.

8. **Config discovery + cancellation.** Resolve `connections.toml` via the same
   `ProjectDirs::from("dev","dbboard","dbboard")` lookup as the GUI (NOT cwd),
   plus an explicit `--config` / `DBBOARD_CONFIG` override (settable in
   `claude_desktop_config.json`'s `env` block, since Claude Desktop's spawn env
   has none of the `DBBOARD_*` vars). **Log the resolved config path + connection
   count to stderr at startup** so a handoff bug is diagnosable; carry over the
   ADR-0024 cloud-sync-path warning. Cancellation (`notifications/cancelled`)
   drops the tool future, but a dropped future only cancels at await points — the
   server-side `statement_timeout` (Postgres), `reqwest` client timeout (D1), and
   libSQL query timeout are the real backstops so an abandoned query can't pin a
   pooled connection.

### Out of scope (v1)

- **Any write tool** (SQL writes, schema DDL). Deferred behind a future
  per-connection opt-in gate (its own ADR).
- **`set_annotation` write tool.** Candidate (annotations are a dbboard-local
  file write, not a DB write, so it does not break the read-only posture) but
  deferred: `annotations.toml` is read-modify-write last-writer-wins, and the GUI
  owns the same file — concurrent edits can silently drop a note. `save_atomic`
  prevents *corruption*, not *lost updates*. Gating it behind the same opt-in as
  future writes keeps the "read-only v1" posture crisp.
- **GUI-embedded "attach to the live session" HTTP/SSE mode** — the staged v2.
- **Resources / prompts / sampling** MCP surfaces — tools only for v1.
- **Localised tool descriptions** — English, agent-facing.
- **`dbboard-web` mirror** — desktop-only; no HTTP contract change.

### Consequences

- **Two new crates/bins + one new dependency.** `crates/dbboard-connect`
  (extraction, `dbboard-server` re-exports through it) and `dbboard-mcp`
  (the binary). `rmcp` gets a security review + `cargo deny` pass before merge.
- **Reuses the proven, `ping()`-validated factory**, so the agent gets the same
  connection fidelity the GUI does — DSQL IAM refresh and D1 HTTP included.
- **Read-only by engine enforcement** keeps the unattended-collector safety bar:
  even pointed at the live Aurora DSQL connection, an agent cannot mutate data,
  and the Postgres multi-statement / CTE-DML hazards are closed at the server,
  not by fragile string matching.
- **Adapter contract grows one method** (`query_read_only`, defaulted) — additive,
  pre-existing adapters compile unchanged. `dbboard-core` gains a pure,
  well-tested SQL classifier the web sibling can adopt.
- **Concurrency**: sqlx `PgPool` and the D1 `reqwest` client are concurrency-safe
  under the shared cache; a single libSQL handle may head-of-line-block — accept
  for v1, note it, add a per-connection semaphore if it bites.
- **Windows footguns carried forward**: the known benign libSQL teardown
  segfault (project memory) now surfaces as an "abnormal child exit" the MCP
  client logs on *every* shutdown — mitigate with an explicit stdout flush +
  `std::process::exit(0)` on a clean shutdown request. The new unsigned
  `dbboard-mcp.exe` is another binary Norton may flag — note it in the
  internal-distribution docs.
- **TDD plan** (next session): tests first — (1) `is_single_read_only_statement`
  against a table of adversarial inputs (`SELECT 1; DROP TABLE t`, `WITH x AS
  (DELETE...) SELECT`, `SELECT ... FOR UPDATE`, `PRAGMA`, leading comments,
  `EXPLAIN <dml>`); (2) `list_connections` redacts secrets; (3) stdout stays
  clean; (4) each engine's `query_read_only` rejects a write inside a read-only
  txn/pragma; (5) a temp-libSQL round-trip of `list_tables` / `describe_table` /
  `run_read_query` with truncation. Then implement: extract `dbboard-connect`,
  add `query_read_only` + classifier, build the bin tool-by-tool.

## ADR-0047 — Download page on GitHub Pages

- **Status**: Accepted 2026-07-22
- **Builds on**: ADR-0044 (release CI + checksummed artifacts — the assets
  this page links to), ADR-0040 (in-app update check that already points at a
  "download page")

### Context

After ADR-0044 the release CI publishes checksummed Windows (exe + MSI) and
macOS (.dmg) artifacts to each GitHub Release, and ADR-0040's in-app update
notice links users to "the download page". But there was no such page — the
link went to the raw GitHub Releases list, which buries the current binaries
under changelog prose, prior-version assets, and source-tarball noise. A
first-time downloader has no clean "get dbboard" landing spot, and no
in-context nudge to verify the checksum before running an unsigned binary.

GitHub Pages is free for public repositories, so a purpose-built download
page costs nothing to host.

### Decision

Ship a single static download page at `site/index.html`, deployed to GitHub
Pages, and point ADR-0040's in-app link and the README at it.

1. **Data-driven, not hand-maintained.** The page is static HTML/CSS/JS with
   no build step and no framework. At load it calls the public GitHub
   Releases API (`/repos/meta-taro/dbboard/releases/latest`) and renders the
   current version, per-platform download buttons, and the
   `SHA256SUMS.txt` link **client-side**. So the page content tracks releases
   automatically — cutting a new release needs no page edit and no redeploy.
2. **Deploy via first-party Actions** (`.github/workflows/pages.yml`):
   `actions/configure-pages` + `upload-pages-artifact` + `deploy-pages`,
   pinned by major tag. The workflow runs on push to `develop` (the
   integration branch; `main` is release-tag-only) under `site/**` (or the
   workflow itself) plus `workflow_dispatch`. Because the content is fetched
   at runtime, the deploy branch does not change what visitors see. Least
   privilege: read-only by default, `pages: write` + `id-token: write`
   granted to the deploy job only — matching the ADR-0044 posture. No
   third-party action.
3. **Verification + honesty up front.** The page carries the `sha256sum -c`
   / `Get-FileHash` commands and an explicit unsigned-binary caveat
   (SmartScreen / Gatekeeper), so the trust story from ADR-0044 travels with
   the download instead of living only in the README. A page-level CSP
   (`script-src 'self'`, `connect-src` limited to the GitHub API) is set via
   a meta tag — GitHub Pages can't send response headers — as defense in
   depth; the page logic lives in a same-origin `app.js` (not inline) so an
   injected inline script cannot execute.

### Alternatives considered

- **Static, hard-coded version links.** Simpler (no JS, works offline), but
  every release would need a page edit + redeploy PR — exactly the manual
  toil the update-check flow was meant to avoid. Rejected: the API call is
  cheap and degrades gracefully.
- **Deploy from a branch / `/docs` folder** instead of the Actions pipeline.
  Rejected to keep one consistent deploy mechanism and least-privilege token
  scoping; the first-party Pages actions are the maintained path.
- **A full marketing site / static-site generator.** Over-scoped for a
  learning/reference project; a single page is the whole need.

### Consequences

- **Runtime dependency on the GitHub API.** If the unauthenticated call fails
  (offline, or the ~60/hr per-IP rate limit), the page falls back to a direct
  link to the Releases page rather than showing a broken state. The dynamic
  parts are built via DOM APIs (not `innerHTML`) and download URLs are
  restricted to GitHub hosts, so an unexpected API payload cannot inject
  markup or an off-site link.
- **One-time enable is a human step.** Pages must be switched on in repo
  Settings → Pages → Source: "GitHub Actions"; the first deploy is triggered
  with `workflow_dispatch`. The published URL is
  `https://meta-taro.github.io/dbboard/`.
- **The unsigned-binary caveat is now front-and-center**, which is the honest
  state until code signing (ADR-0044 §Future) lands.

## ADR-0048 — Client-side multi-column sort of the result grid

- **Status**: Accepted 2026-07-22
- **Builds on**: ADR-0035 (result-grid selection + export — the grid this
  sorts), issue 0013 (inline editing — whose row indices this must not break)

### Context

The result grid rendered rows in the exact order the adapter returned them,
with no way to sort. For a serverless/distributed DB client that is a real
gap: re-sorting by re-issuing `SELECT ... ORDER BY` costs a round trip (and
isn't possible at all for an arbitrary already-run query), yet users routinely
want to eyeball a result by one column, then break ties by another. The ask
was an ordinary spreadsheet-style sort, up to a primary/secondary/tertiary
key.

### Decision

Sort **client-side, in the presentation layer, as a display-only
reordering** — the fetched rows are never mutated or re-queried.

1. **Ordering logic lives in `dbboard-core::sort`**, not the UI. A pure
   `sorted_row_order(rows, keys) -> Vec<usize>` returns a *stable permutation*
   of row indices; `compare_values` imposes a total order over `Value`
   (NULLs first, then numbers by magnitude, then text, then blobs), using
   `f64::total_cmp` so it never panics. Keeping this out of the UI honors the
   architecture rule (no business logic in event handlers) and makes the
   ordering unit-testable without egui.
2. **Sort reorders display, not data.** The grid renders through the
   permutation: the on-screen position maps to an actual `result.rows` index,
   and selection + inline editing continue to key on that actual index. So
   sorting can never corrupt a staged edit's row/primary-key mapping — the
   reason a permutation was chosen over sorting the row vector in place.
3. **Up to three levels, built by clicking headers.** A plain header click
   sorts by that column alone, cycling ascending → descending → off. A
   Ctrl/Shift-click appends the column as the next level (capped at three) or
   cycles an existing level's own direction. The header shows a ▲/▼ arrow and,
   once more than one column sorts, a 1-based level number. The stable sort
   makes the row's natural order the implicit final tiebreak.
4. **The permutation is cached** on the view state and recomputed only when
   the keys change or the row count no longer matches, so a shown grid isn't
   re-sorted every frame. A fresh query result resets the sort (its columns
   may differ entirely).

### Alternatives considered

- **`ORDER BY` round-trips.** Rejected: costs a query per sort, can't sort a
  result whose statement the user typed by hand, and loses the local grid
  state (selection, staged edits).
- **Sort the `Vec<Row>` in place.** Simpler to render, but it invalidates the
  row indices that selection and inline editing depend on, and would force
  re-deriving primary-key mappings after every click. The index permutation
  sidesteps all of that.
- **Full SQL `NULLS FIRST/LAST` + collation fidelity.** Over-scoped; the grid
  needs a predictable, panic-free total order, not engine-exact semantics.
  Documented as a fixed, simple order instead.

### Consequences

- Sorting is instantaneous and offline — no query, no network — and composes
  with the existing selection/export/edit paths unchanged.
- The order is dbboard's own total order, which may differ from what the
  database's `ORDER BY` (with its collation and NULL placement) would produce.
  This is intentional and documented on `compare_values`.
- Very large result sets pay an `O(n log n)` sort when the keys change; it's
  cached between frames, and the grid is already row-capped
  (`MAX_RESULT_ROWS`), so the cost is bounded.

## ADR-0049 — Local logical dump: schema + data, dump-only

- **Status**: Accepted 2026-07-22
- **Builds on**: ADR-0028 (`describe_table` — the introspection this extends
  for full DDL), ADR-0042 (write-back — its dialect-aware identifier/value
  quoting is the seam this reuses), ADR-0035 (result export — the pure,
  I/O-free serialization pattern this copies), ADR-0036 / ADR-0037 (Aurora
  DSQL over the Postgres adapter — the constraint that shapes Decision 6)

### Context

dbboard can export a *result set* (CSV/TSV, ADR-0035) and a *connection
bundle* (`.dbbx`, ADR-0038), but it cannot back up a whole database. The
internal collector runs three connections (Cloudflare D1, Aurora DSQL,
Supabase) on a handed-out Windows exe, and none of those engines offers a
one-click desktop equivalent of `pg_dump` / `sqlite3 .dump`: D1 is HTTP-only,
DSQL is IAM-gated Postgres, Supabase is pooled Postgres. A portable,
self-contained `.sql` backup of a connection is a real operational need.

dbboard already has the three pieces required to build this without new
infrastructure: in-process adapter access snapshotted for background work
(`SchemaSource`, ADR-0028 slice c), a pure serialization precedent
(`export.rs`), and dialect-aware quoting (`write_back.rs`, ADR-0042).

### Decision

Produce a **logical dump** — schema plus data — as one `.sql` text file per
connection, in the **source engine's SQL dialect**. This is **dump-only**;
restore/import is deliberately deferred to a future ADR.

1. **Pure serialization lives in `dbboard-core::dump`** (Value→SQL-literal and
   `INSERT` assembly), unit-tested with no adapter, UI, or I/O — mirroring
   `export.rs`. It reuses `write_back`'s `quote_ident` / `quote_str` (promoted
   to `pub(crate)`) and its `SqlDialect`, so escaping has one implementation
   across the write-back and dump paths.
2. **Value literals are total and dialect-aware.** `NULL`→`NULL`; integers and
   finite reals emit bare (reals via Rust's shortest round-tripping form);
   text is single-quote-escaped; blobs render as `X'…'` (SQLite) or
   `'\x…'::bytea` (Postgres). Non-finite reals — which real data almost never
   yields, since SQLite stores NaN as NULL and the Postgres adapter returns
   values as text — are still handled without panicking (`'NaN'`/`'Infinity'`
   casts on Postgres; NULL / `9e999` on SQLite).
3. **DDL is produced by the adapter, not core**, via a new optional trait
   method `table_ddl(&TableInfo)` gated by `Capabilities::has_table_ddl`,
   defaulting to a `Capability` error — the same evolution shape as
   `describe_table` (ADR-0028), so every existing adapter keeps compiling.
   Engine-specific catalog knowledge stays in the adapter layer.
4. **SQLite-family adapters (D1, Turso) get verbatim DDL cheaply** from
   `sqlite_master.sql` (table plus its `type='index'` rows). No
   reconstruction, so the dump reproduces the exact declared schema.
5. **Postgres-family adapters (Supabase, DSQL) reconstruct DDL from the
   catalog**: columns/types/`NOT NULL`/defaults/identity, primary key,
   unique + check constraints, indexes, foreign keys, and owned sequences,
   assembled in dependency-safe order. The pure assembler is split out so it
   is unit-testable without a live server.
6. **Aurora DSQL degrades by construction.** DSQL has no foreign keys and a
   restricted DDL surface (no sequences/`SERIAL`, no `ALTER … ADD
   CONSTRAINT`). The FK/sequence catalog queries simply return empty on DSQL,
   so those sections are omitted and the emitted DDL faithfully describes what
   DSQL actually holds. The dump makes **no promise of re-importability** into
   DSQL — acceptable because restore is out of scope (Decision 0).
7. **Data is complete for every engine**, read with keyset pagination on the
   primary key (`WHERE pk > $last ORDER BY pk LIMIT <page>`), falling back to
   `rowid`/`ctid`/`OFFSET` only for PK-less tables (documented cost). Page
   size stays below `MAX_RESULT_ROWS` so the per-query cap never trips, and
   each page is rendered straight to the file sink rather than buffered whole.
8. **Huge-DB guard is warn-and-allow.** A preflight `COUNT(*)` per table sums
   to the progress total; above a threshold (constant
   `DEFAULT_BACKUP_WARN_ROWS = 500_000` for now, promotable to a persisted
   setting later) the UI warns with the row count and lets the user proceed or
   cancel. Never a hard block.
9. **Orchestration runs in the worker thread, in-process (never HTTP)**,
   reusing the `SchemaSource`-style injected adapter snapshot and a
   `CancellationToken` (the AI-streaming pattern). Progress and completion
   surface as new `Reply` variants; the egui thread never blocks and the run
   is cancelable.
10. **Partial failure is non-fatal.** A table that errors mid-dump is recorded
    as a SQL comment in the file and collected into a per-table error list on
    the terminal reply (mirroring `SchemaPrefetched`'s `errors`); the run
    continues with the remaining tables.

### Scope

- **First adapters**: the production trio — D1, Aurora DSQL, Supabase. Turso
  and Neon follow for free where the SQLite/Postgres paths already cover them.
- **v1 slices** (TDD, independently shippable): (a) core value→literal +
  `INSERT`; (b) dump plan + threshold; (c1) `table_ddl` trait + D1 verbatim
  DDL; (c2) Postgres/DSQL catalog reconstruction; (d) async orchestrator
  (paging, progress, cancel, partial failure); (e) worker command/reply + egui
  UI; (f) i18n (11 locales) + docs.

### Out of scope / limitations

- **Restore/import** — a future ADR.
- **Aurora DSQL**: no FKs, no sequences; emitted DDL is descriptive, not
  guaranteed re-importable (Decision 6).
- **Views, functions, triggers, grants, RLS policies** — not dumped in v1
  (tables + data only).
- **Blob fidelity** is literal-level (`X'…'` / `'\x…'`), not streamed; a very
  large blob column is the memory worst case and is bounded only by page size.

### Alternatives considered

- **Shell out to `pg_dump` / `sqlite3`.** Rejected: not present on the
  handed-out exe, no binary for D1 at all, and it would fork the trust model
  (external process handling credentials). In-process reuse of the adapter
  keeps secrets in the keyring and the dump on the same connection the user
  already trusts.
- **Typed reconstruction of Postgres values.** Unnecessary: the Postgres
  adapter's simple-query path already returns every cell as text, which is
  exactly what a single-quoted literal wants (the engine re-coerces on
  insert), the same trick write-back uses.
- **Hard block above the row threshold.** Rejected in favor of warn-and-allow
  (Decision 8): the collector may legitimately need a large dump, so the tool
  informs rather than forbids.

### Consequences

- One new core module, one new optional adapter method (two impls for v1: D1 +
  Postgres), a new worker command/reply pair, a save-dialog + progress-modal
  UI flow, and an 11-locale string set.
- The all-text Postgres value path makes dumps literal-faithful but
  type-agnostic on re-insert (engine coercion), consistent with write-back.
- Sibling `dbboard-web` parity: the `table_ddl` capability and the dump concept
  are recorded here; no code is shared.


## ADR-0050 — User-configurable backup warn threshold

- **Status**: Accepted 2026-07-23
- **Builds on**: ADR-0041 (`ui-settings.toml` — the persisted-preferences
  store this extends), ADR-0049 (logical dump — the feature whose
  `DEFAULT_BACKUP_WARN_ROWS = 500_000` constant this promotes to a setting)

### Context

ADR-0049 shipped the logical dump with a fixed large-database warn threshold
(`DEFAULT_BACKUP_WARN_ROWS = 500_000`), and its own text flagged promoting that
constant to a persisted setting as a follow-up. The threshold is a judgement
call — "how many rows is 'a lot'?" — that depends on the connection and the
operator's patience, so a single baked-in number is wrong for someone whose
routine dump is 800k rows (nagged every time) or 50k (never warned when they'd
want to be). The maintainer asked for it to be user-changeable from the app.

### Decision

Make the warn threshold a **persisted, user-editable setting**, reusing the
existing `ui-settings.toml` store (ADR-0041) rather than introducing a new one.

1. **Storage: one new optional field on `UiSettingsFile`** —
   `backup_warn_rows: Option<u64>`, `#[serde(default, skip_serializing_if =
   "Option::is_none")]`. No schema-version bump: a file written before this ADR
   has no key and reads back as `None`, and a theme-only save stays
   byte-identical (the field is omitted when unset). `None` means "not
   configured".
2. **The domain default stays single-sourced in `dbboard-core`.**
   `dbboard-config` has no dependency on `dbboard-core` and must not duplicate
   `DEFAULT_BACKUP_WARN_ROWS`. So `None` is resolved to the fallback at the app
   layer: `DesktopApp` seeds the editable value from the persisted `Option`,
   falling back to `DbboardApp::backup_warn_rows()` (which the inner app itself
   seeded from the core constant) — the binary never re-imports the constant.
3. **The core already took the threshold as a parameter.**
   `DumpPlan::exceeds_threshold(threshold)` needed no change; only the single
   UI read site swaps the constant for a per-app `backup_warn_rows` field,
   pushed in via `DbboardApp::set_backup_warn_rows`.
4. **UI: a `Backup` submenu beside `Theme`** in the menu bar, holding a numeric
   `DragValue` (floored at 1). A change applies to the inner app immediately
   (so a dump started the same frame uses the new value) and persists **the
   moment the value settles** — a keyboard edit commits (`changed()` while not
   mid-drag) or a drag is released (`drag_stopped()`). Guarding the write on
   `!dragged()` keeps a scrub from firing an atomic file write every frame,
   while deliberately *not* keying persistence off focus loss, so quitting
   immediately after an edit cannot drop it.
5. **Load-modify-save, never clobber.** Persisting any one preference now loads
   the whole `UiSettingsFile`, mutates the one field, and writes it back
   (`persist_ui_settings`). This fixes a latent footgun: `set_theme` previously
   saved `UiSettingsFile::with_theme(pref)`, a fresh struct that would have
   reset a sibling `backup_warn_rows` to its default on every theme change.

### Out of scope

- Per-connection thresholds — the setting is global, matching the single
  process-wide dump flow.
- Exposing the threshold over the HTTP contract — it is a desktop-chrome
  preference, like the theme, and lives only on the binary side.

### Consequences

- One optional TOML field, one new inner-app field + setter/getter, one menu
  submenu, and three new i18n keys across 11 locales.
- `UiSettingsFile::with_theme` is retained for tests but documented as
  *not* preserving siblings; production writes go through load-modify-save.
- Sibling `dbboard-web`: no parity impact — the threshold is a desktop UI
  preference, not part of the adapter or dump contract.

## ADR-0051 — Logical restore / import

- **Status**: Accepted 2026-07-23 (implementation in progress, landing in slices)
- **Builds on**: ADR-0049 (logical dump — the write-side this reverses; restore
  targets the same engines and consumes the `.sql` dump produces), ADR-0046
  (`read_only` — the sqlparser-based classifier whose parsing approach Layer 2
  reuses), ADR-0012 (the `DatabaseAdapter` trait + `Capabilities` extension
  model this adds two methods and two flags to)

### Context

ADR-0049 gave dbboard a one-way door: it can dump a connection to `.sql` but
cannot load one back. The maintainer runs collector databases whose recovery
story is "restore the dump", so the missing read-side is real operational
friction, not a completeness itch. Restore is also the natural next major
feature after the dump landed and its warn threshold became configurable
(ADR-0050).

Four scoping decisions were settled with the maintainer up front, because each
changes the shape of the work:

1. **Input = any `.sql`, not only dbboard's own dumps.** `pg_dump` and
   `sqlite3 .dump` output must import too. This forbids a parser that only
   understands the narrow shape dbboard emits, and drives the two-layer split
   below (a lexical splitter that never rejects, plus a best-effort classifier
   that downgrades on parse failure rather than refusing).
2. **Engine scope = the same engines dump supports** — Turso/libSQL and
   Cloudflare D1 (SQLite family), Neon and Supabase (Postgres family), and
   Aurora DSQL best-effort.
3. **Safety model = empty / new targets only.** Restore refuses to run against
   a connection that already has user tables, or demands an explicit typed
   confirmation. It never silently merges into or overwrites populated schemas.
   This keeps the first cut safe without building diff/merge/conflict handling.
4. **Threshold-setting first (ADR-0050), restore second.** Done — ADR-0050
   shipped the settings-persistence groundwork; this ADR is the follow-on.

### Decision

Mirror the dump pipeline's shape (a pure, I/O-free core in `dbboard-core`
driven by the `DatabaseAdapter` trait, with the app supplying the file source
and the progress/cancellation channel) for the read side, under
`crates/dbboard-core/src/restore/`.

1. **A two-layer statement pipeline.**
   - **Layer 1 — `split_statements` (this slice, landed).** A lexical,
     dialect-agnostic splitter that carves a script into statements, correctly
     ignoring `;` inside string literals, quoted identifiers (double-quote and
     backtick), dollar-quoted bodies, and line/block comments (nesting-aware).
     Backslash escapes are honoured *only* inside Postgres `E'…'` strings, to
     match `standard_conforming_strings` (the `pg_dump` default since PG 9.1,
     and SQLite always). It classifies nothing and rejects nothing — it only
     finds boundaries — so it is robust to any `.sql`.
   - **Layer 2 — sqlparser classification (later slice).** Each split statement
     is parsed with the dialect's grammar to label it (DDL / insert / other)
     and to drive ordering and safety checks. Crucially it **downgrades on
     parse failure**: a statement the grammar cannot parse is not dropped but
     passed through as an opaque "run as-is" statement, so a best-effort restore
     of hand-written or exotic SQL still executes. This reuses ADR-0046's
     parsing approach but inverts its stance — read_only *fails closed*, restore
     *degrades open*.

2. **Two additive `DatabaseAdapter` methods + two `Capabilities` flags**,
   following the ADR-0012 / ADR-0049 extension pattern (default impl returns
   `DbError::Capability`, so pre-existing adapters compile unchanged and miss at
   runtime, not build time):
   - `execute(&self, sql: &str) -> DbResult<u64>` — run one write/DDL statement,
     returning rows affected. Gated by `Capabilities::has_execute`.
   - `execute_in_transaction(&self, statements: &[String]) -> DbResult<()>` —
     run a batch atomically. Gated by `Capabilities::has_atomic_restore`.
   Both are *additive*; the dump-only adapters keep working, and each adapter
   opts in as its slice lands.

3. **Empty-target gate via `list_tables`.** Before running, restore calls the
   existing `list_tables`; a non-empty result blocks the run behind a typed
   confirmation (safety decision 3). No new introspection surface is needed.

4. **Per-engine transaction strategy.** Turso/libSQL and Postgres restore
   atomically (`execute_in_transaction`). D1 has no multi-statement transaction
   over its HTTP API, so it falls back to per-statement execution and reports
   which statement failed. DSQL is best-effort `Continue`-on-error, matching how
   dump degrades its DDL for DSQL.

5. **A `RestoreState` UI state machine** mirroring `BackupState`
   (Idle / Planning / Confirming / `Blocked { existing }` / Running / Done /
   Failed), with the `Blocked` variant carrying the existing-table list for the
   typed-confirmation dialog. The core stays I/O-free and testable with a fake
   adapter, exactly like `run_dump`.

Implementation lands in ordered TDD slices; this ADR is written at slice 1
(the splitter) and updated only by appended follow-on ADRs, never rewritten.

### Out of scope

- **Merge / diff / conflict resolution.** Restore targets empty schemas; loading
  into a populated database is explicitly refused rather than reconciled.
- **Cross-engine translation.** A Postgres dump is not rewritten to run on
  SQLite; restore runs a script against an engine of the matching family, same
  as dump produces one per dialect.
- **Selective / partial restore** (single-table, data-only, schema-only). The
  first cut is whole-script.

### Consequences

- A new `restore/` module sibling to `dump/`, and two additive methods on the
  adapter trait that every adapter may implement over successive slices.
- Restore accepts foreign `.sql` (pg_dump, sqlite3) because Layer 1 is lexical
  and Layer 2 degrades open — at the cost of not statically validating a script
  before running it; failures surface per-statement at execution time.
- Sibling `dbboard-web`: shares the *concept* (two-layer split, empty-target
  safety) but not code; if web grows a restore path, coordinate the adapter
  contract shape here.

## ADR-0052 — OpenAI (ChatGPT) provider

- **Status**: Accepted 2026-07-23
- **Builds on**: ADR-0023 (the `AiProvider` trait + `dbboard-anthropic` this
  mirrors), ADR-0025 (the `ai-providers.toml` store + settings admin this adds a
  second `AiProviderKind` variant to), ADR-0026 (the streaming `StreamEvent`
  surface this implements for a second wire protocol), ADR-0027 (the
  `(provider_id, model_id)` identity stamped on history)

### Context

The AI layer has shipped as a single-provider design since ADR-0023: one
`AiProvider` trait, one concrete crate (`dbboard-anthropic`), and an
`ai-providers.toml` store whose `kind` discriminator has had exactly one variant
(`anthropic`). ADR-0025 §Out-of-scope explicitly deferred `openai`/`ollama` to a
follow-up. This is that follow-up: the maintainer wants ChatGPT selectable
alongside Claude, which is the whole reason the provider layer was built as a
trait rather than a hard-coded client.

Adding a second provider exercises every seam the earlier ADRs designed for it —
the trait is already object-safe behind `Arc<dyn AiProvider>`, the store already
`serde(tag = "kind")`-dispatches, the switcher already rebuilds providers from a
`kind`. The only genuinely new surface is (a) a second wire protocol and (b) a
`kind` *selector* in the settings UI, which until now had nothing to choose
between.

### Decision

1. **New crate `dbboard-openai`**, sibling to `dbboard-anthropic`, depending on
   `dbboard-ai` only (same dependency rule as ADR-0023 Decision 1 — never on
   `dbboard-core` or `dbboard-ui` directly).

2. **Chat Completions API** (`POST /v1/chat/completions`), not the newer
   Responses API. Chat Completions is the stable, widely-compatible surface and
   maps one-to-one onto the existing explain/suggest shape: a `system` message
   plus a `user` message, and `usage.prompt_tokens` / `usage.completion_tokens`
   for the token meter. The Responses API's extra machinery buys nothing for
   two single-turn prompts.

3. **Full streaming parity.** The provider advertises
   `AiCapabilities { has_streaming: true }` and implements a real SSE parser,
   so ChatGPT streams token-by-token exactly like Claude. OpenAI's stream
   differs from Anthropic's: plain `data:` frames with no `event:` type, a
   `data: [DONE]` sentinel terminator, and `usage` delivered only when the
   request sets `stream_options.include_usage = true` (a final choices-empty
   frame). The parser normalizes all of this into the same `StreamEvent`
   sequence the UI already consumes.

4. **Default model `gpt-4o`** when the entry's `model` field is empty, mirroring
   Anthropic's `with_default_model`. Any model id typed into the settings form
   overrides it. `gpt-4o` is chosen for broad account availability; a newer
   model that 400s on an account without access would be a worse default.

5. **Auth via `Authorization: Bearer <key>`** (OpenAI's scheme) rather than
   Anthropic's `x-api-key` + `anthropic-version` headers. The key still lives
   only in the OS keyring, referenced by `keyring_api_key_ref`; it never appears
   in `Debug`, logs, or errors, and TLS stays pinned to rustls with `https_only`
   (localhost-exempt for wiremock tests) exactly as ADR-0023.

6. **`AiProviderKind::OpenAi { model, keyring_api_key_ref }`** added to
   `ai-providers.toml` — additive, same shape as `Anthropic`. The settings Add
   and Edit forms gain a kind selector (they previously hard-coded the single
   Anthropic variant); the reconcile/keyring-ref plumbing extends to the new
   variant.

### Out of scope

- **Function calling / tools.** `has_function_calling` stays `false` for both
  providers; that is a later, cross-provider concern.
- **Azure OpenAI, Ollama, and other OpenAI-compatible endpoints.** The
  `base_url` override exists (tests use it) but no UI surfaces it; a
  configurable endpoint is a separate decision.
- **Per-provider capability divergence in the UI.** Both providers stream and
  neither does tools, so the panel needs no capability-conditional rendering
  beyond what ADR-0026 already built.

### Consequences

- A second concrete provider crate proves the ADR-0023 trait boundary holds:
  the only files that learned OpenAI exists are the new crate, the `kind` enum
  and its drafts, the binary's `build_provider_for_kind`, and the settings UI's
  kind selector. The worker, panel, history, and switcher are untouched.
- Sibling `dbboard-web`: shares the *concept* (provider trait, `kind`-dispatched
  store) but not code. If web adds ChatGPT, keep the `kind` string (`openai`)
  and the keyring-ref shape aligned so an exported `ai-providers.toml` reads the
  same on both.

## ADR-0053 — `search_schema`: a sixth read-only MCP tool

- **Status**: Accepted 2026-07-24
- **Builds on / amends**: ADR-0046 (the read-only MCP server — this adds one
  tool to the surface its Decision 5 "fixed at five" fixed), ADR-0028
  (`describe_table` full schema — the introspection this composes), ADR-0045
  (local annotations — the notes an agent reaches for once search points it at
  a table)

### Context

`dbboard-mcp` shipped (ADR-0046) with five read-only tools. Driving it against
a real collector database surfaced a concrete N+1 friction: an agent asked
"which table holds the customer email?" has to `list_tables`, then
`describe_table` on *every* table, then scan each result — a dozen-plus
round-trips against an unfamiliar schema before it can write a single query.
"Which tables relate to orders?" is the same shape. The primitive an agent
wants — *find the tables and columns whose name matches X* — does not exist,
so it re-implements it badly, one describe call at a time, on every session.

ADR-0046 deliberately fixed the surface at five to keep the initial read-only
posture crisp. That posture is about **not writing** (no SQL writes, no DDL, no
annotation writes); it says nothing against *more introspection*. A name search
over tables and columns is strictly the same class of operation as
`list_tables` + `describe_table` — pure catalog reads — so it extends the
surface without touching the read-only boundary the ADR actually protects.

### Decision

Add a sixth tool, **`search_schema`**, alongside the existing five.

1. **Composed in the service layer, not the adapter.** `McpService::search_schema`
   iterates the existing `list_tables` + `describe_table` primitives and filters
   by a case-insensitive substring. No new method on the `DatabaseAdapter`
   contract, no per-engine code, no `dbboard-core` change — the adapter surface
   is untouched and every engine gets the tool for free. (A future engine-native
   `information_schema` fast path is a possible optimization, not a v1
   requirement.)

2. **Params `{ connection_id, pattern }`; returns matched tables with their
   matched columns.** For each table whose name matches, or which has ≥1 column
   whose name matches, the result carries the `TableInfo`, a `table_name_matched`
   flag, and the list of matched `ColumnInfo`. A table-name match with no column
   match returns an empty `matched_columns` (the flag tells the agent to
   `describe_table` for the full column list) — keeping the payload lean rather
   than echoing every column of every hit.

3. **Empty/whitespace `pattern` is rejected**, not treated as "match
   everything". A blank needle would substring-match every table and column and
   haul the whole catalog back through one tool call; that is what `list_tables`
   is for. The rejection is a clean `invalid_params` (new
   `ServiceError::InvalidRequest`), symmetric with an unknown `connection_id`.

4. **Still read-only, secrets still never serialized.** `search_schema` reads
   only catalog metadata through the same cached adapter as the other tools; it
   never touches the `query` path and never sees a keyring reference. The
   ADR-0046 invariants hold unchanged.

### Out of scope

- **Value search** (matching row *data*, not identifiers). That is a
  `run_read_query` job and would defeat the row-cap/read-only reconnaissance
  framing; `search_schema` is metadata-only.
- **Foreign-key / relationship discovery.** Genuinely valuable for an agent
  writing JOINs, but it needs a new adapter introspection method across every
  engine — its own slice, tracked separately.
- **Regex / glob patterns.** Case-insensitive substring covers the real
  "where is X" question; a richer matcher can follow if it is ever asked for.

### Consequences

- The MCP surface is now **six tools**; the "fixed at five" language in
  `server.rs` and the ADR-0046 table is superseded by this entry. The tool is
  additive — existing clients are unaffected, new clients discover it via
  `list_tools`.
- One N+1 exploration pattern collapses to a single call, the most common
  first thing an agent does against an unfamiliar collector database.
- Cost note: the v1 implementation still issues N `describe_table` calls under
  the hood (one per table). Acceptable for the collector databases in scope
  (dozens of tables); documented in the tool description so an agent on a
  thousand-table schema knows to narrow first. An engine-native
  `information_schema` path can replace the loop later without changing the
  tool's shape.
- Bounded output, like `run_read_query`: matches are capped at
  `MAX_SCHEMA_MATCHES` (200) and the result carries a `truncated` flag. A
  deliberately-broad pattern (`"id"`, `"a"`) cannot walk an unbounded catalog
  or return one giant blob — the search stops at the cap, and the early break
  bounds the `describe_table` calls too. A truncated result means "narrow the
  pattern", the same guidance the row cap gives.

## ADR-0054 — Foreign-key introspection and `list_relationships`: a seventh read-only MCP tool

- **Status**: Accepted 2026-07-24
- **Builds on / amends**: ADR-0053 (its "Out of scope" explicitly deferred
  foreign-key / relationship discovery to "its own slice, tracked separately" —
  this is that slice), ADR-0046 (the read-only MCP server — this adds the
  seventh tool), ADR-0012 (the capability model — this adds one adapter method
  and one capability flag), ADR-0028 (`describe_table` — the introspection this
  reuses to resolve implicit primary-key references)

### Context

An agent writing a JOIN needs to know how tables connect. `search_schema`
(ADR-0053) finds tables and columns *by name*, but a foreign key is structural,
not lexical: `orders.customer_id → customers.id` is invisible to a name search
unless the columns happen to share a substring. Today an agent guesses join
keys from naming conventions, or reads full DDL per table (`table_ddl`) and
parses the `REFERENCES` clauses itself — brittle, and impossible on engines
where the DDL is reconstructed rather than verbatim. The primitive an agent
wants — *what does this table reference, and what references it* — does not
exist on the adapter contract.

Unlike `search_schema`, this cannot be composed from existing primitives: the
foreign-key graph is not derivable from `list_tables` + `describe_table`, which
report columns and primary keys but not references. It needs a real
introspection call per engine.

### Decision

Add a foreign-key introspection primitive to the adapter contract and expose it
through a new MCP tool.

1. **One new adapter method, one new capability flag (ADR-0012 shape).**
   `DatabaseAdapter::foreign_keys(&TableInfo) -> Vec<ForeignKey>` joins
   `table_ddl`/`execute` as a per-capability method with a default that returns
   `DbError::Capability`, gated by `Capabilities::has_foreign_keys`. Pre-ADR
   adapters compile unchanged. `ForeignKey` is a new `dbboard-core` value type:
   local `columns`, `referenced_table` (`TableInfo`), `referenced_columns`
   (aligned 1:1 and in key order), and an optional `constraint_name`.

2. **Extend `TableSchema`? No — a separate method.** Attaching foreign keys to
   `describe_table`'s result would churn every `TableSchema { .. }` construction
   site across the workspace and force the cost of an extra introspection query
   onto every describe, most of which don't want it. A separate method keeps
   `describe_table` cheap and matches the granularity ADR-0012 already uses for
   `table_ddl`/`execute`.

3. **Per-engine introspection, mapped to the same shape.**
   - **SQLite/libSQL/D1** (Turso, D1): `PRAGMA foreign_key_list('t')`, grouped
     by the PRAGMA's `id` into composite keys ordered by `seq`. A `NULL` parent
     column (`to`) means the DDL omitted the parent column list — an implicit
     reference to the parent's primary key — resolved with one `describe_table`
     of the parent. No `constraint_name` (SQLite does not report one).
   - **Postgres-wire** (Postgres/Neon/Supabase/CockroachDB/Aurora DSQL):
     `pg_catalog.pg_constraint` where `contype = 'f'`, with `conkey`/`confkey`
     unnested `WITH ORDINALITY` and re-joined on position so local and
     referenced columns stay aligned for composite keys. `conname` is the
     `constraint_name`. Aurora DSQL has no foreign keys, so the query simply
     returns no rows — the capability is still advertised because the
     introspection path itself works.

4. **A seventh MCP tool, `list_relationships`.** Params
   `{ connection_id, table? }`. With no `table`, it maps the connection's whole
   foreign-key graph; with a `table`, it returns edges touching that table on
   *either side* ("how is `orders` connected?" wants both its outbound
   references and the tables that reference it). Composed in the service layer
   over `foreign_keys` across `list_tables`, so the either-side filter and the
   directed-edge shape live in one place. Blank `table` is normalized to "no
   filter" (not rejected — an empty graph request is meaningful, unlike a blank
   search needle). Still read-only, secrets still never serialized: the
   ADR-0046 invariants hold.

5. **Bounded output.** Edges are capped at `MAX_RELATIONSHIPS` (500) with a
   `truncated` flag, the same posture as `run_read_query` and `search_schema`.

### Out of scope

- **Referential actions** (`ON DELETE CASCADE`, `ON UPDATE`, `MATCH`, deferrable
  state). The join graph — which columns reference which — is what a query-writing
  agent needs; action semantics are a schema-editing concern and would widen the
  `ForeignKey` type for no read-side gain. They can follow if asked for.
- **Inferred / logical relationships** (naming-convention guesses where no
  declared FK exists). This tool reports *declared* constraints only; guessing is
  a separate, lossy heuristic.
- **Cross-database references.** Every edge is within one connection.

### Consequences

- The MCP surface is now **seven tools**; the ADR-0053 "six tools" language in
  `server.rs`/`lib.rs` and its consequences are superseded by this entry. The
  tool is additive — existing clients are unaffected.
- Every shipping adapter advertises `has_foreign_keys`, so `list_relationships`
  works across the whole internal-distribution connection set (Turso, D1,
  Postgres-wire including Aurora DSQL). DSQL returns an empty graph rather than
  erroring.
- The adapter contract grew a method; the `dbboard-core` `ForeignKey` type is
  now part of the shared vocabulary the sibling `dbboard-web` repo should track
  for feature parity (per CLAUDE.md), though no code is shared.
- Cost note: the no-`table` graph walk issues one `foreign_keys` call per table,
  the same N-call shape as `search_schema`. Acceptable for the collector
  databases in scope; the `MAX_RELATIONSHIPS` cap bounds the output.

## ADR-0055 — Automated PII / secret leak scanning

- **Status**: Accepted 2026-07-24
- **Relates to**: `docs/maintainer/history-sanitize-runbook.md` (the one-time
  *remediation* of names already in history — this ADR is the ongoing
  *prevention*), ADR-0038 (encrypted connection bundles — why real secrets are
  never in tracked files in the first place)

### Context

dbboard is developed against real, business-identifying databases (store
connection names, sample rows, the maintainer's machine paths) but published as
a public repository. Real store names have already reached tracked test
fixtures once; the history rewrite runbook removes what landed, but nothing
*prevents the next one*. We need a guard that runs on every commit, on every
commit message (a real name pasted into a message leaks exactly as badly as one
in a file), and on a daily schedule so an out-of-band merge or direct push is
caught within a day.

The tension: this is a database client, so its own test suite is legitimately
full of *synthetic* connection strings and example emails. A scanner that
blocks on every passworded-URL shape would be a false-positive wall that trains
everyone to `--no-verify`.

### Decision

A single POSIX-sh scanner, `scripts/pii-scan.sh`, invoked three ways: local
`pre-commit` (`--staged`) and `commit-msg` (`--message`) hooks via cargo-husky,
and a `pii-scan.yml` GitHub Actions workflow (push/PR/daily-cron) running
`--selftest`, `--tree`, and `--range origin/main..HEAD`.

1. **Two severities, because fixtures are noisy.**
   - **Blocking** (fails commit/CI): a *denylist* of real literals, plus
     `private-key` and `aws-access-key-id` shapes. These almost never appear as
     fixtures.
   - **Advisory** (printed in the daily scan, never fails): `passworded-db-url`,
     `personal-email`, `windows-home-path`. By project invariant real secrets
     live only in the OS keyring, so a passworded URL in a tracked file is a
     fixture — worth review, not a build break. Known real values are promoted
     to blocking by adding them to the denylist.
2. **The denylist is never committed.** Committing the real names would put the
   very strings we hide back into a tracked, public file. It lives in an
   untracked `.pii-denylist` (gitignored) locally and the `PII_DENYLIST` repo
   secret in CI, materialized per-run and shredded after. A tracked
   `.pii-denylist.example` documents the format only.
3. **Matches are redacted.** Denylist hits print `[denylist#<sha8>] file:line
   (match redacted)`; CI runs without `--reveal`. A public Actions log never
   echoes a string the scanner exists to hide. Local hooks pass `--reveal`
   because a private terminal is not a public log.
4. **A narrow allowlist** (`scripts/pii-scan.allow`) drops known-safe shapes
   (placeholder emails, example DB URLs, `C:\Users\<placeholder>` docs paths)
   so a clean tree scans green. Denylist literals cannot be allowlisted.
5. **History is out of scope.** CI scans HEAD and *new* commit messages only.
   Full history still holds un-remediated names pending the runbook rewrite;
   scanning it would be permanently red and bury the live signal.

### Alternatives considered

- **A third-party secret scanner (gitleaks/trufflehog).** Strong on generic
  credential shapes, but the actual leak here is *business names* — arbitrary
  literals only the maintainer knows — which a denylist expresses directly. A
  20-line sh script with a private denylist fits the threat; a vendored binary
  action would also widen the CI supply-chain surface (CLAUDE.md flags new
  Actions/deps). Revisit if generic-credential coverage becomes the priority.
- **Block on every passworded-URL shape.** Rejected: false-positive wall against
  this codebase's fixtures; would erode the hook's credibility. Advisory tier
  keeps the signal without the noise.
- **Commit the denylist.** Self-defeating — it publishes the hidden strings.

### Consequences

- A leaked store name/credential is caught before it enters a commit, a commit
  message, or (within a day) an out-of-band update — closing the gap the
  history runbook can only clean up after the fact.
- Operators must maintain the denylist in two places (local file + CI secret);
  `docs/maintainer/pii-scanning.md` is the operator guide. Absent a denylist the
  scan degrades to generic rules, it does not break.
- The hooks reinstall from `.cargo-husky/hooks/` on the next `cargo test`.
- Advisory findings need periodic human review of the daily run; they do not
  gate merges by design.

## ADR-0056 — dbboard design system: branded egui theme

- **Status**: Accepted 2026-07-24
- **Relates to**: `DESIGN.md` (the token spec this fills in), ADR-0041 (Light /
  Dark / Auto theme selection — the switch this theme plugs into), ADR-0015
  (CJK fallback font install, which the new `install_look` sequences before the
  theme)

### Context

Until now the UI ran on **stock egui styling**: the bundled `Ubuntu-Light`
font, egui's built-in blue-grey palette, and a handful of ad-hoc
`Color32::{LIGHT_RED, LIGHT_GREEN, YELLOW}` literals at five call sites that
ignored the active theme (a `LIGHT_RED` error label stayed the same washed-out
red on both grounds). `DESIGN.md` was a placeholder: every palette, type, and
spacing slot read `TBD`, with only the brand accent (`#4F46E5`, from the logo)
pinned. The app looked generic — not because egui is limiting, but because we
had never applied a design.

The maintainer asked to raise the visual quality after an initial
performance-first framing. An HTML before/after mock was approved as the
direction; this ADR records the Rust side of it.

### Decision

A central `dbboard-ui::theme` module owns one branded palette and applies it
once at startup via `theme::apply(ctx)`:

1. **Both themes registered up front.** `apply` calls
   `Context::set_visuals_of` for *both* `Theme::Dark` and `Theme::Light` with a
   customised `Visuals`, then sets shared spacing/radius tokens through
   `all_styles_mut`. Auto (follow-OS) therefore keeps working for free — egui
   swaps between the two registered visuals as the OS theme changes, with no
   per-frame reapplication. The existing theme *pick* (ADR-0041) just selects
   which registered visuals are active.

2. **Indigo-tinted neutrals, indigo accent, separate semantic axis.** Grounds
   are tinted toward the accent rather than pure grey (`canvas`/`surface`/
   `surface.alt` per theme). The accent is the brand indigo — `#4F46E5` on
   light, a brighter `#6366F1` on the dark ground so it keeps its punch.
   Danger/warning/success are a *separate* axis from the accent and are exposed
   as theme-aware accessors (`theme::danger(dark_mode)` etc.) that map onto
   egui's own `error_fg_color` / `warn_fg_color`.

3. **The five ad-hoc colour sites now read the palette.** `ai.rs` (prefetch
   warning), `ai_settings.rs` + `connections.rs` (delete confirmations),
   `connections.rs` (export summary), and `errors.rs` (error label) call the
   accessors instead of hard-coding one RGB. The staged-edit dirty-cell tint
   (previously derived from `selection.bg_fill`) now keys off the accent
   directly: a premultiplied *translucent* `Color32` reads its channels back
   *dimmed*, so a translucent selection fill could not double as the tint
   source — the accent is opaque and keeps its RGB across themes.

4. **Fonts are deferred to a fast-follow (Phase 2).** Bundling Inter +
   JetBrains Mono is a separable concern (binary assets under version control,
   OFL licence text, ~hundreds of KB) and the approved mock itself approximated
   the UI face with the platform system font — the palette, spacing, and
   semantic colours are what carried the look. Shipping the theme first keeps
   this change reviewable; the font install will extend `install_look`.

### Consequences

- The app is branded and theme-consistent; no call site hard-codes a
  theme-blind colour. New UI reads `ui.visuals()` or the `theme::*` accessors.
- `apps/dbboard` gains a single `install_look(ctx, theme)` seam that sequences
  fonts → design system → theme pick before the first paint (no flash), the
  natural place the Phase 2 font bundle will hook into.
- egui's premultiplied-alpha `Color32` is a standing gotcha: reading `.r()/.g()
  /.b()` off a translucent colour returns dimmed channels. Colours meant to be
  sampled must be opaque.
- `DESIGN.md`'s palette / typography / spacing tables move from `TBD` to the
  locked tokens; the module is the single source and the doc mirrors it.

## ADR-0057 — Design system, applied: primary CTA, header identity, count badge, unit-aware threshold

- **Status**: Accepted 2026-07-24
- **Relates to**: ADR-0056 (the branded theme this consumes — palette, spacing,
  radius tokens), ADR-0050 (the backup warn threshold this re-skins), ADR-0041
  (Light / Dark / Auto pick — now a segmented control), ADR-0030 (auto-limit
  guard living on the same toolbar), `DESIGN.md`

### Context

ADR-0056 locked the palette, spacing, and semantic colours but stopped at
"apply the tokens." Standing next to the approved HTML mock, the running app
still read flat: every button was the same neutral grey (the primary **Run**
action no more prominent than a checkbox), the theme picker was a dropdown
buried in the menu bar, there was no at-a-glance signal of *which connection is
live* or *how many tables it has*, and the backup warn threshold was a bare
six-digit `DragValue` the maintainer called fiddly to adjust. The mock's edge
over ours was **structural, not chromatic** — hierarchy, identity, and
affordance — so this ADR records the component-level application of the theme
rather than any new colour.

### Decision

Four slices, all built on ADR-0056 tokens, adding **no new i18n strings** (they
reuse existing keys or locale-neutral proper nouns / multiplier symbols):

1. **Primary call-to-action.** `theme::primary_button(dark_mode, text)` returns
   an `egui::Button` filled with the brand accent and an opaque `ON_ACCENT`
   label — the one filled button on the query toolbar. Every sibling control
   (auto-limit, backup, restore) stays a neutral secondary, so **Run** now reads
   as the primary action. A new `ON_ACCENT` constant is opaque by construction
   (the premultiplied-alpha gotcha from ADR-0056 would otherwise dim a sampled
   label).

2. **Header identity: pill + segmented theme toggle.** `theme::pill(ui, text,
   accent_dot)` draws a rounded chip (faint fill, hairline stroke, optional
   status dot) at the ADR-0056 widget radius. A slim **header strip below the
   menu bar** carries an **active-connection pill** (`name · adapter`, accent
   dot) on the left and an inline **Auto | Light | Dark** segmented control on
   the right, replacing the old `theme_menu` dropdown. The strip is a dedicated
   row rather than the menu bar's leftover space: sharing the menu row let the
   long pill and toggle overlap the menus on a narrow window (egui menu bars do
   not wrap). The dot signals *active*, not health — there is no live probe, so
   it deliberately does not claim connectivity.

3. **Sidebar table-count badge.** The Tables heading carries a count pill
   (`self.tables` length). This is the row of information the mock's sidebar
   badges implied *that we can source honestly*: table count is already in hand.
   **Per-table row-count badges are explicitly deferred** — they need a
   per-table `COUNT(*)` we do not fetch and which is heavy on large DBs; showing
   a fabricated or blocking number would be worse than showing none.

4. **Unit-aware backup threshold (ADR-0050 re-skin).** The raw `DragValue`
   becomes a mantissa editor plus a `×1 | ×1K | ×1M` unit selector. `split_rows`
   seeds the editor from the stored count (largest evenly-dividing unit, so a
   round `500_000` reads `500 ×1K`); `compose_rows` recombines with a saturating
   multiply so an extreme mantissa cannot wrap the threshold to a small number
   and silently disable the huge-DB warning. **The metric stays a row count**
   throughout — the maintainer asked for byte units, but `DumpPlan` carries no
   byte estimate, so units here are multipliers on rows, not bytes.

Supporting move: `ConnectionKind::adapter_label()` moves to the config layer
(`dbboard-config::store`) as the single source of the display name per adapter;
`connections::kind_label` now delegates to it, so the header pill and the
connections window cannot drift apart.

### Consequences

- The query view has one visually primary action; the header answers "which
  connection, which theme" without opening a menu. New primary actions call
  `theme::primary_button`; new chips call `theme::pill`.
- Zebra striping on the result grid was already enabled (`.striped(true)`) and
  now reads correctly off the ADR-0056 faint-row tint — no code change, recorded
  so it is not re-litigated.
- Two honesty boundaries are load-bearing and intentional: the status dot means
  *active-not-health*, and the sidebar shows *table count only* (row counts
  await a lazy `COUNT(*)` design). Both were chosen over fabricating a number.
- Threshold semantics are unchanged for existing `ui-settings.toml` files:
  `split_rows`/`compose_rows` round-trip any stored value exactly; a
  non-round count simply shows under `×1`.
- Still deferred from ADR-0056: the Phase 2 font bundle (Inter + JetBrains
  Mono), a separable binary-asset change.

## ADR-0059 — Tauri 2 + SvelteKit spike for the presentation-layer rewrite

- **Status**: Accepted 2026-07-27 (spike — not a commitment to migrate)
- **Relates to**: ADR-0056 / ADR-0057 (the egui design system this would
  eventually replace), the `dbboard-mcp` `McpService` (the egui-free core the
  spike reuses verbatim), and the sibling `dbboard-web` (Nuxt + NestJS) whose
  stack this is deliberately *not* copying

### Context

The maintainer runs md-business (a separate project) on **Tauri 2 + SvelteKit**
and asked for dbboard's desktop UI to move to "the same mechanism." That is a
presentation-layer rewrite — weeks of work — so before committing we build a
**thin vertical spike**: one screen (pick a connection → run a SELECT → see a
result grid) that exercises the whole WebView↔Rust↔core path against the real
3-store `connections.toml`. If the spike is honest end-to-end, the full
migration is de-risked; if the WebView↔core boundary fights us, we learn it for
the cost of one screen instead of the whole app.

The egui coupling is confined to `dbboard-ui` + `apps/dbboard`. `dbboard-core`,
the adapters, `dbboard-config`, and `dbboard-mcp` are already egui-free, so the
spike is a shell swap, not a rewrite of the data path.

### Decision

New crate `apps/desktop/src-tauri` (`dbboard-desktop`) + a SvelteKit frontend in
`apps/desktop`, added as a workspace member. Key choices:

1. **Reuse `McpService` as the backend.** The spike's three Tauri commands
   (`list_connections`, `list_tables`, `run_read_query`) are thin wrappers over
   the same transport-agnostic `McpService` the MCP server already ships —
   config loading, keychain secrets, adapter connect, and read-only query are
   all solved. The spike inherits the engine-enforced read-only guarantee for
   free; it adds **no new DB code**.
2. **SvelteKit as a static SPA** (`adapter-static`, `ssr = false`,
   `prerender = true`). The desktop app has no server; the WebView loads the
   prerendered shell off disk and talks to Rust via `invoke`. A typed
   `$lib/api.ts` mirrors the `McpService` JSON shapes so components never touch
   `invoke` string names.
3. **pnpm, not npm** (per policy), with the supply-chain guards in
   `pnpm-workspace.yaml` — `minimumReleaseAge: 1440` and an explicit
   `onlyBuiltDependencies` / `allowBuilds` allowlist (only esbuild runs an
   install script, and only to link its already-present prebuilt binary).

### Consequences

- Adds a **JS/TS toolchain** to a so-far Rust-only repo, scoped entirely under
  `apps/desktop`. `node_modules/`, `build/`, `.svelte-kit/`, and Tauri's
  `gen/` are git-ignored; the committed surface is source + config + the five
  desktop icon sizes `tauri.conf.json` references.
- **pnpm 11 gotcha, recorded so we don't relearn it**: pnpm 11 reads
  project settings from `pnpm-workspace.yaml`, **not** package.json's `pnpm`
  field or `.npmrc`. Build scripts are gated behind `allowBuilds`; esbuild's
  platform binary arrives via the `@esbuild/win32-x64` optional dep, so the
  script only needs allowing to silence pnpm's ignored-build error.
- **Icons are reused** from `apps/dbboard/assets/dbboard-logo-256.png` via
  `tauri icon`; the unused mobile/Store tile outputs were pruned.
- This is a **spike, not a decision to migrate**. The two UIs (egui + Tauri)
  coexist in the workspace until the maintainer evaluates the running spike.
  If migration is rejected, `apps/desktop` is deleted and the workspace member
  removed — nothing in the core depends on it.

## ADR-0060 — CodeMirror 6 for the desktop SQL editor

**Status:** Accepted · **Date:** 2026-07-27 · **Scope:** `apps/desktop` frontend

### Context

The Tauri query editor started as a plain `<textarea>`. To match the
visual-direction mock and to be usable for real work (the maintainer wants the
Tauri build releasable as v0.4.0), the editor needs SQL syntax highlighting,
line numbers, bracket matching, and room to grow into autocomplete against the
live schema. Hand-rolling a highlighter overlay would re-implement a solved
problem and cap out well short of autocomplete.

### Decision

Adopt **CodeMirror 6** (modular `@codemirror/*` packages, not the `codemirror`
meta-bundle) behind a single `SqlEditor.svelte` wrapper:

1. **Composed, not `basicSetup`.** Only the extensions we use are imported
   (line numbers, active line, history, close-brackets, autocomplete, `lang-sql`,
   syntax highlighting). This keeps the bundle honest and the config legible.
2. **Themed through the design tokens.** The CM theme and `HighlightStyle`
   reference `var(--...)` (keyword = accent, string = success, number = warning,
   comment = faint), so the editor re-themes on the same light↔dark token swap
   as everything else — no editor-specific theme switching.
3. **Two-way bind with an equality guard.** An `updateListener` pushes edits to
   the bound `value`; an `$effect` adopts external sets (the sidebar's "Select
   top 100" injecting a query) only when the text actually differs, avoiding a
   feedback loop.
4. **Cmd/Ctrl-Enter to run** is registered as a highest-precedence keymap so it
   is never swallowed by the default bindings.

### Consequences

- Adds seven `@codemirror/*` / `@lezer/highlight` runtime deps to
  `apps/desktop`. All are stable, widely used, and clear `minimumReleaseAge`.
- **vitest is pinned to v2** (`^2.1.9`), not v4: vitest 4 requires Vite 6, and
  the app is on Vite 5 (SvelteKit 2.9). Revisit when the app moves to Vite 6.
- Pure SQL-text generation (`quoteIdent`, `qualifiedName`, `selectTopN`) lives
  in `$lib/sql/build.ts` and is unit-tested (`build.test.ts`) independent of
  CodeMirror — identifiers are always double-quoted (Postgres + SQLite/libSQL
  safe), the one injection surface.
- Still within the ADR-0059 spike: if the Tauri UI is dropped, this goes with
  `apps/desktop`.

## ADR-0061 — Aurora DSQL cannot take the read-only transaction preamble

- **Status**: Accepted 2026-07-27
- **Relates to**: ADR-0046 §8 (the read-only enforcement this amends for one
  flavor) and ADR-0021 (the Aurora DSQL divergence log this extends)

### Context

ADR-0046 §8 hardens the MCP read path with a two-part backstop: every
read-only statement runs inside a transaction opened with
`SET TRANSACTION READ ONLY` (the engine then rejects every write for the
transaction's life) plus `SET LOCAL statement_timeout = '30s'` (a server-side
cancellation backstop for an abandoned query). This shipped for all Postgres
flavors alike.

Aurora DSQL rejects **both** statements:

- `SET TRANSACTION READ ONLY` — DSQL does not implement the `SET TRANSACTION`
  command form. It parses the word `TRANSACTION` as a GUC name and fails with
  `ERROR: setting configuration parameter "TRANSACTION" not supported`. This
  is the error a user hit running `SELECT * FROM … LIMIT 100` against a live
  Aurora DSQL IAM connection — a query that worked before ADR-0046 added the
  preamble.
- `statement_timeout` — DSQL manages transaction duration itself and lists
  `statement_timeout` (with `lock_timeout`, `idle_in_transaction_session_timeout`)
  among the parameters it does not accept via `SET` / `SET LOCAL`.

DSQL's supported-session-parameter list and its fixed `REPEATABLE READ`
isolation are documented at
<https://docs.aws.amazon.com/aurora-dsql/latest/userguide/accessing.html>.

**Correction (2026-07-27).** The first cut of this ADR asserted that
server-side cursors (`DECLARE` / `FETCH`) — the other half of the read path —
were *not* on DSQL's unsupported list, so the cursor row-cap was kept. That
was wrong. DSQL rejects `DECLARE CURSOR` with
`ERROR: unsupported statement: DeclareCursor`, which a user hit on the very
next query after the preamble fix shipped. `DECLARE CURSOR` is on DSQL's
unsupported-features list
(<https://docs.aws.amazon.com/aurora-dsql/latest/userguide/working-with-postgresql-compatibility-unsupported-features.html>).
So the cursor row-cap cannot be used on DSQL either — see the amendment below.

### Decision

Gate the preamble on the flavor. A pure `read_only_preamble(flavor)` helper
returns the two `SET` statements for the standard flavors
(`postgres` / `neon` / `supabase`) and an **empty preamble** for
`aurora-dsql`. `run_read_only_txn` still opens a transaction and still rolls
back on drop — only the two unsupported `SET` statements are skipped for DSQL.
The row-cap mechanism is also flavor-gated (see the amendment).

### Consequences

- **DSQL loses the transaction-level read-only backstop.** There, the sole
  read-only guarantee is the pre-connection `classify_read_only` AST guard,
  which already rejects every write, multi-statement batch, and
  data-modifying CTE before a connection is opened — so the app-layer
  guarantee is unchanged; only the engine-level belt is absent on DSQL. This
  is strictly safer than the pre-ADR-0046 DSQL read path, which ran the query
  with no transaction wrapper and no classifier at all.
- **DSQL loses the `statement_timeout` cancellation backstop**, but DSQL
  enforces its own transaction-duration limits, so an abandoned query cannot
  pin a connection indefinitely regardless.
- Restoring a begin-time read-only transaction on DSQL
  (`BEGIN READ ONLY` / `START TRANSACTION READ ONLY`) is a possible follow-up
  once verified against a live cluster; it is deliberately out of scope here
  to keep the fix to the minimal change that unblocks the connection without
  risking another unsupported-statement rejection.
- The helper is unit-tested (`read_only_preamble_*`); the flavor-gated read
  path itself is exercised end-to-end only against a live DSQL cluster, which
  no CI runner has — that verification is the maintainer's.

### Amendment (2026-07-27) — the row-cap must also be flavor-gated

The preamble fix above unblocked the `SET TRANSACTION` rejection, but the very
next query (`SELECT * FROM … LIMIT 100`) failed with
`ERROR: unsupported statement: DeclareCursor`. The read path caps a
row-returning query with a server-side cursor —
`DECLARE dbboard_ro_cursor NO SCROLL CURSOR FOR <sql>` then
`FETCH FORWARD <max_rows>` — so that only `max_rows` rows ever cross the wire.
Aurora DSQL does not implement `DECLARE CURSOR`, so this half of the read path
was still broken for DSQL.

**Decision.** Flavor-gate the row-cap mechanism too, via a pure
`caps_with_cursor(flavor)` helper:

- Standard flavors (`postgres` / `neon` / `supabase`) keep the cursor cap
  (`fetch_via_cursor`).
- `aurora-dsql` uses a new `fetch_capped_stream`: it streams the query's
  portal with `sqlx::query(sql).fetch(...)` and stops after `max_rows` rows,
  then drops the stream so the server stops producing more. This preserves the
  "at most `max_rows` rows cross the wire" property **without a cursor**, and
  without wrapping arbitrary SQL in a `LIMIT` subquery — subquery-wrapping
  would break on a query with duplicate output column names (`SELECT id, id`).

`EXPLAIN` continues to run directly (`run_capped`) on all flavors; it was
never a cursor source.

**Consequences.**

- The DSQL read path is now fully functional: both the preamble and the
  row-cap are DSQL-compatible.
- `caps_with_cursor` is unit-tested (`caps_with_cursor_on_standard_postgres`,
  `does_not_cap_with_cursor_on_aurora_dsql`). The streaming cap itself is
  exercised end-to-end by `aurora_dsql_read_only_caps_without_a_cursor`, gated
  on a live `DBBOARD_AURORA_DSQL_URL` — maintainer-run, as no CI runner has a
  DSQL cluster.
- Lesson: "not mentioned in the accessing.html session-parameter list" is not
  the same as "supported". The authoritative source for what DSQL rejects is
  the unsupported-features page, which lists `DECLARE CURSOR` explicitly. The
  original ADR trusted the wrong list.

## ADR-0062 — Connection management write path (Tauri desktop, v0.4.0 parity)

- **Status**: Accepted 2026-07-27
- **Relates to**: ADR-0016 (the original connection-management UI model),
  ADR-0020 (in-process connection switching), ADR-0038 (passphrase-encrypted
  bundle export/import), ADR-0046 / ADR-0059 (the read-only Tauri spike this
  lifts the write ban from), and the ADR-0016 secrets-in-keyring rule.

### Context

The Tauri 2 + SvelteKit rewrite (ADR-0059) shipped as a deliberately
**read-only** spike: it could browse connections defined in `connections.toml`
but could not create, edit, or delete them, and had no bundle import/export.
That was fine for a spike but is a **regression against the egui build**, which
has full connection CRUD *and* passphrase-encrypted bundle transfer. A user
upgrading from the egui app to the Tauri app would lose the ability to add a
connection from inside the app and — worse — lose import/export entirely, which
is how connections move between machines.

It also surfaced as two concrete bug reports that both root-cause to
"no connection configured, and no in-app way to add one": *Select top 100*
produced no SQL and the *Run* button could not be pressed, because both require
a selected connection that a fresh install has no way to create.

The read path (`McpService`, reads `connections.toml` fresh, caches adapters)
must not be the thing that writes: mixing a cache with a mutator invites stale
credentials. The egui build already solved this with `ConnectionAdmin` in
`dbboard-config` — a CRUD facade that owns `connections.toml` + the OS keyring
with rollback discipline. The Tauri app should reuse it verbatim, not
reimplement it.

### Decision

Lift the read-only boundary for connection management and wire the existing
`dbboard-config::ConnectionAdmin` into the Tauri app as the **sole writer**.

- **`AppState` gains `admin: Mutex<ConnectionAdmin>`** alongside the existing
  `service: McpService`. The two share one `connections.toml`; after any write
  the app calls `service.invalidate(connection_id)` to evict the matching
  cached adapter so the read path can never serve stale credentials.
- **Six new commands**: `add_connection`, `update_connection`,
  `delete_connection`, `connection_edit_fields`, `export_connections`,
  `import_connections`.
- **DTO boundary.** `dbboard-config`'s draft enums carry inline secrets and do
  not derive `Serialize`/`Deserialize`, so the app defines thin Tauri DTOs
  (`KindInput` / `KindEditInput`, tagged `#[serde(tag = "kind",
  rename_all = "snake_case")]`) that the frontend speaks, and maps them to the
  draft types. The Svelte contract stays decoupled from the config internals.
- **Secrets are never read back (ADR-0016).** `connection_edit_fields` returns
  an `EditFieldsDto` with **non-secret fields only**. The edit form leaves
  secret inputs blank; a blank secret on save means "keep the stored secret"
  (`SecretField::Keep`), never "clear it" (`SecretField::Set`).
- **Bundle file I/O stays in Rust.** The frontend uses the Tauri dialog plugin
  only to pick a save/open path; the encrypted blob and the passphrase never
  round-trip through the WebView. `export_connections` writes the ciphertext,
  `import_connections` reads it, and both call `ConnectionAdmin`'s
  `export_bundle` / `import_bundle` (ADR-0038). Import is additive: ids already
  present are **skipped, never overwritten**, and the `ImportReportDto` reports
  imported vs skipped ids.
- **Pure frontend draft module.** Validation and DTO-shaping live in an
  I/O-free `$lib/connections/draft.ts` (mirroring the egui form/admin split),
  so the Svelte `ConnectionManager.svelte` only binds inputs and the command
  only receives an already-validated payload.

### Consequences

- The two upgrade-blocking bugs are fixed at the root: a fresh install can now
  add a connection from inside the app, so *Select top 100* and *Run* have a
  connection to act on.
- Import/export parity with the egui build is restored, so upgrading users keep
  the workflow that moves connections between machines.
- `connections.toml` + `annotations.toml` remain the shared source of truth
  between the egui and Tauri builds (both resolve
  `ProjectDirs::from("dev", "dbboard", "dbboard")`), so a connection added in
  one build appears in the other.
- Coverage: the DTO↔draft mapping (blank-handling for optional/secret fields),
  the add/update/delete flow over a temp store, duplicate-id rejection, and the
  export→import file round-trip are unit-tested in the desktop crate (12
  tests); the pure draft module is unit-tested on the frontend (22 tests).

### v0.4.0 desktop-parity scope map

This ADR is the connection vertical. The **directive is full egui parity in
one release — no segmentation, no "this is also missing" follow-ups.** The
remaining verticals below are tracked here so the whole scope is visible; each
lands as its own focused, tested commit under this parity effort, and each gets
its own ADR entry where it introduces a new decision.

| Vertical | egui source | Status |
|---|---|---|
| Connection CRUD + bundle import/export | `ConnectionsView` + `ConnectionAdmin` | **This ADR — done** |
| Inline cell editing (UPDATE-only, declared PK, `rows_affected == 1` gate) | ADR-0042 | **Done — ADR-0063** |
| Local annotation editing (table/column notes, empty = delete) | ADR-0045 | **Done** |
| Dataset export (CSV / CSV-with-BOM / TSV, row selection) | ADR-0035 | **Done** |
| Logical backup / dump (warn-and-allow threshold; Turso emits no DDL) | ADR-0049 / ADR-0050 | **Done — ADR-0064** |
| Logical restore / import (empty-target confirm; per-engine txn strategy) | ADR-0051 | **Done — ADR-0065** |
| AI assistant (provider trait; explain/suggest; never runs SQL, never sends rows) | ADR-0052 | **Done — ADR-0066** |
| Auto-update (updater plugin + signed `latest.json`; bump 0.3.0 → 0.4.0) | ADR-0044 / ADR-0043 | **Done — ADR-0067** |

The read-only enforcement of the *query* path (ADR-0046 §8, ADR-0061) is
unchanged: lifting the write ban applies to connection **management**, not to
arbitrary SQL — user SQL still runs through the read-only classifier.

---

## ADR-0063 — Desktop inline cell editing: the first DB-write surface (Tauri, v0.4.0 parity)

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0042 (the egui cell-edit model this ports — `build_update_sql`,
  the declared-primary-key requirement, the `rows_affected == 1` commit gate),
  ADR-0046 §8 / ADR-0061 (the read-only classifier on the *query* path, which
  this does **not** relax), ADR-0059 (the read-only Tauri spike), and ADR-0062
  (the connection **management** write path, whose read/write split this reuses).

### Context

The Tauri build could browse rows but not change them — a regression against the
egui app, which edits a cell in place and writes it back as a single-row UPDATE.
This is the second desktop write surface after connection management (ADR-0062),
and the first that writes to a **user's database** rather than to local config.
That raises two questions ADR-0062 did not: *where does the write live* relative
to the read-only MCP tool surface, and *what stops a mis-keyed edit* from
clobbering more than one row.

`McpService` is already the shared desktop data-access layer as well as the
read-only MCP tool surface. The temptation is to expose the write as one more
MCP tool; that would hand external agents a mutation path, which ADR-0046 §8
forbids. The two roles must be split at the method level, not the type level.

### Decision

1. **The write path is a `McpService` method that is deliberately *not* an MCP
   tool.** `McpService::apply_row_update(connection_id, &UpdatePlan)` performs
   the write and returns the affected-row count. It is called only from the
   desktop Tauri command `update_row`; it is **never** registered in the MCP
   tool router. External agents keep the exact read-only surface they had.
   `ServiceError::WriteBack` / `NotEditable` map to `invalid_params` in the MCP
   layer purely for exhaustiveness — they are unreachable from a tool call.

2. **Editability is gated on a *declared* primary key, decided on the frontend.**
   A result grid is editable only when it came from a sidebar *Select top 100*
   browse (which carries the source `TableInfo`) **and** `describeTable` reports
   a non-empty `primary_key`. An arbitrary query is never editable; a table with
   no declared PK (including rowid-only SQLite tables) shows a read-only note.
   The pure grouping step — matching each staged edit to its row's PK values —
   lives in `apps/desktop/src/lib/grid/edit.ts` and is unit-tested in isolation;
   it throws if the PK is empty or a PK column is absent from the result, so a
   browse must `SELECT *`.

3. **The command enforces the `rows_affected == 1` gate** (parity with egui's
   `advance_save`). `update_row` returns `Ok(())` only when exactly one row
   matched; `0` and `n > 1` both surface as an error and leave the edit staged,
   so a stale or non-unique key can never silently write the wrong rows.

### Consequences

- The desktop gains in-place editing at egui parity while agents stay strictly
  read-only — the split is enforced by *what is registered as a tool*, so it
  cannot be bypassed by an agent crafting a request.
- Editing requires a declared PK. rowid-only SQLite tables and view/derived
  results are intentionally read-only in the app, matching egui.
- Tested RED-first: 4 integration tests on `apply_row_update` in
  `dbboard-mcp` (writes exactly one row and reports it; clears a cell to NULL;
  reports `0` when the key matches nothing; surfaces a write-back refusal) and
  8 unit tests on the pure `buildRowUpdates` grouping in `edit.test.ts`.

---

## ADR-0064 — Desktop logical backup: wiring the core dump to Tauri (v0.4.0 parity)

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0049 (the logical-dump design and the pure
  `dbboard-core::dump` orchestrator/preflight this wires), ADR-0050 (the
  user-configurable warn threshold), ADR-0059 (the read-only Tauri spike),
  ADR-0062 (connection-management write path), and ADR-0063 (inline cell
  editing — the *non-MCP-tool* write-method pattern this reuses).

### Context

The logical dump was already built and tested end-to-end in `dbboard-core`
(`plan_dump` preflight, `run_dump` orchestrator, Value→SQL serialization) and
shipped in the egui app via `dbboard-ui::backup`. The Tauri build had none of
it. This vertical is **wiring only** — no new dump logic — plus the desktop
pieces the domain layer cannot hold: file I/O, a cancellation flag, progress to
the WebView, and the confirmation/threshold UX. Three questions had to be
answered the same way ADR-0063 answered them for cell editing: *where does the
write method live* relative to the read-only MCP tool surface, *how does an
un-serialisable plan cross IPC*, and *where does the warn threshold live*.

### Decision

1. **The dump is two `McpService` methods that are deliberately *not* MCP
   tools.** `plan_dump(connection_id)` and
   `run_dump(connection_id, &DumpPlan, &mut dyn DumpSink, &dyn DumpControl)`
   resolve the adapter and its dialect, then call the core functions. Neither is
   registered in the MCP router, so external agents keep the exact read-only
   surface — identical to `apply_row_update` (ADR-0063). Two new
   `ServiceError` variants, `NotDumpable` (adapter has no known SQL dialect) and
   `Dump` (the output sink failed), map to `invalid_params` / `internal_error`
   in the MCP layer purely for exhaustiveness; they are unreachable from a tool
   call.

2. **`DumpPlan` never crosses IPC; the run re-plans internally.** `DumpPlan` is
   not `Serialize` (it holds `TableInfo`), so `plan_dump` returns a flat
   `DumpPlanDto` (table names + counts + `total_rows` + `is_empty_data`) for the
   confirmation dialog, and the `run_dump` **command** re-runs the preflight
   itself before dumping. A dump's preflight is cheap (one `COUNT(*)` per table)
   relative to the dump, so re-planning is preferable to inventing a
   serialisable plan handle or caching plans across commands.

3. **The warn threshold is frontend-owned (localStorage), like theme and
   language.** ADR-0050 requires a *user-configurable* threshold; the desktop
   satisfies that without a `ui-settings.toml` by persisting it in the frontend
   and applying it there (`exceedsThreshold`). The backend never blocks a dump —
   warn-and-allow (ADR-0049 Decision 8) is a UI prompt, not a server gate — so
   the threshold does not belong in the adapter or dump contract.

4. **Progress and cancellation use a Tauri event + a shared `AtomicBool`.**
   The `run_dump` command builds an `EventControl` (a `DumpControl`) that emits
   each `DumpProgress` as a `dump:progress` event and reads cancellation off an
   `AppState.dump_cancel: Arc<AtomicBool>` that a separate `cancel_dump` command
   flips. Only one dump runs at a time, so a single flag suffices; the run
   clears it first so a stale cancel can't abort the next dump. This mirrors the
   egui `ChannelControl`/`CancellationToken`, swapping the mpsc channel for the
   Tauri event bus. The output is a buffered `FileSink` whose sole write is to
   the user-chosen `.sql` path.

### Consequences

- The desktop gains one-click logical backup at egui parity while agents stay
  strictly read-only — again enforced by *what is registered as a tool*.
- A cancellation mid-run is not an error: `run_dump` returns an outcome with
  `cancelled = true` and the partial file is kept and reported honestly; only an
  unopenable/unwritable output fails the command. Per-table read failures and
  keyless-table truncations are surfaced in the outcome, not hidden.
- For SQLite/libSQL the dump is **data-only** (no `CREATE TABLE`) — the
  `table_ddl` capability is Postgres-only in v1 (ADR-0049 Decision 6/9). The UI
  says so, and a test pins that the Turso dump emits `INSERT`s under a header
  comment and no DDL.
- Tested RED-first: 3 integration tests on the new `McpService` dump methods in
  `dbboard-mcp` (preflight counts the seeded rows; unknown connection is a clean
  not-found; a run emits data inserts and reports the table with no DDL), 3
  unit tests on the desktop `FileSink`/cancel-flag plumbing, and 16 unit tests
  on the pure frontend `plan.ts` (threshold clamp/persist, `exceedsThreshold`
  warn-and-allow boundary, progress percent incl. zero-row and over-count edge
  cases, default file-name slugify).

---

## ADR-0065 — Desktop logical restore: wiring the core restore to Tauri (v0.4.0 parity)

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0051 (the logical-restore design and the pure
  `dbboard-core::restore` orchestrator/preflight this wires), ADR-0059 (the
  read-only Tauri spike), ADR-0063 (inline cell editing — the *non-MCP-tool*
  write-method pattern), and ADR-0064 (desktop logical backup — the sibling
  vertical whose re-plan-on-run and progress/cancel shapes this mirrors).

### Context

The logical restore was already built and tested end-to-end in `dbboard-core`
(`plan_restore` preflight + classifier, `run_restore` orchestrator with the
per-engine transaction strategy and the empty-target gate) and shipped in the
egui app via `dbboard-ui::restore`. The Tauri build had none of it. Like the
dump (ADR-0064) this vertical is **wiring only** — no new restore logic — plus
the desktop pieces the domain layer cannot hold: reading the chosen `.sql` file,
a cancellation flag, and progress to the WebView. The same three questions
ADR-0063/0064 answered recur, and are answered the same way. The one asymmetry
with the dump: a restore has **no sink** — it writes into the target database
through the adapter, not to a file — and no warn threshold; its single safety
gate is the empty-target confirmation.

### Decision

1. **The restore is two `McpService` methods that are deliberately *not* MCP
   tools.** `plan_restore(connection_id, script)` and
   `run_restore(connection_id, &RestorePlan, RestoreOptions, &dyn RestoreControl)`
   resolve the adapter and its dialect, then call the core functions. Neither is
   registered in the MCP router, so external agents keep the exact read-only
   surface — identical to `apply_row_update` (ADR-0063) and the dump methods
   (ADR-0064). Two new `ServiceError` variants, `NotRestorable` (adapter has no
   known SQL dialect) and `Restore` (the run failed), map to `invalid_params` /
   `internal_error` in the MCP layer purely for exhaustiveness; they are
   unreachable from a tool call.

2. **`RestorePlan` never crosses IPC; the run re-plans internally.**
   `RestorePlan`/`RestoreStatement` are not `Serialize`, so `plan_restore`
   returns a flat `RestorePlanDto` (statement counts by kind, `existing_tables`,
   `is_target_empty`) for the confirmation dialog, and the `run_restore`
   **command** re-reads the file and re-runs the preflight itself before
   applying it — the same re-plan-on-run shape ADR-0064 established for the dump.
   The counts are of the *runnable* statements only: transaction-control
   statements (a dump's own `BEGIN`/`COMMIT`) are stripped by the runner and
   excluded so the numbers match what actually executes.

3. **The empty-target confirmation is the one gate, collected in the frontend.**
   A restore into a database that already has tables needs `confirmed = true`;
   the plan DTO's `existing_tables`/`is_target_empty` drives a required checkbox
   the run button reads (`needsConfirmation`). There is no warn threshold — that
   was dump-specific. The `on_error` policy (`stop` | `continue`) is a frontend
   choice that only affects the per-statement (non-atomic) path; anything but the
   explicit `continue` is coerced to the safe `stop` at both ends.

4. **Progress and cancellation use a Tauri event + a shared `AtomicBool`,
   symmetric to the dump.** `run_restore` builds an `EventControl` (a
   `RestoreControl`) that emits each `RestoreProgress` as a `restore:progress`
   event and reads cancellation off an `AppState.restore_cancel:
   Arc<AtomicBool>` a separate `cancel_restore` command flips. The flag is kept
   distinct from `dump_cancel` even though the two are never in flight together,
   so a cancel can never cross verticals; the run clears it first so a stale
   cancel can't abort the next restore.

### Consequences

- The desktop gains one-click logical restore at egui parity while agents stay
  strictly read-only — again enforced by *what is registered as a tool*.
- Per-engine transaction strategy is the core's, unchanged: adapters with
  `has_atomic_restore` run the whole script as one all-or-nothing batch (the
  outcome's `atomic = true`); adapters with only `has_execute` (Cloudflare D1)
  apply each statement in order, honouring `on_error`. The UI reflects which
  path ran in its success message.
- A cancellation mid-run is not an error: `run_restore` returns an outcome with
  `cancelled = true` and the already-applied statements reported; on the atomic
  path the flag is only observed before the indivisible batch starts. Only an
  unreadable file or a hard run error fails the command.
- Statements the classifier could not parse under the dialect still run verbatim
  (best-effort) and are surfaced as an `unparsed_count` warning, so a restore is
  never silently narrowed to what the parser understood.
- Tested RED-first: 3 integration tests on the new `McpService` restore methods
  in `dbboard-mcp` (preflight classifies and sees an empty target; unknown
  connection is a clean not-found; a run applies the script and the rows land),
  3 unit tests on the desktop `on_error` coercion / progress-DTO / cancel-flag
  plumbing, and 13 unit tests on the pure frontend `plan.ts` (statement-based
  progress percent incl. empty-script and over-count edges, `needsConfirmation`
  gate, `hasUnparsed`, `restoreHadFailures`, `normalizeOnError` default, file
  filter).

---

## ADR-0066 — Desktop AI assistant: wiring the provider layer to Tauri (v0.4.0 parity)

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0052 (the AI assistant design — the I/O-free provider
  trait, the explain/suggest split, and the never-runs-SQL / never-sends-rows
  guardrail this ports), ADR-0028 (the `describe_table` metadata prefetch this
  fans out for Suggest), ADR-0063/0064/0065 (the desktop write verticals whose
  *non-MCP-tool* method pattern and cancel-flag plumbing this reuses), and
  ADR-0059 (the read-only Tauri spike this extends).

### Context

The AI assistant was already built and tested in the egui client: a pluggable
provider trait (`dbboard-ai`) with two concrete providers
(`dbboard-anthropic`, `dbboard-openai`), an explain path (send the SQL text),
a suggest path (send the prompt + schema names), and the settings admin that
owns `ai-providers.toml` plus the keyring. The Tauri build had none of it.
Like the other v0.4.0 verticals this is **transport wiring only** — no new AI
logic — plus the desktop pieces the domain layer cannot hold: streaming
deltas to the WebView, a cancellation flag, and the provider-management
command surface. The guardrail that defines this feature is inherited
verbatim: the assistant never runs SQL and never sees a single row.

### Decision

1. **The two AI actions are Tauri commands, deliberately *not* MCP tools.**
   `ai_explain(connection_id, sql)` and
   `ai_suggest(connection_id, prompt, include_details)` clone the live
   provider out of an `RwLock<Option<Arc<dyn AiProvider>>>` slot and stream
   its output. Neither is registered in the MCP router, so external agents
   keep the exact read-only surface — identical to the write verticals
   (ADR-0062/0063/0064/0065). This is the enforcement point: read-only is
   *what is registered as a tool*, not a property of the code paths.

2. **The never-runs-SQL / never-sends-rows guardrail is preserved by what the
   commands are allowed to fetch.** Explain sends only the SQL text the user
   typed. Suggest sends the natural-language prompt plus table/column *names*
   (`list_tables`, and — when the user ticks include-details — `describe_table`
   metadata: names, types, PK). No `run_read_query` output ever reaches a
   provider. A `describe_table` that fails is not fatal: it is counted into
   `prefetch_warnings` and surfaced to the user so a partial schema is never
   silently presented as complete.

3. **Streaming uses a Tauri event; cancellation a shared `AtomicBool`.**
   Each provider `StreamEvent` is emitted as an `ai:chunk` event carrying a
   text delta and the running token counts; the frontend folds them through a
   pure `accumulate()` that appends text but **replaces** the cumulative token
   totals (the provider reports `tokens_out` cumulatively, so summing would
   double-count). A single `cancel_ai` command flips the one
   `AiState` cancel flag the in-flight stream polls; only one AI request runs
   at a time, so one flag suffices, and a new request clears it first so a
   stale cancel can't abort the next run.

4. **The API key lives only in the OS keyring; the management surface never
   returns it.** Provider CRUD (`list_ai_providers`, `add_ai_provider`,
   `update_ai_provider`, `delete_ai_provider`, `set_active_ai_provider`) is
   backed by `AiSettingsAdmin`, which writes `ai-providers.toml` for
   non-secret fields and the keyring at `dbboard.ai.<id>.api_key` for the key.
   The key is never written to TOML, never logged, never serialized back to
   the WebView — `AiProviderView` has no key field, and an edit that leaves
   the key blank keeps the stored one.

### Consequences

- The desktop gains explain-my-SQL and draft-SQL at egui parity while agents
  stay strictly read-only and no row data can leave the machine through the
  provider.
- The entry-point button is always present (outside the connection gate) so a
  first provider can be added before any connection exists; Suggest still
  requires a connection (enforced in both the frontend `canSend` guard and the
  command dispatch), while Explain does not.
- Tested RED-first: 9 backend unit tests in `dbboard-desktop` (status DTO
  shape, provider-view redaction, kind-input parse, stream accumulation
  replace-not-sum, cancel-flag clear-on-start, prefetch-warning count), and 19
  frontend unit tests on the pure `panel.ts` (`canSend`, `showIncludeDetails`,
  `accumulate` append/replace/non-mutation, `validateProvider`,
  `normalizeModel`, `providerFormForEdit`, `buildAddKindInput`). The provider
  trait and concrete providers keep their existing `dbboard-ai` coverage,
  unchanged.

## ADR-0067 — Desktop auto-update: tauri-plugin-updater + a CI-assembled `latest.json` (v0.4.0)

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0040 (the egui client's inform-only update check — the
  version-compare rules and the `DBBOARD_NO_UPDATE_CHECK` opt-out this mirrors),
  ADR-0043 (release notes as Markdown — the notes surface this reuses),
  ADR-0044 (the release build + checksums pipeline this extends, and its
  deferred OS code signing), and ADR-0059/0062–0066 (the Tauri desktop app this
  completes to v0.4.0 parity).

### Context

The egui client already *informs* about a newer release (ADR-0040): a
best-effort startup check against the GitHub Releases API that surfaces a Help
notice and the notes, with updating left entirely manual. The Tauri app had no
update path at all. This is the last v0.4.0 parity vertical, and it goes one
step further than egui: Tauri ships a first-party updater
(`tauri-plugin-updater`) that can verify a signed release and install it in
place, so the desktop app can offer **Install & Restart** rather than "go
download it yourself". The egui binary and the Tauri app are shipped
side-by-side — egui stays in production (it runs three store DBs and an
unattended Aurora DSQL consumer) — so this adds a channel, it does not retire
one.

### Decision

1. **`tauri-plugin-updater` + `tauri-plugin-process`, verifying a signed
   `latest.json`.** `tauri.conf.json` points the updater at
   `…/releases/latest/download/latest.json`, embeds the minisign **public** key,
   and sets `createUpdaterArtifacts: true` so `tauri build` emits a `.sig` next
   to each bundle. Windows installs via the NSIS setup `.exe` in `passive`
   mode; macOS ships a universal `.app.tar.gz`. The frontend calls `check()`,
   then `downloadAndInstall()`, then `relaunch()` (from the process plugin).

2. **The signing PRIVATE key never enters the repo.** Only the public key is
   committed (in `tauri.conf.json`). Signing happens in CI from the
   `TAURI_SIGNING_PRIVATE_KEY` secret (empty password). This is the minisign
   *updater* key only — it does not code-sign the binary, so OS code signing
   stays deferred (ADR-0044 §Future) and the bundles still trip SmartScreen /
   Gatekeeper on first run. Accepted trade-off: the signing key is exposed to a
   CI job that also runs untrusted crates.io build code, which is inherent to
   the updater's build-time-signing design (and exactly what `tauri-action`
   does); the blast radius is "can sign an update", bounded by the human
   holding the secret.

3. **`latest.json` is assembled by CI, not by `tauri build`.** `tauri build`
   produces the bundles and their `.sig` files but not the manifest. The
   `release.yml` publish job builds `latest.json` from the `.sig` contents plus
   the static, tag-derived download URLs — one universal macOS artifact serves
   both `darwin-x86_64` and `darwin-aarch64`. Assembly fails loudly (`one()`)
   if any bundle is missing, so a half-built release can never publish a
   manifest that points at a nonexistent asset. No third-party release action
   (ADR-0044): the runner-bundled `gh` CLI publishes, and — fixing the failure
   mode recorded during the egui release work — the publish job now bootstraps
   the release object (`gh release view … || gh release create …`) before
   `gh release upload`, which 404s on a tag that has no release yet.

4. **Opt-out and the newer-guard mirror egui (ADR-0040).** A new
   `update_opt_out` command honours the same `DBBOARD_NO_UPDATE_CHECK` env knob,
   so one variable silences both binaries. The pure `notice.ts` re-implements
   egui's `parse_version`/`is_newer` (tolerate a leading `v`, fill missing
   components with 0, drop pre-release/build metadata, treat any unparseable tag
   as "not newer") as a defensive guard: even though the plugin gates on
   version, a misconfigured endpoint offering a same/older build never nags. The
   notice is a non-modal corner card, never a blocking dialog.

### Consequences

- The desktop app reaches v0.4.0 with full egui feature parity **plus**
  in-place auto-update; the coordinated version bump moves the whole workspace
  0.3.0 → 0.4.0 so both binaries and every crate share one release number.
- The notes are rendered as plain preformatted text rather than Markdown
  (ADR-0043 uses `egui_commonmark`, which has no WebView analogue here without a
  new dependency, deliberately avoided under the package-manager supply-chain
  policy). Rich Markdown notes are a possible follow-up, not a blocker.
- **Human handoff required before the first signed release:** set the GitHub
  Actions secret `TAURI_SIGNING_PRIVATE_KEY` to the generated minisign private
  key with an empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Until then the
  `build-tauri-*` jobs fail (updater artifacts cannot be signed). The public
  key is already embedded, so an unsigned/mismatched build would be rejected by
  clients anyway — signing is not optional for this channel.
- Tested RED-first: 15 frontend unit tests on the pure `notice.ts`
  (`normalizeVersion`, `parseVersion` incl. pre-release/empty rejection,
  `isNewer` incl. the never-phantom guard, `foldDownload` start/progress/finish
  non-mutation, `downloadPercent` rounding/clamp/indeterminate) plus a Rust unit
  test pinning the `update_opt_out` policy. The download-and-install path itself
  is thin glue over the plugin and is exercised through the UI.

## ADR-0068 — MySQL adapter: a fourth engine and the first genuinely new SQL dialect

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0012 (the `DatabaseAdapter` trait this implements),
  ADR-0028 (the `describe_table` column/PK contract), ADR-0046 (the read-only
  transaction guarantee for the MCP/AI surface), ADR-0049/0050/0051 (the logical
  dump/restore contract and the per-dialect value literals), ADR-0054 (the
  `foreign_keys` edge contract), and the write-back dialect rules in
  `dbboard-core::write_back` (ADR-0042). Unlike the Postgres-wire family
  (ADR-0018/0019/0021), MySQL is not another flavor of an existing dialect.

### Context

A maintainer uses MySQL at work and asked for it as a first-class engine. Every
adapter so far has been either SQLite-wire (Turso/libSQL, D1) or Postgres-wire
(CockroachDB, Neon, Supabase, Aurora DSQL) — MySQL is the first engine whose SQL
text differs enough to need its own `SqlDialect` variant. The mandate was full
parity, not a read-only preview: connect, query, introspection
(`list_tables`/`describe_table`/`foreign_keys`/`table_ddl`), inline cell
write-back, CSV/TSV export, logical dump, atomic restore, the read-only MCP/AI
surface, and the desktop connection-manager UI — every vertical the other
adapters already satisfy.

### Decision

1. **A new `SqlDialect::MySql`, not a reuse of an existing one.** MySQL quotes
   identifiers with back-ticks (`` `x` ``, doubling an embedded back-tick), not
   double-quotes; escapes string literals with both back-slash *and* doubled
   single-quote (`'a\\b''c'`); cannot store `NaN`/`±Inf` in a `DOUBLE`, so those
   dump as `NULL`; and shares SQLite's `X'…'` hex blob literal. These live as
   `SqlDialect::MySql` arms in `dbboard-core` (`write_back`, `dump::literal`),
   and `read_only`/`restore::plan` map it to sqlparser's `MySqlDialect` so the
   AST read-only guard parses MySQL grammar. The wire/adapter id is the string
   `"mysql"` throughout.

2. **`dbboard-mysql`, a sibling adapter crate over `sqlx`'s MySQL driver.** It
   depends on `dbboard-core` only, mirroring the Postgres adapter's structure:
   a `MySqlConfig { url }` secret that never lands in `Debug`/`DbError`, a
   pool built through `harden_ssl_mode` (a bare `mysql://…` is upgraded off
   `Disabled`), and `classify_error` reducing every driver error to a fixed
   string so a URL password cannot leak. Introspection reads
   `information_schema` bound through the prepared protocol (`COALESCE(?,
   DATABASE())` — an unqualified `TableInfo` resolves to the connection's single
   database); `table_ddl` uses `SHOW CREATE TABLE` with a back-tick-quoted
   identifier. The `query` path uses the text protocol, so every value arrives
   as `Value::Text` (NULL as `Value::Null`), consistent with the Postgres
   adapter.

3. **Read-only enforced at the engine, not just the AST.** `query_read_only`
   opens a `SET TRANSACTION READ ONLY` transaction (next-transaction scope, no
   SESSION/GLOBAL leak) behind the pre-connection `classify_read_only` guard, and
   sets a session `max_execution_time` backstop. A plain query is capped by
   streaming and dropping after `max_rows` rather than wrapping arbitrary SQL in
   a `LIMIT` subquery (which breaks on duplicate output columns); EXPLAIN runs
   directly. The sqlx `Transaction` rolls back on drop, so an early return never
   strands a pooled connection mid-transaction. Restore runs as one InnoDB
   transaction; the logical dump is data-only (INSERTs), so MySQL's
   DDL-implicit-commit does not break the all-or-nothing guarantee behind
   `has_atomic_restore`.

4. **The variant is threaded top to bottom, compiler-guided.**
   `ConnectionKind::MySql { keyring_url_ref }` (config) → `BackendConfig::MySql`
   + `DBBOARD_MYSQL_URL` env resolution (connect) → the connection-manager add/
   edit forms (egui + the SvelteKit desktop UI) → the Tauri command DTOs → MCP
   `kind_label`. Because `#[serde(tag = "kind", rename_all = "snake_case")]`
   would render the Rust `MySql` variant as `my_sql`, every serde-tagged enum
   carrying it pins `#[serde(rename = "mysql")]` so the wire discriminator stays
   `mysql`; the untagged Rust draft/UI enums need no rename. The URL is the
   secret and lives in the OS keychain like the Postgres family; the store keeps
   only a `keyring_url_ref`. The connection-URL form field reuses the existing
   engine-neutral `connections-field-pg-url` i18n key; only a `conn-kind-mysql`
   brand label is added.

### Consequences

- dbboard supports four engine families across three dialects
  (SQLite-wire, Postgres-wire, and now MySQL). Adding a genuinely new dialect
  exercised — and validated — the `SqlDialect` seam: identifier quoting, literal
  escaping, float handling, and the read-only/restore AST all fanned out from
  one enum variant.
- A new `DBBOARD_MYSQL_URL` env var joins the resolution chain (documented in
  `docs/architecture.md`); it takes precedence over a file-store MySQL entry the
  same way `DBBOARD_PG_URL` does.
- Tested RED-first: dialect rules unit-tested in `dbboard-core`
  (`write_back`/`dump::literal` MySQL arms — back-tick quoting, back-slash +
  doubled-quote escaping, NaN/Inf → NULL, `X'…'` blobs), adapter behaviour
  unit-tested in `dbboard-mysql` (SSL hardening, identifier quoting, FK
  assembly, column parsing, error classification), config/connect propagation
  covered in their existing round-trip suites, and a live, env-gated
  `mysql_roundtrip.rs` integration test (`DBBOARD_MYSQL_URL`) covering
  connect/ping/DML/SELECT, `describe_table` with a composite PK, single +
  composite `foreign_keys`, the read-only truncating cap, and the 10_000-row
  `MAX_RESULT_ROWS` boundary (built from a four-way digit cross-join, since
  MySQL has no `generate_series` and a recursive CTE would trip
  `cte_max_recursion_depth`).

## ADR-0069 — SSH tunnel: pure-Rust local port forwarding (russh) with mandatory host-key verification

- **Status**: Accepted 2026-07-29
- **Relates to**: ADR-0013 (the `connections.toml` + OS-keychain store this
  extends), ADR-0016 (the `ConnectionAdmin` add/update/delete secret-committal
  order the SSH secrets ride on), ADR-0034 (the rustls-**ring** / no-aws-lc-rs
  crypto-backend constraint that gates the SSH crate choice), ADR-0046 (the
  connection factory `connect_adapter` the tunnel wraps), and ADR-0068 (MySQL,
  the engine that surfaced the need — work databases reachable only through a
  bastion).

### Context

The maintainer's work MySQL/MariaDB databases live on VPS hosts and are bound to
the server's `localhost` — they are reachable only by first opening an SSH
connection to the box and forwarding a local port to `127.0.0.1:3306` on the far
side. Every such database is registered in HeidiSQL as an "SSH tunnel" session
(plink + a key file, local port → remote `127.0.0.1:3306`). dbboard could only
accept a `mysql://…`/`postgres://…` URL pointed at an already-reachable host, so
these databases were simply unusable without a second tool holding the tunnel
open. For dbboard to be the maintainer's actual daily client (ADR-0068's stated
goal) it has to open the tunnel itself.

This is the first network-facing credential path dbboard adds that is *not* a
database URL: an SSH private key (possibly passphrase-protected), an optional
SSH password, and — critically — a **server host key** that must be verified or
the tunnel is a silent man-in-the-middle foothold.

### Decision

1. **A new `dbboard-tunnel` crate over `russh` 0.62 (pure Rust), not a shell-out
   to `ssh`/`plink`.** russh gives a self-contained SSH client with no external
   binary dependency, matching the pure-Rust posture of the rest of the tree
   (rustls, libsql-core, age). Shelling out to the system `ssh` — or bundling
   plink as HeidiSQL does — would make the tunnel depend on an out-of-band binary
   whose presence, version, and host-key store dbboard cannot control, and would
   reintroduce exactly the "second tool" problem this ADR removes. `russh` folds
   its former `russh-keys` crate into `russh::keys` (since 0.50) and carries its
   own `ssh-key` fork, so the dependency is the single `russh = "0.62"` line.
   Its crypto rides on `ring` (already in the tree via rustls-ring), so it adds
   **no** `aws-lc-rs`, honouring ADR-0034 — verified with `cargo tree`. The
   crate depends on `russh` + `tokio` only; it does **not** depend on
   `dbboard-core`, so it stays a leaf utility with no knowledge of adapters.

2. **Host-key verification is mandatory; blind-accept is not an option in the
   type.** russh's `check_server_key` defaults to rejecting every key, and we
   keep that safety: the `HostKeyPolicy` enum offers exactly two verifying
   modes — `Fingerprint("SHA256:…")` (pin the server key's SHA-256 fingerprint,
   deterministic and filesystem-free) and `KnownHosts(path?)` (verify against an
   OpenSSH `known_hosts`, where a *mismatch* — russh returns `Err`, the MITM
   signal — is a hard failure distinct from an *unknown* host). There is no
   `AcceptAny`/TOFU-by-default variant. To make first-time pinning usable the
   crate exposes `probe_host_key`, which connects far enough to read the server
   key fingerprint and returns it *without authenticating* — the UI shows it the
   way PuTTY/HeidiSQL show a host-key prompt, and the user pins it. This mirrors
   the ADR-0034 stance that a desktop client must fail *closed* on a bad chain,
   never wave it through.

3. **The tunnel guard is bound to the adapter's lifetime via a decorator, so
   `connect_adapter` keeps its signature.** When a resolved backend carries an
   SSH block, `connect_adapter` opens the tunnel first, rewrites the URL's
   `host:port` to the tunnel's ephemeral `127.0.0.1:<port>` local forward,
   connects the ordinary adapter through it, and wraps the pair in a
   `TunneledAdapter { inner, _tunnel }` that delegates every `DatabaseAdapter`
   method to `inner`. Dropping the returned `Arc<dyn DatabaseAdapter>` drops the
   pool first, then the tunnel — no reconnection, no dangling forward. Both
   `dbboard-server` (one adapter) and `dbboard-mcp` (a per-id cache) get tunnels
   for free with no signature change (ADR-0046).

4. **SSH config is a cross-cutting `ssh` sub-table on `ConnectionEntry`, not a
   field on each URL-bearing `ConnectionKind`.** A tunnel applies uniformly to
   every TCP engine (the Postgres-wire family + MySQL) and to none of the others
   (Turso is a local file, D1 is HTTPS-to-Cloudflare), so threading it through
   five enum variants would be five copies of the same optional. Instead
   `ConnectionEntry` grows one `#[serde(skip_serializing_if = "Option::is_none")]
   ssh: Option<SshTunnelToml>`; parse rejects an `ssh` block paired with a
   `turso`/`d1`/`aurora-dsql-iam` kind. Secrets stay out of the file exactly as
   before: the key-file **path** and the non-secret SSH host/port/user live
   inline, while the key **passphrase** and the SSH **password** are
   `keyring_*_ref` pointers resolved through the same `SecretStore` and committed
   through the same rollback-ordered `ConnectionAdmin` path (ADR-0016). A
   parallel `DBBOARD_SSH_*` env surface layers a tunnel onto whichever URL
   backend the env resolver picked, for headless/CI use and the first live test.

### Consequences

- dbboard can be the daily client for bastion-gated databases without a second
  tool; the HeidiSQL SSH-tunnel sessions map one-to-one onto dbboard
  connections.
- A new leaf crate `dbboard-tunnel` joins the workspace (documented in
  `docs/architecture.md`); `dbboard-connect` gains a `russh`-backed dependency
  transitively and a `TunneledAdapter` decorator.
- Host-key safety is enforced by construction: there is no code path that
  connects a tunnel without either a pinned fingerprint or a known_hosts match,
  and the mismatch case is surfaced distinctly from the unknown-host case.
- Tested RED-first: `SshTunnelConfig`/`HostKeyPolicy`/`SshTunnelToml` parsing and
  validation, the `host:port` URL-rewrite helper, and SHA-256 fingerprint
  formatting are pure unit tests; the actual forward (connect → verify → auth →
  `direct-tcpip` → `copy_bidirectional`) is covered by an env-gated
  (`DBBOARD_SSH_*`) integration test so CI stays offline while the maintainer can
  drive a real bastion. `serialized_toml_has_no_secret_value_keys` is extended to
  prove the SSH passphrase/password never land in the TOML.

---

## ADR-0070 — Row-producing paths pin the simple/text wire protocol

- **Status**: Accepted 2026-07-30
- **Relates to**: ADR-0019 (the sqlx-backed Postgres adapter whose `decode_cell`
  established the text-format value mapping), ADR-0021 (Aurora DSQL, the flavor
  that surfaced the bug in the field), ADR-0046 (the `query_read_only` row cap
  whose transaction body introduced the offending code path), ADR-0061 (the
  DSQL non-cursor cap branch), and ADR-0068 (the MySQL adapter that inherited
  the same shape).

### Context

Both sqlx-backed adapters map every cell to `Value::Text` holding the value's
*printed* representation — the same string the engine itself would print. That
mapping is only correct under a wire protocol that returns values in text
format: Postgres' simple query protocol (`Q`) and MySQL's `COM_QUERY`.
`PostgresAdapter::query` / `MySqlAdapter::query` use `sqlx::raw_sql`, which is
exactly that.

The read-only path added for the row cap did not. Its helpers used
`sqlx::query(sql)`, chosen deliberately: `RawSql`'s own `fetch`/`fetch_all`
bound the executor as `Executor<'e>` with a single lifetime, which trips
`implementation of Executor is not general enough` when the future has to stay
`Send` across an `#[async_trait]` boundary. The comment on `exec_in_txn`
recorded that trade-off, but the conclusion was wrong for row-producing
statements: `sqlx::query` always carries an argument list (empty or not), so it
goes through Prepare/Bind/Execute — and sqlx binds with
`result_formats: Binary`. Every cell then arrived as raw binary bytes and was
read as UTF-8.

The damage split two ways, and the silent half is the dangerous one:

- **Postgres** — `uuid`, `timestamptz`, and wide `int8` values are bytes that
  fail the UTF-8 check, so the query dies with
  `type conversion failed: invalid utf-8 sequence of 1 bytes from index 2`.
  But a binary `int4` of `1` is `00 00 00 01`, which *is* valid UTF-8: the cell
  came back as four invisible control characters instead of `"1"`, with no
  error anywhere.
- **MySQL** — `decode_cell` falls back to `Value::Blob` when the bytes are not
  UTF-8, so *nothing* errored. Numbers and datetimes simply became opaque
  blobs.

This reached a released build (v0.4.0) and affected every `SELECT` run from the
desktop query editor and every `query_read_only` call on the MCP surface, for
Postgres, Neon, Supabase, CockroachDB, Aurora DSQL, and MySQL/MariaDB alike.
The D1 and Turso adapters were unaffected — they do not go through sqlx.

It went unnoticed because the only tests that could catch it are the env-gated
live round-trips. `read_only_query_truncates_to_max_rows` already asserted
`Value::Text("1")` and would have failed on contact with a real database, but
it self-skips without `DBBOARD_PG_URL`, and a `debug_assert` guarding the
text-format invariant in `decode_cell` never ran for the same reason.

### Decision

1. **Every row-producing statement uses the simple/text protocol.** The
   read-only helpers (`fetch_via_cursor`'s `FETCH FORWARD`,
   `fetch_capped_stream`, `run_capped`) switch to `sqlx::raw_sql`.
2. **Hand `RawSql` to the executor, not the other way round.** Calling
   `conn.fetch(sqlx::raw_sql(sql))` uses `Executor`'s two-lifetime signature
   and infers cleanly, so the HRTB error that motivated `sqlx::query` does not
   arise. This is the piece the original trade-off missed.
3. **Non-row-producing statements keep the extended protocol.** The read-only
   preamble (`SET TRANSACTION READ ONLY`, `SET LOCAL statement_timeout`) and
   `exec_in_txn` on the restore path discard their results, so the binary
   result format costs nothing there. Their comments now say so explicitly
   instead of implying row paths should follow suit.
4. **The invariant is enforced at runtime, not with `debug_assert`.**
   `decode_cell` in the Postgres adapter rejects a non-`Text` `PgValueFormat`
   with a `DbError::TypeConversion` naming the cause. A silent corruption must
   not survive a release build just because the assertion was compiled out.
   The MySQL adapter cannot do the same — sqlx keeps `MySqlValueRef::format`
   `pub(crate)` — so there the regression test is the only guard.

### Consequences

- Values from the read-only path once again match what `query` returns and what
  the engine prints. No UI or MCP change was needed; the corruption was entirely
  below the `Value` boundary.
- The row cap's wording is unchanged but was always slightly optimistic: sqlx
  sends `Execute` with `limit: 0`, so the server produces the full result set
  either way and the cap is enforced by stopping the read. Switching to the
  simple protocol does not make that worse.
- Tested RED-first at the only layer that can observe it: new env-gated live
  tests (`read_only_decodes_wide_types_as_printed_text` for Postgres and
  MySQL, plus an Aurora DSQL variant covering the non-cursor branch) select an
  `int4`, a wide `int8`, and a `uuid`/`DATETIME` and assert the printed text.
  The small-`int4` assertion is the important one — it is the case that fails
  silently rather than loudly.
- Standing gap this exposes, recorded rather than fixed here: the live suites
  are the *only* coverage of value decoding, and they self-skip by default, so
  a whole class of wire-level regression can ship green. Running them against
  a real database before a release is a release-checklist item, not something
  CI can do offline.

---

## ADR-0071 — A listed table nobody can read must degrade, not fail the sweep

- **Status**: Accepted 2026-07-30
- **Relates to**: ADR-0054 (`list_relationships`, the sweep this fixes),
  ADR-0025 (the D1 adapter and its `sqlite_master` introspection), and
  ADR-0046 (the MCP read-only tool surface that exposes the view).

### Context

The desktop Structure tab was blank for **every** table of a Cloudflare D1
connection, showing only `query failed: [7500] not authorized: SQLITE_AUTH` —
including for tables that read perfectly well from the query editor.

Three separate things had to line up:

1. Every D1 database carries Cloudflare's own bookkeeping table `_cf_KV`.
   `sqlite_master` lists it, so `LIST_TABLES_SQL` returned it and the sidebar
   showed it.
2. `list_relationships` walks *every* listed table and runs
   `PRAGMA foreign_key_list` against each. The Workers SQLite authorizer denies
   any access to `_cf_%`, so that PRAGMA returns `SQLITE_AUTH`, and the `?` on
   the loop body aborted the whole call.
3. The Structure panel fetched columns, notes, and relationships with
   `Promise.all`. One rejection discarded the two results that had succeeded,
   so the panel rendered the error *instead of* the column list.

Each layer is individually defensible; together they turn one unreadable table
into a database-wide outage of a read-only view. The same shape recurs whenever
a listed table is not introspectable — a revoked grant, a table dropped between
the list and the sweep, a future engine with reserved names of its own.

### Decision

1. **Do not list what cannot be read.** `dbboard-d1`'s `LIST_TABLES_SQL`
   excludes `_cf_%` alongside `sqlite_%`, with `ESCAPE '\'` so LIKE's `_`
   wildcard cannot swallow an unrelated name such as `acf_log`.
2. **The sweep skips, it does not abort.** `list_relationships` catches a
   per-table `foreign_keys` failure, logs it at debug, and carries on.
3. **Skipping is reported, not silent.** `RelationshipView` gains
   `unreadable_tables`. "This table has no foreign keys" and "we could not
   look" are different answers, and the caller — agent or UI — is entitled to
   tell them apart.
4. **Only the load-bearing read is fatal to the panel.** The Structure panel
   uses `Promise.allSettled`; a failed column read still blanks it (there is
   nothing to show), while a failed note or relationship read costs only its
   own section and surfaces as a warning line.

### Consequences

- Decision 1 alone would have fixed the reported symptom. It is deliberately
  not the only fix: it addresses one engine's reserved prefix, while 2–4
  address the class.
- `unreadable_tables` is an additive field on an MCP tool result. Agents that
  ignore it behave as before; the desktop renders it as a warning naming the
  tables.
- Tested RED-first offline: a stub adapter seeded into the service's adapter
  cache lists two tables and denies `foreign_keys` for one, asserting the other
  table's edge still comes back and the denied table is named in
  `unreadable_tables`. A unit test pins the D1 `LIKE` pattern including its
  `ESCAPE` clause.
- Still uncovered: no test drives the Svelte panel's `allSettled` branching —
  the desktop test setup is node-environment unit tests with no component
  renderer. That gap is recorded, not closed.

---

## ADR-0072 — Generated SQL follows the connection's identifier dialect

- **Status**: Accepted 2026-07-30
- **Relates to**: ADR-0068 (the MySQL adapter, which made the previous
  assumption false) and ADR-0042 (the editable browse the generated `SELECT`
  feeds).

### Context

The desktop's SQL builders quoted every identifier as `"name"`, documented as
"what every engine we target accepts". That was true of the Postgres family,
SQLite, libSQL, and D1. MySQL reads `"orders"` as a *string literal* unless the
server runs with `ANSI_QUOTES`, so the sidebar's "select top 100" would have
generated `SELECT * FROM "shop"."orders" LIMIT 100;` — a syntax error on the
first MySQL connection anyone registers. The adapter itself already back-quotes
correctly (`qualified_ident`); only the frontend's generator did not.

The egui build has the same latent issue in `quoted_table_ref`. It is left
alone here: the desktop shell is the shipping surface and the one about to meet
a MySQL connection.

### Decision

1. `dialectForKind(kind)` maps a `ConnectionView.kind` slug onto `'ansi'` or
   `'mysql'`. Unknown and missing kinds fall back to ANSI — it is what every
   adapter except MySQL accepts, so it is the right guess for an adapter added
   after this code.
2. `quoteIdent`/`qualifiedName`/`selectTopN`/`countRows` take the dialect and
   double only that dialect's *own* quote character. A `"` inside a
   back-quoted MySQL identifier is an ordinary character and passes through.
3. The table right-click menu is built from a pure `tableMenuActions(table,
   kind)` in `$lib/sidebar/menu.ts`, not inline in the component, so which
   actions exist and what SQL each produces are unit-testable.

### Consequences

- The desktop menu regains the `SELECT COUNT(*)` entry the egui build has
  always had; a test pins the action-id list so a future edit cannot silently
  drop one again.
- Dialect selection is a lookup on the connection kind, not a runtime probe. A
  MySQL server actually running with `ANSI_QUOTES` still gets back-ticks, which
  it also accepts — the fallback direction is the safe one.

---

## ADR-0073 — Connection credentials are entered as parts, not as a hand-written DSN

- **Status**: Accepted 2026-07-31
- **Relates to**: ADR-0068 (the MySQL adapter this was first felt on) and
  ADR-0069 (the SSH tunnel that makes the host field ambiguous).

### Context

The connection form asked for the credential as a single `url` string. That is
the shape the adapters parse, so it was the shape the form collected. Every
other desktop client — HeidiSQL, DBeaver, TablePlus — asks for host, port,
user, password and database as separate fields, so a maintainer arriving with a
working session in one of those had to hand-assemble a DSN, and got no feedback
until the connection attempt failed.

Hand-assembly has two failure modes that are invisible until they bite:

- A password containing `@`, `/`, `#` or `?` is not percent-encoded, so the
  authority is cut at the wrong character and the client silently dials a
  different host.
- With an SSH tunnel (ADR-0069), the host in the DSN is the host *as seen from
  the SSH server*, almost always `127.0.0.1` — not the address you would use
  from this machine. Nothing in a single `url` box says so.

### Decision

1. `$lib/connections/dsn.ts` owns the parts: `DsnParts`, the display order
   (host → port → user → password → database, matching HeidiSQL), `composeDsn`,
   and `validateDsn`. Percent-encoding and IPv6 bracketing happen there, once,
   under test.
2. The form defaults to the field mode for every DSN-bearing kind and keeps a
   `use_url` escape hatch for pasting a provider-issued URL (Neon, Supabase and
   Aurora DSQL hand out ready-made ones). Turso and D1 are excluded — their
   credentials are not DSNs.
3. `defaultPort` fills a blank port (3306 for MySQL, 5432 otherwise) so the
   common case needs four fields, not five.
4. When `ssh_enabled` is set, the host field carries an inline hint that the
   host is resolved on the SSH server.

### Consequences

- A password with URL-significant characters now works without the user
  knowing what percent-encoding is.
- The stored credential is still one DSN string — nothing changes below the
  form, and an existing connection edited in field mode is rewritten in full,
  which the edit view states explicitly.
- A new DSN-bearing adapter gets the field mode for free; only `defaultPort`
  and `schemeFor` need a line each.

---

## ADR-0074 — Kinds that live only in `connections.toml` are disabled in the list, not refused on submit

- **Status**: Accepted 2026-07-31
- **Relates to**: ADR-0057 (Aurora DSQL IAM, the only such kind today).

### Context

Aurora DSQL (IAM) entries are declared in `connections.toml`; there is no
in-app form for them because there is no static secret to store. The backend
enforced this in `update_connection`, so pressing **Edit** on such a row opened
the form, let the user fill it in, and only then failed with a red banner. The
rule was correct and its presentation was not.

### Decision

`isEditableInApp(kindSlug)` in `$lib/connections/draft.ts` holds the list of
TOML-only backend slugs. The list row disables its Edit button and shows the
reason inline. The backend check stays — this is a UX layer over an existing
guard, not a replacement for it.

Two details are deliberate:

- The slug space is the backend's hyphenated `kind_label` (`aurora-dsql-iam`),
  not the form's underscored `ConnectionKind`. They are disjoint namespaces and
  a test pins that `aurora-dsql` is not confused with `aurora-dsql-iam`.
- An unrecognised slug is treated as **editable**. A newly added backend kind
  should surface a backend error, not be silently locked out of the UI by a
  frontend list nobody remembered to update.

### Consequences

- Delete stays enabled for these rows: removing an entry is a store operation
  that works for every kind.
- Adding a future TOML-only kind means one array entry plus its test.

---

## ADR-0075 — The SQL editor takes external documents by call, not by watching a prop

- **Status**: Accepted 2026-07-31
- **Relates to**: ADR-0060 (the CodeMirror editor) and ADR-0072 (the table menu
  whose "Count rows" entry exposed this).

### Context

`SqlEditor` took its document as a two-way bound `value` and adopted outside
changes in an effect that compared the prop against the live document. Running
"Count rows" from the table menu produced the right answer in the result grid
while the editor kept showing the seed text `SELECT 1 AS hello;` — the query
that ran was one the user could not see.

The adoption effect is only correct if it runs *after* the CodeMirror view is
built. When it does not, the dispatch is skipped, nothing records that a
document was missed, and the editor stays stale for the rest of the session. An
earlier attempt to fix the ordering by making the view reactive did not cure
the report, which is the argument against the whole approach rather than
against that particular patch: a channel whose correctness depends on framework
scheduling, and which fails silently when the schedule differs, cannot be
verified by reading it.

### Decision

1. `ExternalDoc` (`$lib/editor/external-doc.ts`) buffers one pending document.
   `null` means nothing pending; `''` is a real empty document, so no code path
   tests it for truthiness.
2. `SqlEditor` exports `setDoc(text)`, reachable through `bind:this`. It pushes
   into the buffer and flushes; `onMount` flushes again after building the view,
   so a call that arrives first is applied rather than lost.
3. `value` now seeds the initial document and carries typing back out. It is no
   longer watched — the prop comment says so, because the binding still looks
   two-way at the call site.
4. `QueryPanel` routes both of its non-keyboard writes (the sidebar request and
   a history replay) through one `setSql` helper, so there is a single place
   that can forget to notify the editor.

### Consequences

- Applying a document is unconditional: pressing the same menu entry twice
  resets an editor the user typed over in between, which is what "run this
  query" should do.
- The ordering rule is a unit test on `ExternalDoc`, not an assumption about
  effect scheduling. The remaining untested part is the two-line flush inside
  the component.
- Any future writer of the editor's contents must call `setDoc`; assigning the
  bound variable alone now visibly does nothing, instead of working by accident
  until the timing changes.

---

## ADR-0076 — The connection form fetches the SSH host key instead of demanding it

- **Status**: Accepted 2026-07-31
- **Relates to**: ADR-0069 (the SSH tunnel and its host-key policy).

### Context

`HostKeyPolicy` has no trust-on-first-use variant by design: both variants
verify. The form therefore presents "Server fingerprint" as a required field —
and offered no way to learn the value. A maintainer setting up a tunnel met a
red box, no explanation of what a host key is, and no path forward short of
knowing to run `ssh-keyscan | ssh-keygen -lf -` by hand.

`probe_host_key` — which reads the server's key without authenticating and was
written for exactly this — shipped in `dbboard-tunnel` and was wired to
nothing.

The alternative, accepting the first key seen, is what the type deliberately
refuses. The gap was never the policy; it was that a mandatory field had no
discoverable source.

### Decision

1. `probe_ssh_host_key(host, port)` exposes the existing probe as a Tauri
   command, and a **Fetch** button beside the fingerprint field fills it in.
2. The probe runs only on that click. Nothing in the app contacts a server the
   user has not pressed a button for.
3. Fetching fills the form; it does not save. The user still confirms the value
   and presses Save, so pinning stays deliberate — this is the SSH first-
   connection prompt, not TOFU behind their back.
4. `canProbeHostKey` gates the button. A port outside 1–65535 disables it rather
   than falling back to 22: pinning the fingerprint of a server you will not
   connect to is worse than an unfilled field.
5. Both policy fields gained a hint saying what host-key verification is for,
   and the note on a `connections.toml`-only row now shows the file's resolved
   path — "not editable here" needed a *where*.

### Consequences

- The desktop app now depends on `dbboard-tunnel` directly, for this one
  function. Opening the tunnel stays in `dbboard-connect`.
- A failed probe reports inline next to the field and leaves the rest of the
  form alone; it is not a save failure.
- The probe is unauthenticated, so it works before any credential is entered —
  the fingerprint can be pinned first and the key file chosen after.

## ADR-0077 — Every filesystem path in the connection form has a Browse button

- **Date**: 2026-07-31
- **Status**: Accepted

### Context

The connection form asked for three filesystem paths by making the user type
them: the Turso/SQLite database file, the SSH private key, and `known_hosts`.

Typing an absolute path by hand is the wrong ask. The path is long, the user is
usually looking at the file in Explorer while retyping it, and the failure mode
is silent — a stray quote from a "Copy as path" paste, a backslash the shell
ate, the wrong one of two similarly named keys. The error surfaces much later
as a connection failure that says nothing about the path being wrong. Every
other desktop client puts a Browse button here, so its absence also reads as
the field being for something more exotic than "pick a file".

`@tauri-apps/plugin-dialog` was already a dependency — the `.dbbx` bundle
import/export uses it — so the native dialog cost nothing to add.

### Decision

1. `path`, `ssh_key_path` and `ssh_known_hosts` each get a **Browse…** button
   that opens the native single-file open dialog and writes the chosen path
   into the field.
2. What the dialog says and shows lives in `lib/connections/file-picker.ts`
   (`isPathField`, `pickerFilters`, `pickerTitle`) so it is unit-testable; only
   the `open()` call itself stays in the component.
3. Filters apply to the database file only, and always end with an all-files
   entry. An OpenSSH private key and `known_hosts` have **no** extension, so any
   filter there would hide exactly the file the dialog was opened to pick.
4. The dialog's title is the field's own label rather than a generic "Open", so
   a user with three dialogs' worth of muscle memory still knows which one this
   is.
5. Picking a file clears that field's validation error immediately — the value
   came from the filesystem, so re-flagging it as missing is noise.
6. Cancelling is not an error. `open()` resolves to `null`; the field is left
   exactly as it was.
7. The private-key field gained a hint that it wants an OpenSSH key (the file
   *without* `.pub`) and that PuTTY `.ppk` is not read. Both are mistakes the
   file dialog makes easier to commit, not harder.

### Consequences

- Path entry no longer depends on the user transcribing correctly, which
  removes a class of connection failures that reported themselves as
  authentication problems.
- The picker is deliberately file-only (`directory: false`). No current field
  wants a directory; when one does, `PathField` is where that branches.
- `pickerTitle` returns a `MessageKey`, so a new path field that forgets its
  label is a type error rather than an untranslated dialog.

## ADR-0078 — TLS is a form choice, defaulting to required, never a silent fallback

- **Date**: 2026-07-31
- **Status**: Accepted

### Context

Registering a MySQL connection through dbboard's own SSH tunnel failed with:

```
connection failed: error occurred while attempting to establish a TLS
connection: server does not support TLS
```

This is `harden_ssl_mode` (`crates/dbboard-mysql/src/lib.rs`) working as
designed. sqlx defaults an unspecified `ssl-mode` to `Preferred`, which tries
TLS and **silently continues in plaintext** when the server refuses. Both the
MySQL and Postgres adapters rewrite that default up to `Required`, because a
connection the user believes is encrypted and is not is worse than one they
knowingly turned off.

The adapters always preserved an explicit `ssl-mode=DISABLED` in the URL. The
gap was in the form: the structured host/port/user/password/database inputs
(ADR-0073) compose a URL with no query string, so there was no way to express
the choice. The only escape was to abandon the parts, switch to raw-URL entry,
and hand-write the parameter — the exact hand-assembly ADR-0073 removed.

A tunnelled connection makes this ordinary rather than exotic. Traffic between
this machine and the SSH server is already encrypted by SSH, and the database
on the far side is very often a `127.0.0.1` MySQL with TLS never configured.
Requiring TLS inside the tunnel is redundant *and*, in that setup, impossible.

### Decision

1. `DsnParts` gains `db_ssl: SslMode`, rendered as a select in the Server
   fieldset. It defaults to `require`.
2. Exactly two choices: **Required** and **Disabled**. `preferred`/`prefer` is
   not offered — that is the plaintext-fallback mode the adapters already
   refuse to ship. `verify_ca`/`verify_full` need a CA file the form has
   nowhere to put; raw-URL entry can still ask for them.
3. `require` emits **no** query parameter. The composed URL is byte-for-byte
   what it was before this option existed, and the adapter's hardening supplies
   the mode. Only `disable` is written out, so the URL says something only when
   it says something surprising.
4. MySQL and Postgres disagree on both the parameter name and the value
   spelling (`ssl-mode=disabled` vs `sslmode=disable`), and sqlx rejects a
   wrong one outright. `sslQuery` picks by scheme so no call site guesses.
5. The field's hint changes when the SSH tunnel is enabled, naming what the
   tunnel does and does not encrypt rather than repeating a generic warning.

### Consequences

- Existing stored connections are untouched: they carry no parameter, which is
  what `require` composes.
- `rewrite_to_loopback` (`crates/dbboard-connect/src/ssh.rs`) preserves the
  query when it repoints the URL at the local forward, so the choice survives
  tunnelling. This ADR depends on that; a future rewrite that rebuilds the URL
  from parts would silently re-enable TLS.
- Turning TLS off is now a two-click decision recorded in the connection, not a
  reason to fall back to hand-writing a DSN.

## ADR-0079 — The TLS select belongs to the connection, not to the entry mode

- **Date**: 2026-07-31
- **Status**: Accepted

### Context

ADR-0078 added the TLS select to the Server fieldset, but placed it inside the
structured-parts branch of the form. Opening an existing connection for editing
starts in **URL mode** — `formForEdit` sets `use_url` for every DSN kind,
because the stored DSN is a secret the backend never sends back and a blank URL
has to mean "keep it". So the person who most needed the control (someone whose
existing connection just failed with `server does not support TLS`) could not
see it at all. Their only route was to switch to separate fields and retype
every credential.

### Decision

1. The select moves outside the mode branch. TLS is a property of the
   connection, not of how its credential happened to be typed.
2. In URL mode the select is a **view of the URL text**, not a shadow copy:
   `sslModeFromUrl` reads it back and `withSslMode` rewrites it. A hand-written
   `?ssl-mode=…` is therefore never contradicted by what the select displays.
3. `withSslMode` edits the query as text instead of round-tripping through
   `URL.toString()`, which would re-serialise parts of the URL the user typed
   and did not ask to change.
4. A URL still being typed (`mysql://app@`, or anything unparseable) is handed
   back untouched. Rewriting a half-written value under the cursor is worse
   than ignoring it.
5. Both spellings (`ssl-mode`, `sslmode`) are recognised on read and removed on
   write — sqlx's MySQL parser accepts either, so writing one without clearing
   the other could leave two contradicting parameters.
6. Only an explicit `disabled`/`disable` reads as off. `required`, `verify_ca`
   and `verify_identity` all mean encrypted, and the two-value select must not
   misreport a stricter mode as the weaker one it can express.
7. When the URL box is blank on edit, the select is disabled and says why: the
   stored credential is being kept, so there is no URL here to rewrite.

### Consequences

- The parts-mode path is unchanged; `db_ssl` still drives `composeDsn`.
- Switching entry modes does not carry the TLS choice across. That matches the
  existing rule that parts mode on edit is a full replacement, and the select
  re-reads from whichever store is live, so it never shows a stale value.

## ADR-0080: The edit form asks for the same fields the add form does

**Status**: Accepted
**Date**: 2026-07-31

### Context

The add form takes host / port / user / password / database separately
(ADR-0073). The edit form opened in raw-URL mode, because the stored DSN is a
keyring secret the backend never sent back — a blank URL had to mean "keep the
stored one", and the structured parts had no way to express "keep".

That divergence reached the maintainer as a bug report about a missing feature:
"was the user/password input removed?" — followed by
「「編集」で開くと URL モードは追加の時のフォームが変わるので困りますね。」
From outside, one button led to a client that asks for five fields and another
to a client that asks for a URL. Worse, the second one silently loses the
first's guarantees: percent-encoding a password containing `@` or `/` is done
for you on add and left to you on edit.

The blocker was never the form. It was that the process holding the credential
refused to say anything at all about it, including the parts that are not
secret.

### Decision

1. `dbboard-config` gains a `dsn` module that splits a stored URL into
   `DsnParts { host, port, user, database, query }`. The type has **no password
   field** — a prefill payload built from it cannot leak one by oversight,
   because there is nowhere to put one.
2. `ConnectionAdmin::dsn_prefill(id)` is best-effort: a kind with no DSN, an
   unreadable keychain entry, or an unparseable stored value all return `None`,
   and the form opens with empty parts rather than refusing to open.
3. `ConnectionAdmin::dsn_with_stored_password(id, url)` is the strict half. The
   UI rebuilds the DSN from the parts it was shown and the stored password is
   grafted back on **inside the Rust process** — it never crosses into the
   webview in either direction. An unparseable stored value is an error
   (`ConfigError::DsnUnparseable`), not a fall-through to "no password": saving
   a working connection back without its credential would break it with no
   visible cause.
4. `update_connection` takes `keep_password`. It is the structured-input
   counterpart of the blank-secret rule the form already uses everywhere else.
5. `formForEdit` opens in parts mode whenever the backend sent parts, and falls
   back to URL mode when it did not. URL mode stays available as the escape
   hatch for what a form cannot express.
6. The stored query string travels with the parts, so a TLS choice made under
   ADR-0078 is still what the select shows when the form is reopened.

### Consequences

- Add and edit now render the same inputs for every URL-bearing kind.
- In edit + parts mode a blank password box cannot mean "remove the password" —
  it means keep. That matches every other secret in this form; removing one is
  rare enough to be worth a delete-and-re-add.
- The password is still never sent to the frontend. What changed is that the
  *non-secret* parts no longer travel with it into the keyring's shadow.
- Process note: this was reported, not noticed. Add/edit parity is now a thing
  to check before shipping a form change, not after a user hits it.

## ADR-0081 — The statement-timeout variable is probed, not assumed

- **Date**: 2026-07-31
- **Status**: Accepted

### Context

Every read-only query in the MySQL adapter installs a 30-second session
statement timeout before opening its transaction (ADR-0046 §8). It is the
cancellation backstop: an MCP client that drops a tool future only cancels the
Rust side at an await point, so the server-side timeout is what stops an
abandoned query from pinning a pooled connection.

That `SET` was hard-coded to `max_execution_time`, which exists only in MySQL
5.7.8 and later. On MariaDB — same wire protocol, same `mysql://` URL, same
adapter — the very first query fails with:

```
query failed: Unknown system variable 'max_execution_time'
```

The connection succeeds, the schema tree loads, and then nothing can be
selected. A defence-in-depth measure was breaking the feature it was
protecting.

The three servers disagree in two ways at once, so a rename is not enough:

| Server | Variable | Unit |
|---|---|---|
| MySQL 5.7.8+ | `max_execution_time` | milliseconds |
| MariaDB 10.1+ | `max_statement_time` | seconds |
| MySQL 5.6 and older | — | — |

### Decision

1. **Probe, do not detect.** The adapter tries `max_execution_time`, and on
   `ER_UNKNOWN_SYSTEM_VARIABLE` (1193) tries `max_statement_time`. A
   `SELECT VERSION()` handshake would cost a round trip on every connection to
   learn something one `SET` already reveals.
2. **Only 1193 falls through.** Any other failure is returned. A dead
   connection or an exhausted pool means the query is doomed anyway, and
   retrying another statement on it would hide the real cause behind a second,
   misleading one.
3. **A server with neither variable is not an error.** The query runs without
   the backstop. Refusing to query at all is exactly the failure this ADR
   removes, and the `SET TRANSACTION READ ONLY` guard plus the pre-connection
   AST check (`classify_read_only`) are the actual safety properties — the
   timeout is a resource-hygiene measure.
4. **The answer is cached per adapter** in an `AtomicU8`. Re-probing per query
   would put a rejected statement on the wire — a wasted round trip, and a line
   in the server's error log — for every read-only query a MariaDB user runs.
5. **The MariaDB timeout is cleared before the connection returns to the
   pool.** MySQL applies `max_execution_time` to read-only `SELECT`s only, so
   it can stay set. MariaDB's `max_statement_time` applies to *every*
   statement, so a pooled connection still carrying it would kill a later
   restore's long `INSERT` at 30 seconds. The reset uses `= DEFAULT`, which
   restores the server's own global value rather than hard-coding "no limit"
   over an administrator's setting.
6. **The unit conversion is a tested fact, not a comment.** Writing the 30 000
   ms budget verbatim into MariaDB's seconds-valued variable would ask for an
   eight-hour timeout — a silently absent backstop, the worst kind.

### Consequences

- MariaDB is now a supported target of the MySQL adapter in practice, not just
  in the module docs. It was named as compatible from the start and never was.
- The first read-only query against MariaDB pays one extra rejected `SET`.
  Every query after it costs the same as on MySQL.
- The probe is pure-function-testable (statement text, units, session scope,
  probe order, cache round-trip); only the ~15-line loop that puts those
  statements on the wire needs a live server, which the env-gated round-trip
  test covers.
- Cross-engine reminder: the Postgres adapter's `statement_timeout` has no such
  divergence, so this stays MySQL-local. A future engine sharing the MySQL wire
  protocol should extend `TimeoutStyle` rather than add a second mechanism.

## ADR-0082 — Long cell values get an editor, not a keyhole

- **Date**: 2026-07-31
- **Status**: Accepted

### Context

Inline cell editing (ADR-0042) replaced the cell's text with an `<input>` while
editing. Two things went wrong at once on a real table:

1. **The column collapsed.** With the text gone from the flow, the table's auto
   layout resized the column to the input's minimum width. Starting to edit a
   `varchar(500)` therefore made the field *narrower* than the value it was
   showing a moment earlier — roughly a dozen characters of a 500-character
   value, with no way to widen it.
2. **Nothing accounted for full-width text.** The read-only value popup opened
   at `value.length >= 40`, so 25 characters of Japanese — 50 display columns,
   long since truncated on screen — never offered one.

A `<input>` is also the wrong element for a value containing a newline: HTML's
value sanitisation strips CR and LF, so committing a multi-line value edited
inline would have silently flattened it.

### Decision

1. **The inline editor floats over the cell.** The value stays in the flow,
   hidden rather than removed, so the column keeps its width. The editor is
   absolutely positioned with `min-width: max(100%, 22rem)`: at least as wide
   as the cell, never narrower than a usable field.
2. **Long values open a full editor dialog instead** — a fixed 720px surface
   with a textarea, reached automatically on double-click, or from a `⤢` button
   in the inline editor when the value turned out to need more room than it
   first appeared to. The draft carries across; nothing is retyped.
3. **"Long" is measured in display columns, not `.length`.** `displayWidth`
   counts CJK, kana, Hangul and emoji as the two columns they occupy, and
   iterates code points so an astral character counts once. Japanese prose
   reaches the threshold at half the character count, which is exactly when it
   stops fitting.
4. **A value containing a newline always takes the dialog**, however short.
   This one is correctness, not comfort: the alternative is silent data loss.
5. **The read-only popup uses the same test**, so a truncated value opens its
   viewer at the same point regardless of script.
6. **Escape and clicking away cancel; only Apply and ∅ stage.** A dialog opened
   by a stray double-click must not be able to leave an edit behind.
7. The dialog shows a character count, counted by code point — the same unit a
   `varchar(500)` limit uses, so the number means what the column enforces.

### Consequences

- `displayWidth` / `needsWideEditor` are pure and unit-tested; the component
  keeps only wiring. The threshold constant is exported, so the tests assert
  behaviour at the boundary rather than restating a magic number.
- The inline editor can extend past the right edge of the grid viewport on a
  far-right column, where it is reachable by horizontal scroll. The `⤢` button
  is the escape hatch: the dialog is centred and never clipped.
- Blob cells are still not editable, and the primary-key columns are still held
  fixed. Nothing about which cells can be edited changed here — only the
  surface they are edited on.

---

## ADR-0083 — The sidebar splits, and popovers place themselves

- **Date**: 2026-07-31
- **Status**: Accepted

### Context

Two layout complaints from real use, both about space the window has but the
app would not give:

1. **The sidebar was a fixed 260px.** A schema-qualified table name is longer
   than that on a real database, and there was no way to trade grid width for
   list width — in either direction.
2. **The query-history popover was clipped.** It opened upward from the editor
   bar with `position: absolute`, and the tab pane it lives in scrolls: the top
   of the popover — the *newest* entries — was cut off by the pane's edge and
   unreachable. On a short window it was cut off by the viewport as well.

The second is not a styling slip that a larger `max-height` fixes. An absolutely
positioned element is clipped by any scrolling ancestor, and the anchor's
distance from the top of the window is not knowable from CSS.

### Decision

1. **A draggable divider sits between the sidebar and the main pane.** The
   sidebar's width comes from a `--sidebar-width` custom property set by the
   shell, so the sidebar component keeps owning its own styling and none of the
   drag machinery.
2. **Double-clicking the divider resets it to the default** — and *forgets* the
   stored width, so a later launch also starts at the default. Resetting the
   position but leaving the preference behind would be a lie the next restart
   exposes.
3. **The chosen width is stored unclamped; the applied width is derived.**
   Narrowing the window squeezes the sidebar to half the viewport, but widening
   it again restores what the user actually asked for. Clamping on write would
   have quietly destroyed the preference.
4. **The minimum width wins over the viewport cap.** On a window too narrow for
   both panes, a cramped sidebar beats an unreadable one; the grid can scroll.
5. **The divider is a real `role="separator"`** with `tabindex`, arrow-key
   nudges and `Home` to reset. A drag handle reachable only by mouse is not a
   control, and the pointer plumbing uses pointer capture so the drag survives
   the pointer outrunning a 7px target.
6. **Popovers anchored to a toolbar are placed with `position: fixed` and
   explicit coordinates.** `placePopover` prefers opening upward (the anchors
   live on bottom toolbars), flips below when there is not enough room, picks
   the roomier side when neither fits, and caps `max-height` to the space
   actually available. It pins the popover by the edge that touches the button,
   so a short list hugs its anchor instead of floating away from it.
7. **The popover is re-placed on resize while open**, since its fixed
   coordinates were measured against the old window.

### Consequences

- `clampSidebarWidth` / `loadSidebarWidth` / `resetSidebarWidth` and
  `placePopover` are pure and unit-tested; the components keep only wiring.
  `placePopover` is deliberately DOM-free — it takes a rect and a viewport, not
  an element — so the flip and clamp cases are testable without a browser.
- `placePopover` is written for reuse but is used by the history popover only
  for now. The result grid's own popups are anchored inside a pane that does not
  clip them, and are left alone.
- The divider is a sibling of the sidebar rather than part of it, which keeps
  `Sidebar.svelte` free of layout state. The cost is a CSS custom property as
  the contract between them, documented at both ends.

---

## ADR-0084 — Commit identity is scanned, because it cannot be edited

**Date:** 2026-07-31
**Status:** Accepted

### Context

dbboard is a public repository. `scripts/pii-scan.sh` (ADR-0055) has guarded
file *contents* since it landed, and a review of what that guard actually
covers turned up the gap: every commit in the repository carries the
maintainer's personal email address in its author and committer fields, and
the scanner had no way to see it — `git grep` reads trees, not commit objects.

The two leaks are not equally bad, and the difference is the whole argument.
A string committed into a file is removed by the next commit; the old copy
survives only in history, which is a known, bounded risk. An address in a
commit object is *part of the commit*: fixing it changes the commit's hash,
which changes every descendant hash, which requires a force-push and breaks
every existing clone. There is no "fix it in the next commit" for identity.

The tracked `.claude/` directory prompted the review and turned out to be
fine — the parts that carry other people's names (`.claude/rules/`,
`.claude/templates/`) were already ignored, and the 22 tracked files scan
clean. The real finding was the one nobody was looking at.

### Decision

1. **Identity is checked as its own mode, not as another content rule.**
   `pii-scan.sh --identity <range>` reads `%ae`/`%ce`/`%an`/`%cn` from
   `git log`. Content rules cannot reach commit metadata at all, so this is a
   separate code path rather than another entry in `advisory_rules`.
2. **Only GitHub's noreply forms are publishable.** Both the modern
   `<id>+<login>@users.noreply.github.com` and the legacy
   `<login>@users.noreply.github.com` pass; everything else fails, including
   the `user@hostname` git invents when it is unconfigured. The pattern is
   overridable via `OSS_IDENTITY_ALLOW_RE` should the repo ever leave GitHub.
3. **Identity is blocking, not advisory.** The advisory tier exists for shapes
   that synthetic fixtures also match. An author address has no fixture
   equivalent — it is either publishable or it is not.
4. **It is checked before the commit exists.** `--staged` (the pre-commit
   hook) validates `git config user.email`, because the cheapest moment to
   stop a bad identity is before the object that would carry it forever is
   written. The `--identity` range mode is the CI backstop for commits that
   arrive by other routes.
5. **Findings are redacted like every other finding.** The output names the
   commit and the field, never the address. A check whose failure output
   republishes the address would defeat itself in a public Actions log.
6. **CI scans only the commits the push or PR introduced**
   (`event.before..sha`, or the PR's `base..head`) — never a wider range.
   Existing history is uniformly non-compliant pending the one-time rewrite,
   so a wider range would be permanently red and drown the signal. This is
   the same reasoning the message scan already uses.
7. **Display names are checked against the denylist, not a pattern.** A real
   name has no shape to match; only the private denylist knows it.
8. **A mode with no dispatch branch is now a hard error.** The identity mode
   was briefly parsed but not dispatched, and the script reported `clean`
   without scanning anything. For a leak scanner that is the worst possible
   failure, so the `case` gained a `*)` arm that exits 2.

### Consequences

- Every future commit in this repository is authored by a noreply address;
  the local `user.email` was fixed at the same time as this change.
- The ~428 commits already published under the personal address are **not**
  fixed by this ADR. Removing them is a history rewrite plus a force-push —
  a human decision (CLAUDE.md: pushes are done by the human), documented in
  `docs/maintainer/history-sanitize-runbook.md`, which now covers the
  identity rewrite alongside the string replacement it already described.
  The repository has no forks and no stargazers, so a rewrite would in fact
  be effective here; that is a reason to consider it, not a reason to do it
  unilaterally.
- The pre-commit hook will now refuse to commit from a clone whose
  `user.email` has not been set. That is the intended behaviour: the failure
  message names the exact `git config` command to run.

## ADR-0085 — The identity allowlist admits GitHub's web-flow committer

**Date:** 2026-08-03
**Status:** Accepted

### Context

ADR-0084 defined the publishable-identity allowlist as GitHub's per-account
noreply forms:

```
^([0-9]+\+)?[A-Za-z0-9-]+@users\.noreply\.github\.com$
```

That is the complete set of addresses a *person* can commit under, and it is
the right rule for the author field. It is not the complete set that appears
in the committer field. A commit created through the GitHub web UI — every
"Squash and merge" included — is committed by GitHub itself under the bare
`noreply@github.com`. That address is not under `users.`, so the pattern
rejects it.

The consequence is not merely a nuisance failure. It is a check that no
configuration can satisfy: the maintainer's `git config` has no bearing on a
commit this clone never authored, so the identity step goes red on every web
merge regardless of whether anything leaked. A check that fails identically
whether or not the condition it tests for holds carries no information.

That cost was paid before it was noticed. The merge of PR #127 leaked a
personal address in the author field of `e15dcff` — a genuine finding, exactly
what ADR-0084 exists to catch. The fix (turning on **Keep my email addresses
private**) worked: the next squash commit, `d7ed16b`, was authored by the
noreply form. The identity step still failed, now on the committer, and the
two failures are indistinguishable from the job status alone. A real leak and
a false positive that had been firing on every merge since ADR-0084 landed
present as the same red X.

### Decision

Admit `noreply@github.com` as a third publishable shape:

```
^(([0-9]+\+)?[A-Za-z0-9-]+@users\.noreply\.github\.com|noreply@github\.com)$
```

Matched as a whole-string alternative, not as a suffix. `evil-noreply@github.com`
and `noreply@github.com.example.com` are both still rejected, and both are now
asserted in `--selftest` alongside the existing near-miss case.

The address is admitted for the author field as well as the committer field.
Splitting the allowlist per field would buy nothing: `noreply@github.com`
belongs to GitHub rather than to any account, so it identifies nobody wherever
it appears, and a single predicate is one thing to reason about instead of two.

### Consequences

- The identity step now distinguishes a leak from a web merge. This is the
  point of the change; the previous behaviour would have masked any future
  author leak the same way it masked this one.
- The allowlist is wider by exactly one literal string. It admits no shape that
  varies, so it cannot be widened further by an address someone chooses.
- ADR-0084's scope is unchanged. History still carries the personal address on
  every commit before the fix, and `docs/maintainer/history-sanitize-runbook.md`
  is still the only route to removing it.

---

## ADR-0086 — The desktop lib is an rlib, so its fingerprint can vary

**Date:** 2026-08-04
**Status:** Accepted

### Context

`pre-push` runs two commands in sequence:

```sh
cargo build --release
cargo test --all-features --release
```

Both recompiled `dbboard-desktop` every time, on an otherwise untouched tree.
The crate is the largest link unit in the workspace, so each recompile cost
about 42s, and the hook paid it twice — once on the way into the build, once on
the way into the test run.

The obvious suspect was `build.rs`: `tauri_build::build()` with no
`cargo:rerun-if-changed`, which is the usual cause of an unconditional rebuild.
It was wrong. Running `cargo build --release` twice in a row finishes in 1s and
compiles nothing. The rebuild is not unconditional; it appears only when the
two commands *alternate*, which is exactly the shape `pre-push` has.

`CARGO_LOG=cargo::core::compiler::fingerprint=info` names the reason without
guessing:

```
fingerprint dirty for dbboard-desktop .../ lib_target("dbboard_desktop_lib",
  ["staticlib", "cdylib", "rlib"], ...)
    dirty: UnitDependencyInfoChanged { unit: UnitIndex(148) }
```

The binary follows as a cascade (`FsStatusOutdated(StaleDependency)`); the lib
is where the decision is made. Both commands write the same file —
`target/release/.fingerprint/dbboard-desktop-<hash>/lib-dbboard_desktop_lib.json` —
and each rewrites what the other recorded.

That is the whole mechanism, and it is specific to this crate's shape. Cargo
normally separates two build configurations by hashing the unit into `-C
metadata`, which gives each configuration its own fingerprint directory. A
crate that emits `staticlib` or `cdylib` cannot take part: those artifacts have
fixed filenames a linker is expected to find, so the hash cannot vary, and
neither can the fingerprint path. Every other unit in the graph is separated
this way. Comparing the two `.fingerprint` trees confirms the scope: **1210
units compared, exactly one differs**, with nothing present in only one tree.

What differs between the configurations is real, not incidental. `--all-features`
is a no-op here — the workspace declares no features at all — but dev-dependencies
still unify into the graph, and `cargo tree -e normal,build` against
`-e normal,build,dev` shows `hyper` gaining `full` and `http2`, `hyper-util`
gaining `http2`, `slab` gaining `default`, and `tempfile` gaining `getrandom`.
Those are genuinely different dependency units, so the hashes the desktop lib
records for them genuinely differ. The bug is not that cargo noticed; it is that
both answers were being written to one slot.

### Decision

Build the desktop lib as `crate-type = ["rlib"]`.

`staticlib` and `cdylib` were carried over from the Tauri template, where they
exist so an iOS or Android host can link the library. This application is
desktop-only: `main.rs` calls `run()` and nothing else, there is no
`gen/android` or `gen/apple`, and no `#[cfg(mobile)]` appears in the source.
Dropping them costs nothing that is used and restores the metadata hash, which
gives each build configuration its own fingerprint directory.

The alternative — making the two commands agree on one feature set — was
rejected. It would mean shaping the dependency graph around a cache artifact,
and it would not survive the next dev-dependency anyone adds.

### Consequences

Measured on a warm tree, alternating the two commands as `pre-push` does:

| | before | after |
|---|---|---|
| `cargo build --release` after a test run | 42s, 1 crate recompiled | **2s, 0 recompiled** |
| `cargo test --all-features --release` after a build | 94s, 1 crate recompiled | **56s, 0 recompiled** |
| `pre-push` total | ~136s | **58s** |

The remaining 56s is test execution, which this decision does not address and
was explicitly out of scope.

Issue #130 recorded 237s (build 99s / test 138s) for the same sequence on a
loaded machine. The absolute totals move with machine load and with the
antivirus scan of the release binary; what the fix removes is a fixed unit of
work — one full recompile of the largest crate in the workspace, in each
direction — and that part does not vary.

- If mobile is ever targeted, `staticlib`/`cdylib` come back and the thrash
  comes back with them. The fix then is a separate crate for the mobile host,
  not a return to one crate serving both.
- The comment in `apps/desktop/src-tauri/Cargo.toml` records this, because the
  template's shape is what a reader would otherwise assume is correct.
- `build.rs` is unchanged. Adding `cargo:rerun-if-changed` would have been a
  plausible-looking edit that fixed nothing, and the fingerprint log is the
  reason it was not made.

## ADR-0087 — The MCP server writes, behind a per-connection flag and a closed list

**Date:** 2026-08-04
**Status:** Accepted

### Context

`dbboard-mcp` shipped read-only. Every tool went through `check_read_only`, and
the one thing that changed data — `apply_row_update` — was not exposed as a
tool at all. The reasoning at the time was that an agent holding a database
handle is an agent holding a loaded gun, and that a read-only surface is the
only one that cannot be misused.

That reasoning does not survive contact with the work. The maintainer's
statement was direct: *"MCP書き込みできないのはあかんです。alterとかcreatetable
もです。"* An agent that can read a schema but cannot add a column has to hand
the change back to a human to type, which is the entire cost the server was
meant to remove. Asked whether destructive statements were a concern, the
answer was that they are not normal work — *"通常破壊は行わないと思うので、
もんだいないでうs"* — but that privilege changes and `TRUNCATE` are a different
matter: *"granteとかremoveみたいなはなしですかね？それなら制限はあったほうが
いいです。truncateとか。"*

So the question is not "read or write". It is which writes, gated how.

An earlier draft of this design got it wrong in a way worth recording. It
proposed a single `run_write` tool with no classification, on the grounds that
the database already parses SQL and dbboard classifying it a second time would
only produce a second, wrong answer. That argument is sound about *syntax* and
useless about *policy*: the engine will happily tell you that
`GRANT ALL ON *.* TO 'agent'@'%'` is valid SQL, because it is. The engine has
no opinion about who is asking. Policy is exactly the thing dbboard has to
decide for itself, and the draft would have passed both categories the
maintainer named straight through to the server.

### Decision

Three tiers.

**1. Off by default, per connection.** `ConnectionEntry` gains
`mcp_write: bool`, default `false`. No connection writes until the operator
sets it. The flag is per connection, not global, because the same
`connections.toml` backs the desktop app and can name a production database
alongside a scratch one; a single global switch would hand DDL rights over
every entry the moment one of them needed it.

It is `skip_serializing_if = "is_false"`, so turning it on for one connection
does not rewrite a key into every other entry of an existing file. It sits
*before* `ssh` in the struct because TOML requires scalars to precede tables,
and `[connections.ssh]` is a table — a test asserts the ordering rather than
leaving it to whoever next edits the struct.

**2. An allowlist, decided by AST.** `crates/dbboard-core/src/write_policy.rs`
classifies a statement as `Data` (`INSERT`/`UPDATE`/`DELETE`/`MERGE`) or
`Schema` (`CREATE TABLE`/`VIEW`/`INDEX`/`SCHEMA`, `ALTER TABLE`) and refuses
everything else. It mirrors `read_only.rs`: single statement only, parsed not
prefix-matched, fails closed on anything unrecognised, and never echoes the SQL
back in the refusal. Batches are refused however they end, so
`UPDATE t SET a = 1; DROP TABLE t` cannot ride in on a permitted first
statement.

**3. A closed list that no flag opens.** Privilege changes (`GRANT`, `REVOKE`,
`DENY`), principal changes (`CREATE`/`ALTER`/`DROP` of a `USER`/`ROLE`/`GROUP`,
`SET PASSWORD`), `TRUNCATE`, and `DROP` are refused *permanently* — the refusal
says so, and points at the desktop app's SQL editor. There is no configuration
that turns them on.

The line between `DELETE` (permitted) and `TRUNCATE`/`DROP` (closed) is not
squeamishness about the word "destructive". A `DELETE` is row-logged and rolls
back inside a transaction; `TRUNCATE` and `DROP` are DDL that commit implicitly
on MySQL and leave nothing to undo. Privilege and principal changes are closed
for a different reason: an agent that can grant is an agent that can widen its
own reach past the connection it was handed, which makes tier 1 meaningless.

Connection CRUD — creating or editing entries in `connections.toml` — stays
closed regardless, under baseline §15. Credentials are the human's to place.

### The prefilter, and why it can only refuse

`classify_write` runs a leading-keyword check before the parser. This looks
like the string matching `read_only.rs` explicitly rejects, so the constraint
is written into the code: **the prefilter can only ever refuse; the AST is the
sole authority on what is permitted.** A wrong guess there costs a false
refusal, never a false permit.

It exists because sqlparser 0.62 does not parse every vendor spelling of
`CREATE USER` — not `CREATE USER a PASSWORD 'x'`, not `WITH PASSWORD`, not
MySQL's `'a'@'%' IDENTIFIED BY`. Those already failed closed, but with the
reason "could not be parsed", which reads as a syntax complaint and invites an
agent to retry with different syntax until something lands. The prefilter
upgrades them to a permanent refusal that names the category. A test asserts
`CREATE TABLE` and `ALTER TABLE` are not caught by it.

### Consequences

- Dump stays *outside* the write gate. Taking a backup does not change the
  database, and requiring the flag for it would mean the safest thing an agent
  can do needs the same permission as the least safe. Restore is a write and is
  gated.
- The refusal text is part of the contract. An agent that gets "refused
  permanently" should stop, not retry; one that gets a plain refusal may
  legitimately rephrase. That distinction is why `WritePolicyViolation` carries
  `is_permanent()` rather than a single opaque message.
- `read_only.rs` is untouched. The read tools keep their row cap and their own
  classifier; nothing about writes weakens what a read tool may do.
- New sqlparser `Statement` variants arrive refused by default. That is the
  intended failure direction, and the cost is a follow-up issue rather than a
  lost table.

## ADR-0088 — The MCP surface shows an alias, not the connection's real id

**Date:** 2026-08-05
**Status:** Accepted

### Context

`list_connections` returned each connection's `id` and `name` verbatim, and
both are whatever the operator typed when adding the connection. On this
maintainer's install they are the thing the connection actually is: a store
name, or a host. That string then travels wherever the agent's transcript
does — the tool result, the model provider's logs, a pasted-in bug report,
every later tool call that names the connection.

The repository already treats those strings as leakable. `scripts/pii-scan.sh`
blocks real store names from entering a tracked file or a commit message
(ADR-0055), and `connections.toml` is untracked precisely because it holds
them. The MCP surface was a hole straight through that: an id the scanner
would refuse in a commit was handed to an external model on the first tool
call.

Renaming the ids is not free. An id is the primary key for
`annotations.toml`, for `DBBOARD_CONNECTION`, and for every tool call already
written down in a saved agent thread; changing it orphans notes and breaks
whatever referenced it. And the leak is not only in the id — a connection
named `店名 本番` leaks the same fact through `name`.

### Decision

A connection may carry an optional `mcp_alias`. When it has one, **the alias
replaces both the id and the name** in everything `dbboard-mcp` hands an
agent, and the real id stops being accepted as a handle.

**1. It applies at the agent boundary only.** `ConnectionService::connections`
and the desktop app keep returning real ids, because that is what
`annotations.toml`, the connection picker, and the config file are keyed on.
The projection is two new methods — `list_agent_connections` and
`resolve_agent_handle` — and the MCP server is their only caller. Nothing
below the boundary knows an alias exists.

**2. The alias covers `name` as well as `id`.** Aliasing only the id would
leave the display name leaking the same store, which is the failure this ADR
exists to prevent. An aliased connection tells an agent one string and no
other.

**3. The real id is refused once an alias exists.** `resolve_agent_handle`
matches an aliased entry by its alias and nothing else. This is the part that
makes the change worth doing: an id learned from an older transcript cannot be
handed back and echoed into the new one. It is the same reason a rotated
credential must stop working rather than merely stop being displayed.

**4. Uniqueness is enforced across aliases *and* ids.** A handle that matched
two connections would route a query to whichever the resolver saw first, so
`ConfigError::DuplicateAlias` rejects an alias colliding with another entry's
alias or id, and a *new id* colliding with an existing alias. An entry's own
id as its own alias is allowed — it says "show my id, and only my id".

**5. Opt-in, and absent by default.** `skip_serializing_if` keeps the key out
of `connections.toml` for everyone who never sets one, and a connection
without an alias behaves exactly as before. Editing follows the `mcp_write`
precedent with one extra state: omitted keeps the stored alias, a string sets
it, and an empty string — what an emptied text input sends — clears it.

**6. A source-text test guards the wire layer.** Every tool taking a
`connection_id` opens with `self.resolve(&connection_id).await?`. The
realistic future failure is not one of the eight existing tools losing that
line, it is a ninth tool added without it — which no behavioural test of the
existing eight can catch. `every_tool_taking_a_connection_id_resolves_it_first`
reads `server.rs` as text and asserts the line is present in each.

### Consequences

- Error messages are out of scope, as the issue said. A refusal may still name
  the real id; closing that would mean threading the projection through every
  error path, and the leak this ADR closes is the one on the happy path that
  fires on *every* session.
- An operator who sets an alias must expect an agent working from an old
  transcript to fail with "connection not found" for the real id. That is the
  intended behaviour, and the tool description says the returned id is the
  only one there is.
- `dbboard-web` is unaffected: it has no MCP surface, and the HTTP contract
  keeps real ids.
- The egui client (retired under #139) has no alias input. Its edit path sends
  `None`, which means keep — so it cannot clear an alias set from the desktop
  app.

## ADR-0089 — The egui client is retired; Tauri is the only client

**Date:** 2026-08-05
**Status:** Accepted

### Context

dbboard shipped two desktop clients from the same tag. `crates/dbboard-ui` +
`apps/dbboard` is the original egui app; `apps/desktop` is the Tauri 2 +
SvelteKit app that started as a presentation-layer spike (ADR-0064) and
reached feature parity at v0.4.0 — connections, cell edit, annotations,
export, backup, restore, AI.

Parity was the condition for having this conversation, and it has already been
passed. The Tauri connection form can add and edit an SSH tunnel; egui still
requires hand-editing `connections.toml` (ADR-0069). That gap did not appear
because egui is hard to write — it appeared because every write vertical was
being built twice and the second build kept losing. v0.4.0 shipped ten release
assets, four of them a client nobody was choosing to develop against.

The layering makes the removal cheap. `dbboard-ui` was already the only crate
that touched egui, nothing depends on it except `apps/dbboard`, and the domain
crates never knew either UI existed — which is the dependency rule in
`docs/architecture.md` paying for itself.

### Decision

**1. The egui client is retired.** `crates/dbboard-ui` and `apps/dbboard` are
deleted, along with their workspace members and the `eframe` / `egui_extras` /
`egui_commonmark` dependencies. The Tauri client is the client.

**2. Release CI stops building it.** The `build-windows` / `build-macos` jobs
go, and with them `dbboard-windows-x86_64.exe`, `dbboard-<v>-x86_64.msi`, and
`dbboard-macos-universal-<v>.dmg`. `SHA256SUMS.txt` still covers everything
that remains.

**3. The download page classifies by product name, not by extension.** This
supersedes #135. The page had been keying buckets on the file extension alone,
so while both clients shipped, which build a card offered depended on the order
the Releases API returned assets in. Retirement removes the ambiguity at the
source, but releases up to v0.4.0 still carry the egui assets and the page
fetches `releases/latest`. So `bucketFor` now matches the `dbboard-desktop`
product-name prefix and treats everything else as not-a-download. The page
renders correctly against v0.4.0 today and against every release after it.

**4. The CJK font loader goes with the binary.** `load_first_cjk_font` and
`install_cjk_font` are `egui::Context` code in `apps/dbboard/src/main.rs`.
The Tauri client renders in the system webview, which resolves CJK from the OS
font stack itself. Nothing outside egui used them. This was checked rather
than assumed: that fallback chain has regressed twice.

**5. `dbboard-i18n` goes; `dbboard-server` stays.** Both lost their only
consumer in the same commit, and they are not the same case.
`crates/dbboard-i18n` held egui's message catalogues; the Tauri client carries
its own under `apps/desktop/src/lib/i18n/`, so keeping the crate would leave two
sources of truth for one set of strings. It is deleted. `crates/dbboard-server`
is the executable statement of the HTTP contract `dbboard-web` mirrors
(`docs/api-contract.md`) — a spec that compiles and is tested. Nothing in this
repo boots it any more, which makes it *look* like dead code and is exactly why
the reason is written down here, in its module doc, and in `api-contract.md`.
Removing it would be an architecture decision about the sibling contract, not
part of retiring a UI, so it is out of this ADR's scope.

**6. The download page gets advertised.** Retirement is also the moment the
project stops having two answers to "where do I get it". Being published is not
the same as being findable: an agent with web search, asked to use dbboard,
concluded it was "not a publicly available tool" and proposed alternatives —
because the repo had no homepage URL and no topics, the README buried the link
below the fold, release pages listed raw asset filenames with no guidance, and
the site had no canonical or Open Graph tags. All of that is fixed in this
change: repo `homepageUrl` + 15 topics, an above-the-fold download block in
`README.md`, `--notes` prepending the download link to every generated release
page, `robots.txt` / `sitemap.xml` / canonical + `og:` tags on the site, and the
URL in `CLAUDE.md` so every agent working in this repo can quote it.

### Consequences

- The retired client is in git history, not gone. Reviving it means reverting a
  commit, not rewriting a UI.
- Users on the egui build are not auto-migrated. It has no updater (that is
  ADR-0067, Tauri-only), so they keep running v0.4.0 until they download the
  Tauri app. The internal collector install is already on the Tauri build.
- `docs/desktop-parity.md` has finished its job — it existed to track the gap
  that this ADR closes — and is archived rather than kept as a live checklist.
- The workspace loses its `rust-version = 1.92` floor rationale: that number
  came from `egui_commonmark` 0.23 (ADR-0043). The floor is left where it is
  rather than lowered speculatively; the maintainer builds on current stable
  and no consumer is served by guessing at a new minimum.
- `dbboard-web` is unaffected. It never shared code with either client, only
  concepts (CLAUDE.md "Sibling Repository").
- The brand assets moved to `assets/` (`dbboard.ico`, `dbboard-logo-256.png`).
  They lived under the deleted binary's directory but were never egui's — the
  Tauri icon set, the download page, and `README.md` all consume them.
- Two `deny.toml` advisory ignores go with the tree they excused
  (RUSTSEC-2026-0194 / -0195). Suppressions outlive the code that needed them
  unless removal is part of the same change.
- The rewritten docs keep egui in the past tense on purpose. "Ported from the
  egui client" explains why something looks the way it does and stays; any
  sentence implying egui is a current client is now false and was rewritten.
- Discoverability is now a shipping surface with an owner. If the download URL
  moves, the places to update are: `README.md`, `CLAUDE.md`,
  `crates/dbboard-mcp/README.md`, `apps/desktop/README.md`,
  `.github/workflows/release.yml`, `site/index.html`, `site/sitemap.xml`, and
  the repo's homepage field.

## ADR-0090 — The MCP server is a released binary, not a `cargo build` step

**Date:** 2026-08-05
**Status:** Accepted

### Context

`dbboard-mcp` (ADR-0046) had documentation, a nine-tool surface, a write policy
(ADR-0087), an alias scheme (ADR-0088) — and no distribution channel. It was
absent from `tauri.conf.json` (no `externalBin`, no `resources`) and absent from
`release.yml`. The only way to obtain it was `cargo build --release -p
dbboard-mcp`, so the README's `claude mcp add dbboard -- /absolute/path/to/dbboard-mcp`
named a file that could not exist on a machine without a Rust toolchain.

This is how it surfaced. An AI agent was told by its own operating notes to use
the dbboard MCP server. Those notes covered usage and failure handling but not
where to get it; searching found nothing installable; it stopped at "it clearly
exists but I can't install it" and switched to a different tool. The failure was
not phrasing. No amount of rewriting reaches a binary that is never published.

Bundling it inside the desktop installer was considered and rejected. The agent
does not launch the app — it needs an absolute path to hand `claude mcp add`, and
burying the executable in an install tree means every setup instruction becomes a
guess about where that tree is on this machine, on this OS, for this installer
version. A standalone asset has one answer.

### Decision

1. **The release workflow publishes the MCP server as its own asset**, from the
   same tag as the desktop app: `dbboard-mcp-windows-x86_64.exe` and
   `dbboard-mcp-macos-universal` (a `lipo` fat binary), each with a checksum file.
   Two product lines, one tag.
2. **The download page does not offer it.** `bucketFor` classifies on the
   product-name prefix (ADR-0089), so `dbboard-mcp-windows-x86_64.exe` resolves to
   `null` despite ending in `.exe`, and a unit test pins that. Someone clicking
   "Download for Windows" wants a GUI, not a headless stdio server.
3. **Setup is a copy-pasteable line, not prose.** `README.md`,
   `crates/dbboard-mcp/README.md` and `site/index.html` give a concrete install
   path per OS and the exact `claude mcp add … --scope user -- <path>` that follows
   from it. An agent reading the docs can execute them without inventing a path.
4. **Credentials can be passed as environment variables.** Agents commonly operate
   under a rule that forbids writing credentials to a file, which made
   `connections.toml` a hard stop. The `DBBOARD_*` variables were already read;
   they are now documented against an `mcpServers` `env` block, with the caveat
   that `~/.claude.json` is itself a file on disk.

### Consequences

- The release job count goes from three to five. The two new jobs are
  `cargo build` only — no bundler, no signing, no notarization — so they are the
  cheapest jobs in the workflow.
- `latest.json` is untouched. The updater globs `out/*-setup.exe` and
  `out/*.app.tar.gz`; neither matches an MCP asset name.
- The MCP binaries are unsigned, like the desktop ones. macOS users need
  `xattr -d com.apple.quarantine`, which is now written down next to the download
  instruction rather than left to be discovered.
- Building from source still works and is still documented — it is now the
  fallback, not the only path.
- There is no `--use-system-ca` flag to add for corporate TLS-terminating
  proxies, and the docs say so explicitly rather than staying silent. dbboard
  reads the OS trust store for every TLS connection (ADR-0034); that is the only
  mode, so a certificate failure means the proxy CA is missing from the OS store,
  and the fix is at the OS level.
- The version skew that bites source-built MCP servers — app on one release, MCP
  built from a checkout of another — stops being the default state. Both come from
  the same tag now.

## ADR-0091 — Document stores join through the same trait, with a per-adapter read-only classifier

**Date:** 2026-08-05
**Status:** Accepted (direction; no adapter written yet)

### Context

Seven adapters ship, and every one of them speaks SQL. MongoDB sat in the
roadmap as half of a one-line stretch bullet — "Additional adapters
(PlanetScale, MongoDB)" — and Firestore was never written down anywhere. Both
were agreed verbally and neither was recorded, which is the failure mode this
log exists to prevent.

Before promising a phase, the question worth answering is what actually stands
in the way. Reading `dbboard-core`, it is four things, and only one of them is
the trait:

1. **`DatabaseAdapter::query(&self, sql: &str)`** (`adapter.rs:40`). This looks
   like the blocker and is not. The parameter is a string of *the adapter's own
   query text*; nothing in the trait parses it. MongoDB's wire protocol takes a
   command document, and Firestore's REST API takes a `StructuredQuery` — both
   are JSON, both are strings.
2. **`Value` is flat** — `Null | Integer | Real | Text | Blob` (`value.rs:17`).
   A row is a `Vec<Value>` under named columns. A document is a tree. There is
   no variant that can hold one.
3. **`read_only.rs` is `sqlparser`-based** and says so in its first line. It is
   the primary enforcement for D1 and the defence-in-depth for everything else
   (ADR-0046), and the MCP write gate (ADR-0087) is built on top of it. It
   cannot classify a Mongo command document or a Firestore request, and a
   classifier that cannot parse its input must fail closed — which would mean
   every document-store query is refused.
4. **`describe_table` assumes declared columns** (`schema.rs`). Collections have
   no declared shape. Two documents in one collection need not share a field.

### Decision

1. **No second trait, and no query IR.** A document adapter implements
   `DatabaseAdapter` and treats the `query` string as JSON in its own native
   form. Inventing a neutral query language over SQL *and* two dissimilar
   document APIs would be a translation layer nobody asked for, and it would
   put a lossy abstraction between the user and a database they already know
   how to address. The `sql` parameter name becomes wrong the day the first
   document adapter lands; it gets renamed **in that PR**, not before —
   renaming across seven adapters with nothing to justify it is churn.
2. **`Value` gains a nested variant** carrying a parsed JSON tree, which makes
   `serde_json` a real dependency of `dbboard-core` rather than a dev one. That
   is consistent with the existing carve-outs: parsing JSON is a pure data
   transformation, so the crate's no-I/O rule still holds, exactly as argued for
   `serde` and `sqlparser`. The wire form is tagged the way blobs already are
   (`$blob`, `value.rs:48`) so `dbboard-web` can round-trip it; the tag goes in
   `docs/api-contract.md` before any adapter code is written, because the
   contract is shared and the sibling repo cannot be edited from here.
3. **Read-only enforcement is per-adapter and fail-closed, never a shared
   parser.** `read_only.rs` stays SQL-only and keeps its name honest.
   - **Firestore** gets it nearly free: the REST API splits reads
     (`:runQuery`, `:batchGet`) from writes (`:commit`), so the *transport*
     is the boundary — a read-only Firestore connection can only reach the read
     endpoints, and there is no string to classify.
   - **MongoDB** does not: `runCommand` accepts any verb, so it needs an
     explicit allowlist of read commands (`find`, `aggregate`, `count`,
     `distinct`, …) with everything unlisted refused, plus a rejection of
     `$out` / `$merge`, which are aggregation *stages* that write. This mirrors
     the closed list already used by the MCP write policy (ADR-0087).
4. **Schema is sampled, and says so.** `describe_table` on a collection reads a
   bounded sample and reports the field union, together with the sample size and
   how often each field appeared. It must be visibly an inference — a UI that
   renders a sampled shape identically to a declared one teaches people to trust
   it as declared.

### Consequences

- The two adapters are not equal work. Firestore's read path is a documented
  REST surface with no query-string classification problem; MongoDB needs the
  allowlist to be right before it can be exposed to the MCP server at all.
- `Value` changing shape touches every adapter's row construction, the frontend's
  cell rendering, and the wire contract with `dbboard-web`. It lands on its own,
  ahead of either adapter, so a regression there is attributable.
- The MCP server gains document stores only after each one's read-only
  classifier is unit-tested against adversarial input, the same bar `read_only.rs`
  was held to.
- Sampling costs a read per `describe_table` call. On a large collection that is
  a bounded scan, not a full one, and the bound is the thing to make
  configurable if it becomes a problem.
- PlanetScale, the other half of the old stretch bullet, is unaffected — it is
  MySQL-compatible and reachable through the existing `dbboard-mysql` adapter.

---

## ADR-0092 — A cached connection must prove it is alive before it is handed out

**Date:** 2026-08-06
**Status:** Accepted

### Context

In use, a connection through an SSH bastion stopped working. Every subsequent
call failed with

```
connection failed: error communicating with database: expected to read 4 bytes, got 0 bytes at EOF
```

and it kept failing until the app was quit and started again. Restarting fixed
it every time, which is the tell: nothing about the database had changed, only
something this process was holding on to.

The first diagnosis — a stale pooled connection — was wrong, and worth
recording as wrong. sqlx's `PoolOptions::test_before_acquire` already defaults
to `true`, so the pool pings before lending a connection out and re-dials if
the ping fails. sqlx was doing its job.

The actual mechanism is one layer up, and has two halves:

1. **`McpService::adapter_for` caches adapters for the lifetime of the
   process.** The only eviction is `invalidate()`, called when a connection's
   configuration is written. Nothing evicts on failure.
2. **Through a bastion the cached adapter owns the tunnel.**
   `TunneledAdapter` holds the `SshTunnel` guard. When the bastion drops the
   SSH session the loopback forward dies while the guard stays held — so the
   port is still bound and still accepts, but nothing survives the far side.
   sqlx cannot heal that: it discards the dead socket and dials the *same dead
   forward*, forever.

Together they are a permanent wedge. And the reason the session was dropped in
the first place is that `dbboard-tunnel` set no keepalive at all — russh
defaults `keepalive_interval` to `None`, so an idle tunnel sends nothing and
gets reaped by sshd, a NAT table, or a load balancer.

### Decision

Three changes, at the three layers that were each individually wrong.

1. **Keep the session warm.** `dbboard-tunnel` sets `keepalive_interval` to
   30 s and `keepalive_max` to 3. Thirty seconds is inside the shortest idle
   window worth surviving (a 60 s load-balancer timeout); three misses means a
   bastion that has genuinely vanished closes the session instead of leaving a
   forward that accepts and never answers.

2. **Re-check an idle adapter before lending it out.** A cache entry now
   carries the time it last completed a round-trip. Within
   `HEALTH_CHECK_AFTER_IDLE` (30 s) it is handed out unchecked; past that it is
   pinged first, and a failed ping evicts it — which drops the tunnel guard,
   which is what lets the rebuild open a *new* forward. This is test-on-borrow
   with an idle window, the same shape sqlx uses one layer down, chosen over
   pinging every call because an interactive burst of ten queries should not
   cost ten extra round-trips.

3. **Offer the user the same thing on demand.** A `reconnect` service method,
   a `reconnect_connection` Tauri command, a reload icon on the connection pill
   and a Reconnect action in the error banner. The read path recovers on its
   own now, so this is not needed to recover — it is needed to recover *now*,
   without the user having to guess whether clicking again would have worked.

### Consequences

- The wedge becomes one failed call at worst, and usually none.
- The ping runs while holding the cache lock, so a health check briefly
  serialises other connections. Releasing the lock first would let a second
  caller take the entry this one is about to evict, which is the bug again.
- Thirty seconds is a guess in both places, deliberately the same guess. If a
  bastion is found that reaps faster, the tunnel interval is the one to lower;
  the cache window only affects how long a *known* death goes unnoticed.
- The keepalive costs one small packet per idle connection per 30 s, which is
  not worth making configurable until someone reports otherwise.
- Nothing here helps a connection that dies *during* a query. That surfaces as
  a plain error, the user sees it, and the reconnect button is right there.

---

## ADR-0093 — The Firestore adapter calls REST directly, signs with `ring`, and overrides `query_read_only`

**Date:** 2026-08-06
**Status:** Accepted

### Context

ADR-0091 settled the direction for document stores and left two questions to
answer while writing the first one: which crate, if any, to depend on instead
of calling the REST API directly, and how to test without a live Google Cloud
project or a real credential.

A third question only appeared once the code existed, and it is the one with
teeth. `DatabaseAdapter::query_read_only` has a default implementation that
runs `check_read_only(sql, SqlDialect::Postgres)` before delegating to `query`.
A Firestore query is a `StructuredQuery` in JSON. The parser cannot read it,
and a classifier that cannot parse its input fails closed — so inheriting the
default would refuse **every** Firestore query, and only on the MCP surface,
where nobody would see it until an agent asked.

### Decision

1. **Call the REST API directly; add no Firestore client crate.** The three
   endpoints this adapter needs (`:runQuery`, `:listCollectionIds`, and a
   documents `GET`) are a documented, stable HTTP surface, and `reqwest` and
   `serde_json` are already in the tree. The gRPC clients in the ecosystem
   would pull a `tonic` stack in for that, and — decisively — a client that
   exposes writes would defeat point 3 below: the read-only guarantee is that
   this crate contains no code able to build a write URL.

2. **Sign the service-account assertion with `ring`.** Access without a browser
   means the JWT-bearer grant: sign a short-lived assertion with the account's
   private key and exchange it at Google's token endpoint. `ring` 0.17 is
   already in `Cargo.lock` via rustls-ring, so this adds no new dependency, its
   RSA signing is constant-time, and it is the backend ADR-0034 already
   committed to (`aws-lc-rs` stays out). The obvious alternative, `rsa` 0.9,
   carries RUSTSEC-2023-0071 — a timing sidechannel on exactly the private-key
   operation this performs.

3. **Read-only is structural, and `query_read_only` is overridden to say so.**
   `ReadEndpoint` enumerates every URL the adapter can build and has no write
   variant; `is_read` matches exhaustively with no catch-all, so adding one
   would fail to compile until someone answered for it. Because the guarantee
   does not depend on reading the query text, the override does no
   classification at all — it runs the query and truncates. This is the single
   most important line in the crate, and it is covered by a test that names the
   trap.

4. **Tests need no cloud project and no committed key.** Every HTTP path is
   exercised against `wiremock`, and the credential tests generate a throwaway
   2048-bit key at runtime. `rsa` and `rand` are **dev-dependencies only**,
   where RUSTSEC-2023-0071 cannot reach a real credential, and `rsa` is the
   only pure-Rust RSA *keygen* in the ecosystem. A committed test key would be
   a `scripts/pii-scan.sh` blocking finding, and rightly so.

5. **A service-account token is never sent over plain HTTP.** `connect` refuses
   an `http://` base URL when service-account credentials are configured, and
   the client is built with `https_only` so an `https → http` redirect cannot
   route around it. The emulator is exempt because it issues no credential to
   leak — it accepts the fixed string `owner`.

### Consequences

- Two dev-dependencies (`rsa`, `rand`) and one production dependency (`ring`)
  that was already being compiled. `num-bigint-dig` gets `opt-level = 3` in the
  dev profile, because generating a key unoptimised turns a sub-second
  operation into tens of seconds and would make `cargo test` unpleasant.
- The read-only guarantee is now stronger than the SQL adapters': theirs rests
  on a parser being right, this one rests on a URL not existing. It is also
  narrower — it says nothing about *which* documents may be read, which is
  Firestore Security Rules' job and stays there.
- Every error path that touches the token endpoint quotes back only the OAuth
  `error` and `error_description` fields. A token endpoint's response body is
  the one place an access token is guaranteed to appear, so it is never echoed.
- `describe_table` reports each field's type together with the sample it came
  from (`string (12/20 sampled)`), because `TableSchema` has nowhere else to
  put the caveat required by ADR-0091 §4 — and the frequency doubles as the
  evidence for the `nullable` flag beside it.
- The crate is complete and tested but not yet reachable from the desktop
  client: `BackendConfig` has no Firestore variant, and the service-account
  JSON needs the same keychain handling every other secret gets. That is the
  next slice of issue 0019, deliberately separate so this one is reviewable.

---

## ADR-0094 — A blank credential is a *choice*, and a Firestore browse is not SQL

**Date:** 2026-08-06
**Status:** Accepted

### Context

ADR-0093 left the Firestore adapter complete but unreachable: nothing in
`dbboard-config`, `dbboard-connect` or the desktop client could name it. Wiring
it through the five layers every other adapter already passes through surfaced
two problems that no SQL adapter has.

The first is the credential. Every kind so far has exactly one answer to "where
is the secret?" — the OS keychain. Firestore has two, because the local
emulator has no credential at all: it accepts the fixed string `owner`. So a
blank service-account box is not an unfinished form. It is a valid, deliberate
configuration — and it collides with the meaning a blank secret box already
has on the *edit* form, where blank means "keep the stored one" (ADR-0016,
which also forbids reading a stored secret back to show it).

The second is the query text. The sidebar generates `SELECT * FROM … LIMIT n`
for a table and `SELECT COUNT(*)` beside it. Firestore takes a
`StructuredQuery` in JSON. This is not a quoting dialect the way MySQL's
back-ticks are; there is no spelling of that `SELECT` that Firestore can run.

### Decision

1. **The mode gets its own flag.** `FirestoreCredentialField` is a three-state
   enum — `Keep`, `Set(json)`, `Emulator` — matching the shape
   `SshPassphraseField` already uses for its own "there is deliberately no
   secret here" case. In the form it is `use_emulator: boolean` beside
   `service_account: string`, and the checkbox **wins** over anything left in
   the credential box. Half-applying a contradictory pair would be worse than
   either reading of it.

2. **`use_emulator` is sent back to the webview on edit; the credential is
   not.** It is a mode, not a secret — ADR-0016 forbids reading the
   service-account JSON back, and says nothing about whether one exists.
   Without the flag the edit form cannot open in the state the connection is
   actually in, which is the one thing an edit form must do.

3. **Required-ness is per kind, not per field.** `requiredFields` used to
   exclude one hardcoded name (`base_url`). Firestore adds two more optional
   fields — a blank `database_id` means the project's `(default)` database, and
   a blank `service_account` means the emulator — so the exclusion moved into
   `optionalFields(kind)`. Firestore is consequently the only kind that can be
   added with no credential of any sort.

4. **The generated query follows the connection's language, not its dialect.**
   `browseQuery(table, n, kind)` returns a `StructuredQuery` for Firestore and
   SQL for everything else; `countQuery` returns `null` for Firestore and the
   sidebar drops the menu entry. Counting in Firestore is
   `:runAggregationQuery`, a separate endpoint the read-only adapter does not
   implement — a greyed-out entry would still claim the feature exists.

5. **A Firestore browse is read-only in the grid.** Inline cell editing
   composes an `UPDATE`, so a connection that cannot be sent SQL cannot be
   edited inline, whatever its schema declares. Firestore's `describe_table`
   *does* declare a primary key (the document path), so this had to be decided
   explicitly rather than falling out of the existing no-primary-key check.

### Consequences

- The service-account key is entered in a `<textarea>`, not a masked input. It
  is a multi-line JSON document, and a paste nobody can read back is a paste
  nobody can tell went wrong. It is still keychain-stored and never read back.
- Firestore carries no DSN and cannot front an SSH tunnel; both exclusions are
  now data (`NON_DSN_KINDS`, `SSH_TUNNELABLE_KINDS`) rather than a condition
  written at each call site.
- `dialectForKind` still answers `ansi` for `firestore`. Nothing asks it any
  more on that path, and giving it a third answer would imply Firestore has a
  quoting style, which is the misconception this ADR exists to remove.
- The next adapter that is not SQL (MongoDB, issue 0020) inherits all of this:
  it needs a `STRUCTURED_QUERY_KINDS` entry and its own query builder, not a
  new set of concepts.

## ADR-0095 — MongoDB's read-only guarantee is an allowlist of commands *and* of their options

- **Status**: accepted
- **Date**: 2026-08-09
- **Context**: issue 0020, building on ADR-0091 (document stores join through
  the same trait) and ADR-0087 (the MCP write policy that sits on top)

### Context

Firestore (ADR-0093) is read-only by construction: reads and writes are
different REST endpoints, so a crate that never builds a write URL has no
classifier to get wrong. MongoDB has no such split. Every command — `find`,
`insert`, `dropDatabase` — travels the same `runCommand` path, and the wire
carries no hint about which of them mutate.

`dbboard_core::read_only` cannot help. It is `sqlparser`-based by design and
says so in its first line; a classifier that cannot parse its input must fail
closed, which here would mean refusing every MongoDB query.

So MongoDB needs a classifier of its own, and it is this adapter's
safety-critical piece: the MCP read-only surface is built on top of it.

Three properties of the command language make it more than a verb check.

- **The command name is the document's first field.** That is MongoDB's own
  rule. `serde_json::Value` sorts its map, so a classifier built on `Value`
  would read `find` from `{"filter": …, "find": …}` — a document the server
  would reject, classified as one it would accept.
- **`$out` and `$merge` are aggregation stages that write.** `aggregate` has to
  be on any useful read list, and an `aggregate` can still mutate. This is the
  same shape as `WITH x AS (DELETE … RETURNING *) SELECT * FROM x`, which is
  why the SQL classifier walks an AST instead of matching a prefix.
- **`$where`, `$function` and `$accumulator` run JavaScript on the server.**
  They do not write by themselves.

### Decision

1. **Allowlist the commands.** `find`, `aggregate`, `count`, `distinct`,
   `listCollections`, `listIndexes`. Everything else is refused, including
   commands that plainly read. The list grows one reviewed entry at a time.
   `mapReduce` is the reason this is not a denylist of writes: it reads like a
   read and writes through its `out` clause.

2. **Allowlist the *options* too, per command.** A denylist of write verbs has
   to be complete to be sound, and MongoDB adds commands. With an option
   allowlist, `{"find": …, "insert": …, "documents": …}` is refused because
   `insert` is not a `find` option — nobody has to remember that it writes.

3. **Parse with field order intact.** `CommandDoc` keeps `Vec<(String, Value)>`
   rather than a map, so "the first field" means what MongoDB means by it. This
   is a local `Deserialize` impl rather than `serde_json/preserve_order`, whose
   feature unification would change map ordering for every crate in the build.

4. **Walk the whole document for forbidden keys, at any depth.** `$out` inside
   a `$facet`, a `$lookup.pipeline` or a `$unionWith.pipeline` is still a
   write. Keys only — a document whose *value* is the string `"$out"` is
   ordinary data, and refusing it would be a false positive on real queries.

5. **Refuse server-side JavaScript deliberately.** `$where`, `$function` and
   `$accumulator` are on the forbidden list even though they do not write. This
   is the classifier behind a surface an agent drives (ADR-0087), and arbitrary
   server-side code execution is not a thing to arrive at by omission. It is
   recorded here so that re-allowing it is a decision someone makes, not a gap
   someone finds.

6. **Never echo the command in a refusal.** Same rule as
   `ReadOnlyViolation`: the reason names a category, and the constants it
   quotes (`$out`, the option list) are ours, not the caller's. A refusal is
   often logged, and a filter holds real data.

### Consequences

- A legitimate query using an option nobody has reviewed is refused. That is
  the fail-closed trade, and the refusal lists the reviewed options for that
  command so the fix is visible rather than a guess.
- The classifier is pure and I/O-free, so it is unit-tested against adversarial
  input with no cluster running — the same property that let the SQL classifier
  be reviewed on its own terms, and the reason this crate leads with it instead
  of with a driver.
- Recursion is bounded by the parse: `serde_json` refuses to build a `Value`
  nested past its own limit, so a hostile document fails while parsing rather
  than while being walked.

## ADR-0096 — The MongoDB adapter uses the official driver, and parses the command twice

- **Status**: accepted
- **Date**: 2026-08-09
- **Context**: issue 0020, building on ADR-0095 (the classifier), ADR-0091
  (document stores join through the same trait), ADR-0034 (pure-Rust TLS) and
  ADR-0055 (this project's hostnames are business-identifying)

### Context

ADR-0095 settled how a MongoDB command is judged read-only. This one settles
the two things that follow: what talks to the server, and how approved text
becomes what goes on the wire.

Firestore (ADR-0093) went to REST directly, because its wire protocol is HTTP
and JSON and a hand-written client was smaller than the generated one. MongoDB
is the opposite case: a binary wire protocol, SCRAM authentication, server
discovery and monitoring, and connection pooling. Reimplementing that would be
a much larger surface to get wrong than the driver is to depend on.

### Decision

1. **Depend on the official `mongodb` crate (3.8), with default features.**
   Its default TLS backend is `rustls-tls` = `rustls/ring`, which is exactly
   the pure-Rust stack ADR-0034 asks for. `rustls-tls-aws-lc` is the sibling
   that would pull `aws-lc-sys` and is never selected.

2. **Enable `redact-errors`.** The driver's `Redact<T>` replaces server
   addresses and hostnames in error strings with `<redacted>`. Those strings
   reach the UI, and this project's hosts are business-identifying (ADR-0055).
   It is the same call `without_url()` makes in the Firestore adapter: the
   caller already knows which connection it picked, so the host adds nothing to
   the message and costs something if it is copied into an issue.

3. **Parse the command text twice, with the same parser into two types.** The
   classifier reads it as `serde_json` — pure, driver-free, reviewable on its
   own terms (ADR-0095). What travels to the server is deserialized straight
   into a `bson::Document`, which `serde_json::Value` cannot stand in for:
   - **Field order at every depth.** `serde_json`'s map is sorted, and a
     `{"sort": {"a": 1, "b": -1}}` that arrives as `{"b": -1, "a": 1}` sorts by
     a different key.
   - **Extended JSON.** `{"_id": {"$oid": "…"}}` has to become an `ObjectId`,
     or a query by id matches nothing.

   The double parse is sound in the direction that matters: both parsers keep
   the *last* of a duplicated key, and the classifier holds duplicates in a
   `Vec` and walks all of them. The classifier's view is therefore a superset
   of what the server is asked to run.

4. **The invariant is "no caller text reaches the server unclassified", not
   "every command is classified".** The adapter's own commands — `ping`, the
   collection listing, the `find` behind `describe_table` — are constants in
   the adapter file and none of them writes. Routing them through the
   classifier would test the classifier against input we wrote, which proves
   nothing.

5. **Inject `cursor` from the classifier's option table, not from a list of
   verbs.** `aggregate` and `listCollections` are errors without a `cursor`
   option, and a caller asking for their rows meant to receive them. `find`
   streams but has no `cursor` option, and sending one makes the server refuse
   the command. So the injection asks `ReadCommand::allows_option("cursor")` —
   the same table the classifier decides with — and `find` is excluded without
   being named.

6. **Refuse a connection that names no database.** MongoDB's URI makes the
   database optional. Falling back to `admin` or `test` would run the caller's
   query somewhere they never named, which is worse than not connecting.

7. **A command the *server* turns down is `DbError::Query`; everything else is
   `DbError::Connection`.** Same line the Firestore adapter draws — the socket
   being fine is what distinguishes a bad query from a bad connection.

### Consequences

- **Known gap: the driver offers no native-roots option.** Unlike `reqwest` and
  `sqlx` elsewhere in this workspace, it trusts the bundled `webpki-roots`. A
  server whose CA is in the OS store but not in that bundle — a private CA, an
  enterprise proxy — will fail the handshake with no way to point the driver at
  the OS store. Recorded here rather than papered over; if it bites, the fix is
  a driver-level `TlsOptions` with an explicit CA file path.
- No connection timeout is configured. The workspace has no precedent for one,
  and the driver's defaults are the vendor's, so inventing a number here would
  be a guess presented as a decision.
- The crate is verified end-to-end against a real server in
  `tests/live_mongodb.rs`, ignored by default and pointed at a local container.
  Unit tests prove the crate sends what we think MongoDB accepts; that file
  proves MongoDB accepts it.

## ADR-0100 — A document cell opens as a tree, because a document *is* one

### Status

Accepted.

### Context

Firestore and MongoDB store trees. dbboard renders a result as a grid, and a
grid cell is one line, so a document arrives in the UI as
`{"customer":{"name":"Sample Customer","address":{"city":"…"}}}` — truncated at
the cell's width. Every value is present and none of them is findable. Opening
the cell showed the same single line inside a `<pre>`.

The grid is the right shape for a row-and-column result and the wrong shape for
a nested value, and the tension is not resolvable in the cell: the cell is one
line by definition.

### Decision

1. **The document cell keeps its one-line preview in the grid, and opens as an
   indented tree.** The grid stays a grid; the tree lives in the read-only
   value dialog that already existed for long text.
2. **The flattening is a module, not a component** — `src/lib/grid/tree.ts`.
   Ordering, empty containers, array indices and collapsed subtrees are where
   the subtle cases are, and they are worth testing without a component around
   them.
3. **Nodes are identified by dotted path** (`lines.0.sku`). Collapse state is a
   set of paths, so it survives a re-render and cannot drift from the tree.
4. **A collapsed container stays visible**, showing its size (`{3}`, `[2]`).
   Hiding the container along with its children would read as deleting it.
5. **Container previews are language-neutral** — `{3}`, not "3 fields". This is
   data, and it must read the same in every one of the 11 locales.
6. **A document always opens**, however short its serialisation. The width test
   that gates the dialog for text does not apply: the fault being fixed is the
   shape, not the length.
7. **Copy still copies JSON**, now pretty-printed. What the user pastes
   elsewhere should be the document, not a rendering of it.

### Consequences

- A nested document is navigable without leaving dbboard, which is what
  verification sheet 001 row No.8 asks for.
- `flattenDocument` walks the whole document on every collapse toggle. For the
  documents these stores hold that is far below the cost of a re-render; if a
  pathological document ever proves otherwise, the fix is memoisation inside
  the module, with no change to its callers.
- The tree is read-only, as the dialog already was. Editing a document is a
  separate decision: a free-text edit of a tree can leave it unparseable, which
  is why document cells are excluded from inline editing in the first place.
- Not addressed here: a raw-JSON view alongside the tree. The copy button
  already yields the raw document, and a second view would have to earn its
  place in the dialog.

## ADR-0101 — The window gets a status bar, carrying only what is not already on screen

### Status

Accepted.

### Context

The bottom edge of the window was empty: the shell was a title bar and a body,
with nothing below it. The request was for something useful there, with the
explicit condition that filler is not wanted.

Most of what a status bar traditionally shows is already on screen in dbboard,
and putting it there again would be exactly the filler that was ruled out: the
row count and the "capped at N" note live in the result toolbar, the connection
kind is in the sidebar list, and the connection name is in the top bar.

Two things were genuinely absent. **How long the last statement took is
measured nowhere in the app** — `QueryOutput` carries columns, rows, a count
and a truncation flag, and no timing at all. And **an available update was
reachable exactly once**: `UpdateNotice` was rendered from a `+page.svelte`
state that dismissing set to null, so closing the card discarded the update for
the rest of the session with no way back to it.

### Decision

1. **The status bar carries the last statement's elapsed time and the running
   version, plus a chip when an update is waiting.** Nothing else. Anything
   already visible elsewhere is excluded on the grounds that it is visible
   elsewhere.
2. **The row count stays out of it**, though every other client puts it there.
   It is two centimetres away in the result toolbar, and a status bar repeating
   the toolbar is the filler this was supposed to avoid.
3. **Elapsed time is measured in the frontend, around the `invoke` alone**
   (`src/lib/status/status.svelte.ts`). The schema read that follows a browse
   is our own bookkeeping and is not charged to the statement. A backend-side
   measurement would be more truthful about the database and would require a
   change to `QueryOutput` and every adapter that builds one; this buys most of
   the answer for none of that.
4. **A failed statement is still timed.** A query that died after thirty
   seconds is a different problem from one rejected instantly, and the error
   text alone does not distinguish them.
5. **The formatting is a module** — `src/lib/status/summary.ts`. The
   interesting part is the boundaries: a sub-millisecond query renders as
   `<1 ms` rather than `0 ms`, because "0 ms" reads as "not measured"; an
   impossible duration renders as `0 ms` rather than breaking the layout.
   Digits with unit suffixes, not words, so the bar reads the same in all 11
   locales.
6. **Update availability moves into a store** (`src/lib/update/state.svelte.ts`)
   that separates "there is an update" from "the card is closed". Dismissing
   hides the card; the chip in the status bar brings it back.

### Consequences

- The bar is 24px and always present, including before the first query, where
  it says so. A bar that appears and disappears would move the layout under the
  user.
- Timing includes the IPC round trip and JSON deserialisation, so it reads
  slightly high against the same query in a native client. It is honest about
  what dbboard took to answer, which is the number the person watching cares
  about.
- Dismissing an update no longer buries it. The trade is one persistent chip in
  a corner of the status bar, which is the quietest place in the window.
- Not addressed here: rows-per-second, a query timer that ticks while running,
  or a history of timings. The bar shows the last statement, and the run in
  flight is already announced by the Run button.

## ADR-0102 — An ENUM column is edited by picking a member, not by typing one

**Status**: Accepted (2026-08-12)

**Context**: Inline editing (ADR-0042) hands every column the same single-line
text box. For an ENUM that turns a closed set of, say, three values into a
spelling test: the member has to be typed exactly, and getting it wrong is not
caught until the UPDATE comes back rejected — or worse, on a lax server, until
a truncated-to-empty value has already landed.

The member list is available, but only in one place. MySQL reports the column
as `enum('draft','sent','paid')` in `information_schema.columns.column_type`,
which `DESCRIBE_COLUMNS_SQL` already reads into `ColumnInfo.declared_type`. The
metadata attached to a result set says only `ENUM` — the members are gone by
then. So the choices can only come from the table schema, which we already
fetch on a browse to learn the primary key.

### Decision

1. **The dropdown is sourced from `describeTable`, not from the result set.**
   `QueryPanel` computes the members in the same `try` that reads the primary
   key, and passes them to `ResultGrid` as a separate `enums` prop, because
   `EditContext` carries no column types. A failed schema read leaves it empty
   and the column reverts to text, exactly as the primary key falls back to
   read-only.
2. **Parsing the declaration is a pure, tested module** —
   `src/lib/grid/enum.ts`. The escaping is where this goes wrong: a comma
   inside a member must not split it in two, `''` is a doubled quote and not
   the end of the literal, and MySQL also accepts backslash escapes. A member
   list read half-right would offer the *wrong* values, which is worse than the
   text box it replaces.
3. **A declaration we cannot parse yields nothing at all**, and the column
   stays free text. There is no partial list: an editor that can only write a
   wrong value is not a safer editor.
4. **SET is deliberately excluded.** It holds several members at once, so a
   single-select would silently drop all but one. It keeps the text box.
5. **A stored value outside the declared members is kept**, at the head of the
   list and selected. Opening the editor on a row written before the type was
   narrowed must not rewrite it to the first member just by being opened.
6. **Both editors are covered.** The inline editor becomes a `<select>`, and
   the full-editor dialog (ADR-0082) does too when it is reached; the `⤢`
   button is dropped for enum columns, because the choices are the whole value
   space and none of them needs more room. No path leaves an enum edited as
   free text.

### Consequences

- The `∅ NULL` button stays on both editors: a nullable enum still needs a way
  to say NULL, and that is a different act from picking the empty-string
  member, which MySQL allows and which the dropdown shows as `(empty)`.
- Only MySQL benefits today, because it is the only adapter whose
  `declared_type` carries the members. Postgres named enum types report the
  type name, not its labels; wiring those would need a separate catalogue read
  and is not attempted here.
- The parser accepts the shape MySQL emits, not arbitrary SQL. That is the only
  input it ever sees — the string comes from `information_schema`, not from a
  user.
