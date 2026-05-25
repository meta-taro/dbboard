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
