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

