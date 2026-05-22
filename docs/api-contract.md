# dbboard HTTP API Contract

This document is the **canonical** definition of the dbboard HTTP API.
Both implementations conform to it:

- **Desktop** (this repo): the in-process loopback server
  `crates/dbboard-server` (axum), consumed by `crates/dbboard-ui` over
  HTTP. See [ADR-0006](decisions.md) and [ADR-0009](decisions.md).
- **Web** ([`dbboard-web`](https://github.com/meta-taro/dbboard-web)):
  the NestJS service mirrors this same surface.

Breaking changes are drafted here first and mirrored to `dbboard-web`
before either side ships against them (ADR-0004).

## Transport

- The desktop server binds **loopback only** (`127.0.0.1`) on an
  **OS-assigned ephemeral port** (`bind 127.0.0.1:0`, port read back from
  the listener). The spawning process passes the resulting base URL
  (e.g. `http://127.0.0.1:54123`) to the UI.
- Requests and responses are JSON (`Content-Type: application/json`)
  except where noted.
- The server is **unauthenticated by design** (single-user, loopback,
  ephemeral port). See the security note in ADR-0009 before widening the
  bind address or persisting the port.
- `POST /query` bodies are capped at **64 KiB**; a larger body is
  rejected with `413 Payload Too Large` before any handler runs.

## Endpoints

### `GET /health`

Liveness probe. Does not touch the database — a `200` means the HTTP
server is up and the backend connected successfully at startup.

```
200 OK
{ "status": "ok" }
```

### `GET /tables`

List user tables for the sidebar.

```
200 OK
{ "tables": [ { "schema": null, "name": "users" } ] }
```

- `tables` is an array of [`TableInfo`](#tableinfo) objects, in the order
  the adapter returns them.

### `POST /query`

Run a single SQL statement.

Request:

```
Content-Type: application/json
{ "sql": "SELECT 1 AS one" }
```

Response:

```
200 OK
{
  "columns": [ { "name": "one", "declared_type": null } ],
  "rows": [ [ 1 ] ],
  "rows_affected": 0
}
```

The body is a [`QueryResult`](#queryresult). A non-`SELECT` statement
returns an empty `rows` array and a non-zero `rows_affected`.

## Data Shapes

### `Value`

A cell value. JSON has no byte type, so blobs use a tagged object; every
other variant maps to a native JSON scalar.

| Domain variant | JSON encoding | Example |
|---|---|---|
| `Null` | `null` | `null` |
| `Integer(i64)` | number | `42` |
| `Real(f64)` | number | `3.5` |
| `Text(String)` | string | `"hi"` |
| `Blob(Vec<u8>)` | `{ "$blob": "<base64>" }` | bytes `[0, 255]` → `{ "$blob": "AP8=" }` |

- Blob base64 uses the **standard** alphabet (`+`/`/`, `=` padding).
- The `$blob` object must have exactly that one key; any other or extra
  key is a malformed value.

### `QueryResult`

```jsonc
{
  "columns": [ /* Column */ ],
  "rows": [ [ /* Value */, ... ], ... ],  // each row is a bare array
  "rows_affected": 0                       // u64
}
```

- `rows` is an array of arrays: one inner array per row, positional,
  aligned with `columns`.

### `Column`

```jsonc
{ "name": "id", "declared_type": "INTEGER" }  // declared_type may be null
```

- `declared_type` is `null` when the adapter reports no type (e.g. SQLite
  expression columns, D1 `/raw` results).

### `TableInfo`

```jsonc
{ "schema": null, "name": "users" }            // unqualified (SQLite/D1)
{ "schema": "public", "name": "accounts" }     // schema-qualified (Postgres)
```

- `schema` is `null` for engines without a schema namespace.

## Errors

Any non-2xx response carries an error envelope:

```jsonc
{ "error": { "category": "query", "message": "..." } }
```

- `message` is the bare detail string (no category prefix), so it can be
  reconstructed into a domain error without doubling the prefix.

### Categories and statuses

| `category` | HTTP status | Meaning |
|---|---|---|
| `query` | `400 Bad Request` | The SQL statement is the caller's fault. |
| `type_conversion` | `422 Unprocessable Entity` | A value cannot be represented in the domain `Value` set. |
| `connection` | `502 Bad Gateway` | The upstream database is unreachable / failed. |
| `schema` | `502 Bad Gateway` | Schema introspection failed upstream. |

An unknown `category` received by a client degrades to `query` so a
contract drift surfaces as a visible error rather than a crash.

### Request-level rejections

These come from the HTTP layer before a handler runs and do **not** use
the error envelope (the body is plain text):

| Condition | HTTP status |
|---|---|
| Body is not valid JSON | `400 Bad Request` |
| Valid JSON missing the `sql` field | `422 Unprocessable Entity` |
| `Content-Type` is not `application/json` | `415 Unsupported Media Type` |
| Body exceeds 64 KiB | `413 Payload Too Large` |
