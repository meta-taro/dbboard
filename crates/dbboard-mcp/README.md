# dbboard-mcp

A headless [MCP](https://modelcontextprotocol.io) (Model Context Protocol)
server for [dbboard](../../README.md). It hands the databases dbboard is
already configured with to an external AI agent — Claude Desktop, Claude
Code — as a small, **read-only** tool surface, served over stdio.

The agent can list connections, browse schemas, read rows, and see
dbboard's local annotations. It **cannot write**, and it **never sees a
secret**. See [ADR-0046](../../docs/decisions.md) for the design.

## What it exposes

`dbboard-mcp` reuses the exact same config and connection machinery as the
desktop GUI: the `connections.toml` entry store plus the OS keychain
(Windows Credential Manager / macOS Keychain / Linux Secret Service). It
adds no new place to keep credentials — it reads the ones dbboard already
holds.

Six read-only tools (ADR-0046 Decision 5, extended by
[ADR-0053](../../docs/decisions.md)):

| Tool | What it returns |
|---|---|
| `list_connections` | Every configured connection as `{ id, name, kind }`. **No** keyring references, URLs, or tokens — secrets are never serialized. |
| `list_tables` | The tables in a connection's database. |
| `describe_table` | One table's columns (name, type, nullability, PK flag, ordinal) and primary key. `schema` is optional (the Postgres schema namespace; omit for SQLite/libSQL/D1). |
| `search_schema` | The tables and columns across a connection whose **name** contains a case-insensitive substring — the fast "which table has the email column?" lookup, without `describe_table` on every table. Matches identifiers, not row data. Capped at 200 matched tables with a `truncated` flag. |
| `run_read_query` | The rows from a single read-only SQL statement (`SELECT` / `WITH` / `EXPLAIN`), capped at `max_rows` (default 200, hard cap 1000) with a `truncated` flag. |
| `get_annotations` | dbboard's local table/column notes ([ADR-0045](../../docs/decisions.md)) for a connection, optionally filtered to one table and/or column. |

There is no write path. Any statement that is not a single read-only
query is rejected **by the database engine**, not by string matching:

- Postgres-wire adapters run it inside `BEGIN TRANSACTION READ ONLY`.
- libSQL/Turso runs it under `PRAGMA query_only`.
- D1 classifies the statement AST.

So `DELETE`, `UPDATE`, DDL, multi-statement batches, and locking reads
(`SELECT … FOR UPDATE`) all fail at the source. See `dbboard-core`'s
`query_read_only` and the per-adapter enforcement.

## Security posture

- **Secrets stay in the keychain.** The only connection metadata that
  crosses the wire is id/name/kind (`ConnectionView`). Resolved URLs,
  tokens, and keyring references are never part of a tool result, and no
  error message embeds one.
- **Read-only is engine-enforced**, not advisory (see above).
- **Result sets are bounded.** `max_rows` is clamped to 1000; the read
  path is for reconnaissance, not bulk export, so a wide table cannot
  exhaust memory.
- **stdout is sacred.** stdout carries the JSON-RPC frames. All logging
  goes to **stderr** (`RUST_LOG`, default `info`); a single stray byte on
  stdout would corrupt the stream.

The agent is trusted to author SQL — it can read any row in any
configured database. Point `dbboard-mcp` only at connections you are
comfortable exposing to the agent read-only, and prefer a
least-privilege database role in `connections.toml` where the engine
supports one.

## Build

```sh
cargo build --release -p dbboard-mcp
# binary at target/release/dbboard-mcp(.exe)
```

## Configure Claude Desktop

Add an entry to Claude Desktop's `claude_desktop_config.json`
(`%APPDATA%\Claude\claude_desktop_config.json` on Windows,
`~/Library/Application Support/Claude/claude_desktop_config.json` on
macOS). Use the absolute path to the built binary:

```jsonc
{
  "mcpServers": {
    "dbboard": {
      "command": "C:\\path\\to\\dbboard-mcp.exe"
    }
  }
}
```

With no arguments the server reads the same per-user config the desktop
GUI uses:

- **Windows:** `%APPDATA%\dbboard\dbboard\config\connections.toml`
- **macOS:** `~/Library/Application Support/dev.dbboard.dbboard/connections.toml`
- **Linux:** `$XDG_CONFIG_HOME/dbboard/connections.toml`
  (default `~/.config/dbboard/connections.toml`)

`annotations.toml` is read from the same directory.

To point at a different config file — a curated, read-only-role subset,
say — pass `--config` or set `DBBOARD_CONFIG`; `annotations.toml` is then
taken from that file's directory:

```jsonc
{
  "mcpServers": {
    "dbboard": {
      "command": "C:\\path\\to\\dbboard-mcp.exe",
      "args": ["--config", "C:\\path\\to\\agent-connections.toml"]
    }
  }
}
```

Restart Claude Desktop after editing the config. The server also runs
under Claude Code and any other MCP client that speaks stdio.

## Run manually

```sh
# default config paths
dbboard-mcp

# explicit config
dbboard-mcp --config /path/to/connections.toml
DBBOARD_CONFIG=/path/to/connections.toml dbboard-mcp

# verbose logging (to stderr)
RUST_LOG=debug dbboard-mcp
```

The server serves on stdin/stdout until the peer disconnects or it
receives Ctrl-C.

## Layers

- `service.rs` — `McpService`, the transport-independent tool logic.
  Resolves a connection + keyring secret into a cached adapter, runs the
  six read-only operations, and enforces the row cap and secret
  redaction. Testable against a real (in-memory) adapter with no MCP
  wiring.
- `server.rs` — `DbboardMcp`, the thin `rmcp` `ServerHandler` that wraps
  each service method as a `#[tool]`, serializes results to a JSON text
  block, and maps errors onto the MCP envelope.
- `main.rs` — startup wiring: tracing to stderr, config-path resolution,
  and the stdio serve loop.

## See also

- [ADR-0046](../../docs/decisions.md) — the dbboard-mcp read-only MCP
  server decision, and [ADR-0053](../../docs/decisions.md) — the
  `search_schema` tool that extends the surface to six.
- [ADR-0045](../../docs/decisions.md) — local table/column annotations,
  surfaced by `get_annotations`.
- [`docs/connections.md`](../../docs/connections.md) — `connections.toml`
  schema and the keyring-reference layout.
- [`docs/architecture.md`](../../docs/architecture.md) — where this crate
  sits in the workspace.
