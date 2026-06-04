# dbboard-postgres

PostgreSQL-wire adapter for [dbboard](../../README.md).

This crate implements the workspace-wide `DatabaseAdapter` trait
(`dbboard-core`) over `sqlx` + `tls-rustls-ring`. One adapter handles
every Postgres-flavored server dbboard supports today: CockroachDB,
vanilla Postgres, Neon, and Supabase.

## Flavors

The crate exposes three constructors that build the same underlying pool
and surface the same SQL path, differing only in the label they report
through `DatabaseAdapter::id()`:

| Constructor | `id()` | Constant | Notes |
|---|---|---|---|
| `PostgresAdapter::connect(config)` | `"postgres"` | `FLAVOR_POSTGRES` | Generic Postgres-wire — CockroachDB, vanilla Postgres. |
| `PostgresAdapter::connect_neon(config)` | `"neon"` | `FLAVOR_NEON` | Neon managed Postgres. TLS required; the URL should include `sslmode=require`. |
| `PostgresAdapter::connect_supabase(config)` | `"supabase"` | `FLAVOR_SUPABASE` | Supabase managed Postgres. TLS required. Both the direct `:5432` endpoint and the transaction-pooler `:6543` endpoint route through this constructor — the URL itself picks which one. |

The flavor label is what the connection picker, capability output, and
ADR-0017 history records see. The wire protocol, the SQL dialect, and
the set of advertised capabilities are identical — see
[ADR-0018](../../docs/decisions.md) (Neon) and
[ADR-0019](../../docs/decisions.md) (Supabase) for the rationale.

The recipe is the same for any future pg-wire flavor: add a
`FLAVOR_<name>` constant and a `connect_<name>` constructor. No trait
churn, no new crate, no `id()` stability break for existing callers.

## TLS hardening

Whatever connection string you provide, `connect` / `connect_neon` /
`connect_supabase` upgrade `PgSslMode::Prefer` → `PgSslMode::Require`
before opening the pool. This means:

- A URL with no `sslmode` parameter still requires TLS.
- A URL with `sslmode=prefer` is silently upgraded to `require`.
- `sslmode=require` / `verify-ca` / `verify-full` are honored as-is.
- `sslmode=disable` is honored — you opted out explicitly.

Both Neon and Supabase enforce TLS at the server, so the hardening
matches their requirement; the URL should include `sslmode=require`
(or stronger) when you store it in `connections.toml`.

## Dynamic value decoding

`dbboard-core::Value` has five SQLite-shaped variants, while PostgreSQL
has a rich type system. Rather than enumerate every type, the adapter
issues every statement through `sqlx::raw_sql` (the simple query
protocol). The server returns each value in its **text** representation,
which the adapter surfaces as `Value::Text` (NULL → `Value::Null`).
This is lossless for `int8` / `numeric` and covers `uuid`,
`timestamptz`, `jsonb`, arrays, and user-defined types without per-type
decode features.

## Row cap

Every query is capped at `dbboard_core::MAX_RESULT_ROWS`. Streaming the
result set means the cap fires mid-stream rather than after buffering
everything, so a runaway `SELECT` cannot exhaust client memory before
the error surfaces.

## Tests

Unit tests cover error classification and the `information_schema`
introspection mapping. Live round-trip tests (in
`tests/pg_roundtrip.rs`) are gated on environment variables and
self-skip otherwise:

- `DBBOARD_PG_URL` — runs `select_one_round_trips`,
  `dml_and_select_round_trip`, `query_at_the_row_cap_returns_all_rows`,
  and `query_over_the_row_cap_is_a_query_error` against a real
  Postgres-wire endpoint. Point this at CockroachDB or vanilla
  Postgres.
- `DBBOARD_NEON_URL` — runs `neon_round_trip_reports_neon_flavor`
  against a real Neon database. The test asserts that `id()` returns
  `"neon"` end-to-end. The URL must include `sslmode=require`.
- `DBBOARD_SUPABASE_URL` — runs `supabase_round_trip_reports_supabase_flavor`
  against a real Supabase database. The test asserts that `id()` returns
  `"supabase"` end-to-end. The URL must include `sslmode=require`; the
  direct `:5432` host and the transaction-pooler `:6543` host are both
  valid choices.

```sh
# CockroachDB / vanilla Postgres path
DBBOARD_PG_URL=postgres://… cargo test -p dbboard-postgres -- --include-ignored

# Neon path
DBBOARD_NEON_URL='postgres://…@…neon.tech/…?sslmode=require' \
  cargo test -p dbboard-postgres -- neon_round_trip

# Supabase path (direct or pooler URL works)
DBBOARD_SUPABASE_URL='postgres://…@db.…supabase.co:5432/postgres?sslmode=require' \
  cargo test -p dbboard-postgres -- supabase_round_trip
```

All three env vars can be set together; the round-trip test functions
are independent.

## See also

- [`docs/architecture.md`](../../docs/architecture.md) — adapter layer
  in the workspace.
- [`docs/connections.md`](../../docs/connections.md) — `connections.toml`
  schema, including the `kind = "neon"` and `kind = "supabase"` entry
  shapes.
- [`docs/compatibility.md`](../../docs/compatibility.md) — supported
  Postgres / CockroachDB / Neon / Supabase versions.
- [ADR-0008](../../docs/decisions.md) — original Postgres-wire adapter
  decision.
- [ADR-0018](../../docs/decisions.md) — Neon as a flavored kind.
- [ADR-0019](../../docs/decisions.md) — Supabase as a flavored kind.
