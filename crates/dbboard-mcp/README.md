# dbboard-mcp

A headless [MCP](https://modelcontextprotocol.io) (Model Context Protocol)
server for [dbboard](../../README.md). It hands the databases dbboard is
already configured with to an external AI agent — Claude Desktop, Claude
Code — as a small tool surface, served over stdio.

dbboard is free and open source.

> **Get the server:** download `dbboard-mcp-windows-x86_64.exe` or
> `dbboard-mcp-macos-universal` from the
> **[latest release](https://github.com/meta-taro/dbboard/releases/latest)**,
> then register it in one line — see [Install](#install) and
> [Configure Claude Code](#configure-claude-code). It is a single executable
> with no runtime dependencies. The desktop app is a separate download from
> the **[download page](https://meta-taro.github.io/dbboard/)** and is not
> required to run this.
>
> Prefer prose to a reference? There is a walkthrough in Japanese:
> **[Claude Code に自分の DB を触らせる](https://zenn.dev/dokokade/articles/46b8c608715963)**.

The agent can list connections, browse schemas, read rows, take a dump,
and see dbboard's local annotations. It **never sees a secret**, and it
**cannot write** until a human opts a connection in — and not even then
for privilege changes, `TRUNCATE` or `DROP`, which no setting opens. See
[ADR-0046](../../docs/decisions.md) for the design and
[ADR-0087](../../docs/decisions.md) for the write policy.

## What it exposes

`dbboard-mcp` reuses the exact same config and connection machinery as the
desktop GUI: the `connections.toml` entry store plus the OS keychain
(Windows Credential Manager / macOS Keychain / Linux Secret Service). It
adds no new place to keep credentials — it reads the ones dbboard already
holds.

Eighteen tools (ADR-0046 Decision 5, extended by
[ADR-0053](../../docs/decisions.md),
[ADR-0054](../../docs/decisions.md),
[ADR-0087](../../docs/decisions.md),
[ADR-0107](../../docs/decisions.md),
[ADR-0108](../../docs/decisions.md),
[ADR-0109](../../docs/decisions.md),
[ADR-0116](../../docs/decisions.md) and
[ADR-0134](../../docs/decisions.md)). Seven read a database, one writes
behind a per-connection flag, one takes a backup, seven reach no
database at all — those seven read or work the running window — one
reaches nothing whatever (it names the build that is answering), and one
registers a connection without ever touching a database.

| Tool | What it returns |
|---|---|
| `list_connections` | Every configured connection as `{ id, name, kind, position }`, plus `color` and `tag` where a person has marked one. **No** keyring references, URLs, or tokens — secrets are never serialized. A connection with an `mcp_alias` shows that alias as *both* its id and its name, and the id you get back is the only one the other tools accept (see below). |
| `list_tables` | The tables in a connection's database. |
| `describe_table` | One table's columns (name, type, nullability, PK flag, ordinal) and primary key. `schema` is optional (the Postgres schema namespace; omit for SQLite/libSQL/D1). |
| `search_schema` | The tables and columns across a connection whose **name** contains a case-insensitive substring — the fast "which table has the email column?" lookup, without `describe_table` on every table. Matches identifiers, not row data. Capped at 200 matched tables with a `truncated` flag. |
| `list_relationships` | The foreign-key join graph as directed edges (`from_table.from_columns → to_table.to_columns`). With no `table`, the whole graph; with a `table`, every edge touching it on **either** side — the "how is `orders` connected?" lookup. Declared constraints only; Aurora DSQL (no FKs) returns none. Capped at 500 edges with a `truncated` flag. |
| `run_read_query` | The rows from a single read-only statement, capped at `max_rows` (default 200, hard cap 1000) with a `truncated` flag. SQL (`SELECT` / `WITH` / `EXPLAIN`) on the eight SQL engines; JSON on the two document stores — a `StructuredQuery` for `firestore`, a command document like `{"find": "users", "limit": 100}` for `mongodb`. The tool description says so, so an agent told only "SQL" does not send a `SELECT` and read the parse error as its own mistake. |
| `get_annotations` | dbboard's local table/column notes ([ADR-0045](../../docs/decisions.md)) for a connection, optionally filtered to one table and/or column. |
| `run_write` | Runs one write statement and returns the rows affected. Requires `mcp_write = true` on the connection; see the write policy below. |
| `dump_database` | Writes a logical SQL dump of a whole connection to a file and reports the path, counts, byte size, and a `complete` flag. Reads only, so it needs **no** flag — it is what an agent should call before a `run_write` it might need to undo. |
| `get_ui_locale` | dbboard's UI language as `{ locale, supported }`. `locale` is `null` when nothing has been chosen — the app then follows the OS language, so `null` is a state, not a missing value. `supported` is the codes this build ships; there is no other way to learn them. |
| `set_ui_locale` | Sets the UI language to one of those codes. Exact match: `ja-JP` and `JA` are refused where `ja` is accepted. A running window picks the change up within about a second, with no restart. **Only when asked** — this changes what someone sees on their screen. |
| `capture_window` | A PNG of the running dbboard window, as an MCP image block, plus the title and the size before and after scaling (`max_edge` defaults to 1400; a capture is never enlarged). The window is found by application name, not by title — a terminal tab called "dbboard" is not it. Fails when the app is not running or is minimised; neither is fixable by retrying. **The image is the operator's real screen**, real connection names and all: describe it, but do not paste it into an issue, a PR, or a commit without asking. |
| `set_editor_sql` | Replaces the text in the running window's query editor and brings the Query tab forward. Does not run it. Returns once the window has taken the text — a success here means it is on screen, not that the request was filed. |
| `run_query` | Presses Run in that window: executes whatever its editor holds, against the connection **the window** has selected, and returns the row count once the rows are displayed. Not a cheaper `run_read_query` — it uses the window's connection and row limit and leaves the result on someone's screen, so reach for it when what the app *displays* is the point. Fails when no connection is selected there, or when a query is already running. |
| `open_ai_panel` | Opens the AI panel, which is where the AI provider settings live. Says so when it was already open. |
| `get_server_info` | Which build is answering, as `{ name, version }`. This binary is installed by hand and never updates itself, so it can be older than the fix for whatever you are looking at — quote the version in any bug report. Deliberately carries **no** filesystem path: on Windows the config path holds the operator's OS username, and a tool result lands in the calling agent's transcript as plaintext on disk. The same version also opens the handshake `instructions`, because some clients drop those and some agents never call a tool they were not asked about. |
| `add_connection` | Registers a new connection in `connections.toml` and returns it as `{ id, name, kind }`. Only the kinds that put **nothing** in the keychain: `turso` (a SQLite/libSQL file on this machine, from a `path`) and `firestore` (the local emulator, from a `project_id` and a `base_url` — it authenticates with a fixed token, so there is no service account to save, [ADR-0093](../../docs/decisions.md)). Every other kind is refused permanently, and the refusal is in the tool's *description* so an agent reads it before sending a password rather than after. The entry is created read-only — `mcp_write` stays off and no alias is set, since an alias the agent chose would hide nothing from it. A dbboard window that is already open lists the new connection after a refresh. See [ADR-0134](../../docs/decisions.md). |
| `set_connection_mark` | Sets a connection's identity mark: a colour from dbboard's eight (`red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `pink`) and a tag of at most 12 characters. Both halves are written together, so sending one clears the other and sending neither unmarks the connection — the same all-at-once edit the app's own picker makes, because a half-sent mark and a cleared one are indistinguishable in an agent-composed JSON object. Answers with the whole list in its new state. Reaches no database, so it is **not** behind `mcp_write`: that switch is about data (ADR-0087), and putting the sidebar's colours behind it would make an operator grant production write access to have their list tidied. See [ADR-0136](../../docs/decisions.md). |
| `move_connection` | Moves a connection to a `position`, sliding the rest over. The list order is the order the sidebar renders (#192), and `position` counts from zero — it is the `position` field `list_connections` reports, not a row counted off by hand, because another window may have reordered the file since. Answers with the whole list in its new order, so a sort can be driven from the response rather than from a read that is one move stale. Reaches no database. See [ADR-0136](../../docs/decisions.md). |
| `open_ai_settings` | Opens the AI provider settings, which live *inside* that panel — so it is refused while the panel is shut, and `open_ai_panel` comes first. The refusal is the point as much as the opening: there is no top-level route to these settings, and being told the verb has no owner is how an agent learns that. Says so when they were already open. |

`run_read_query` has no write path at all. Any statement that is not a
single read-only query is rejected **by the database engine**, not by
string matching:

- Postgres-wire adapters run it inside `BEGIN TRANSACTION READ ONLY`.
- MySQL / MariaDB run it inside `SET TRANSACTION READ ONLY`.
- libSQL/Turso runs it under `PRAGMA query_only`.
- D1 classifies the statement AST.
- Firestore has no write path to reach: the adapter implements the read
  endpoints and nothing else ([ADR-0093](../../docs/decisions.md)).
- MongoDB allow-lists the command names an agent may send — `aggregate`,
  `count`, `distinct`, `find`, `listCollections`, `listIndexes` — **and the
  options each of those takes**, then walks the pipeline for a `$out`,
  `$merge`, `$where`, `$function` or `$accumulator` hidden inside a
  `$facet`, `$lookup` or `$unionWith`
  ([ADR-0095](../../docs/decisions.md)).

So `DELETE`, `UPDATE`, DDL, multi-statement batches, and locking reads
(`SELECT … FOR UPDATE`) all fail at the source. See `dbboard-core`'s
`query_read_only` and the per-adapter enforcement.

`run_write` is a SQL path and stays one. Setting `mcp_write = true` on a
`firestore` or `mongodb` connection opens nothing: neither adapter
implements `execute`, so a write fails as an unsupported capability. Both
report that in `capabilities` rather than advertising a write and then
refusing it — the two document stores are read-only for every caller, not
only for agents.

## The write policy (ADR-0087)

Writing is off. Turning it on does not turn everything on. Three tiers,
each of which must pass:

1. **The connection is opted in.** `mcp_write = true` in
   `connections.toml`, or the "Let an AI agent write to this database"
   toggle in the dbboard app. Absent means `false`, so every existing
   connection stays read-only across the upgrade.
2. **The statement is on the allowlist.** `run_write` parses the SQL to
   an AST and accepts `INSERT` / `UPDATE` / `DELETE` / `MERGE` (data) and
   `CREATE TABLE` / `CREATE VIEW` / `CREATE INDEX` / `CREATE SCHEMA` /
   `ALTER TABLE` (schema). Everything else — including harmless-looking
   things nobody listed, like `COMMENT ON` — is refused. It fails closed.
3. **The statement is not permanently closed.** No flag opens
   `GRANT` / `REVOKE` / `DENY`, user or role DDL, `SET PASSWORD`,
   `TRUNCATE`, or `DROP` of anything at all (an index included: creating
   one is permitted, dropping one is not). These are refused with a
   distinct, permanent
   reason, so an agent that hits one knows there is no setting to ask
   for.

`DELETE` is allowed and `TRUNCATE` is not, deliberately: a `DELETE` has a
`WHERE`, is transactional, and is the thing a dump can undo.

Connection CRUD is closed by design — the MCP surface cannot add, edit or
delete a connection, because that would mean handling credentials.
Restore is closed too: its plan runs statements verbatim and covers
`DROP`/`TRUNCATE`, which cannot be reconciled with an allowlist. A human
restores, in the dbboard app.

## Security posture

- **Secrets stay in the keychain.** The only connection metadata that
  crosses the wire is id/name/kind (`ConnectionView`). Resolved URLs,
  tokens, and keyring references are never part of a tool result, and no
  error message embeds one.
- **Reading is engine-enforced read-only**, not advisory (see above).
- **Writing is off until a human turns it on**, per connection, and the
  permanently-closed list holds regardless (see the write policy above).
- **Dumps never overwrite.** `dump_database` takes an absolute path whose
  parent already exists and whose file does not, created with
  `create_new` — so an agent cannot clobber a backup, including one it
  wrote a moment ago.
- **Result sets are bounded.** `max_rows` is clamped to 1000; the read
  path is for reconnaissance, not bulk export, so a wide table cannot
  exhaust memory.
- **stdout is sacred.** stdout carries the JSON-RPC frames. All logging
  goes to **stderr** (`RUST_LOG`, default `info`); a single stray byte on
  stdout would corrupt the stream.

## Which connection store it reads

Every connection this server can reach is an entry in `connections.toml`.
There is no way to hand it a connection through the environment: a tool
call names a `connection_id`, and the server resolves that id against the
store file and the OS keychain, nothing else. A store file that does not
exist is not an error — it simply means no connections, and
`list_connections` answers with an empty list.

The one thing the environment selects is **which store file**:

| Variable / flag | For |
|---|---|
| `--config <path>` | the `connections.toml` to read (wins over the variable) |
| `DBBOARD_CONFIG` | same, as an environment variable |
| `RUST_LOG` | log level on stderr (default `info`) |

```jsonc
{
  "mcpServers": {
    "dbboard": {
      "type": "stdio",
      "command": "C:/Users/<you>/AppData/Local/dbboard/dbboard-mcp.exe",
      "env": {
        "DBBOARD_CONFIG": "C:/Users/<you>/dbboard-agent/connections.toml"
      }
    }
  }
}
```

The `DBBOARD_MYSQL_URL` / `DBBOARD_PG_URL` / `DBBOARD_TURSO_PATH` /
`DBBOARD_D1_*` / `DBBOARD_SSH_*` / `DBBOARD_CONNECTION` variables
documented in [`docs/connections.md`](../../docs/connections.md) belong to
the single-connection resolution path used by `dbboard-server`. This
server does not read them. A tunnelled connection is configured by the
`[connections.ssh]` block on the entry instead, where host-key
verification is mandatory the same way.

## Behind a TLS-terminating proxy

Corporate networks that re-sign HTTPS (and AV products that do the same)
break tools which trust a bundled CA list. dbboard reads the **OS trust
store** for every TLS connection — Postgres-wire, MySQL, Cloudflare D1,
and the AI providers alike (ADR-0034) — so the proxy's CA is trusted as
soon as the machine trusts it. There is no `--use-system-ca` equivalent
to pass: it is the only mode.

If a connection still fails with a certificate error, the CA is not in
the OS store; install it there rather than looking for a dbboard flag.

## Aliasing a connection (`mcp_alias`)

An id like `db01.internal` or a name like `Acme Inc. (production)` says
something about your business, and everything this server returns goes into the agent's
transcript and its model provider's logs. Setting `mcp_alias = "store-a"`
on a connection makes `store-a` the only string an agent ever sees for it,
in place of both the id and the name — and the real id stops being accepted
as a handle, so one picked up from an earlier session cannot be handed back.
Connections without an alias are unchanged. See
[ADR-0088](../../docs/decisions.md) and
[`docs/connections.md`](../../docs/connections.md).

Error messages are out of scope: a refusal may still name the real id.

The agent is trusted to author SQL — it can read any row in any
configured database. Point `dbboard-mcp` only at connections you are
comfortable exposing to the agent read-only, and prefer a
least-privilege database role in `connections.toml` where the engine
supports one. Set `mcp_write` on top of that only where you would accept
the agent editing rows and schema unattended; the database role is still
the outer bound, and it is the one an engine enforces.

## Install

The server is **one self-contained executable**. There is no npm package,
no installer, and no runtime to install alongside it — download the file,
put it somewhere stable, and point your agent host at that path.

Get it from the [latest release](https://github.com/meta-taro/dbboard/releases/latest):

| Platform | Asset |
|---|---|
| Windows x64 | `dbboard-mcp-windows-x86_64.exe` |
| macOS (Intel + Apple silicon) | `dbboard-mcp-macos-universal` |

The desktop app's installer does **not** contain it. They are separate
products from the same tag: the app is for a human, this is for an agent.

Where to put it — the paths the rest of this README assumes:

```sh
# Windows
mkdir %LOCALAPPDATA%\dbboard
#   → %LOCALAPPDATA%\dbboard\dbboard-mcp.exe
#     i.e. C:\Users\<you>\AppData\Local\dbboard\dbboard-mcp.exe

# macOS
mkdir -p ~/.local/bin
mv ~/Downloads/dbboard-mcp-macos-universal ~/.local/bin/dbboard-mcp
chmod +x ~/.local/bin/dbboard-mcp
xattr -d com.apple.quarantine ~/.local/bin/dbboard-mcp   # not notarized yet
```

Verify the download against `SHA256SUMS.txt` on the same release page.

### Or build from source

```sh
cargo build --release -p dbboard-mcp
# binary at target/release/dbboard-mcp(.exe)
```

**Copy it out of `target/` before registering it.** An agent holds its
server process for the whole session, and on Windows a running executable
cannot be replaced — so pointing `claude mcp add` straight at
`target/release/dbboard-mcp.exe` means the next `cargo build --release`
fails with `failed to remove file … (os error 5)` until every agent that
ever started the server is closed. Registering a copy keeps building and
serving independent.

## Configure Claude Code

One command, run from anywhere. `--scope user` registers the server for
every project on the machine rather than just the current one. Give the
**absolute** path — the agent host does not resolve `PATH` or `~` here:

```sh
# macOS / Linux
claude mcp add dbboard --scope user -- "$HOME/.local/bin/dbboard-mcp"
```

```powershell
# Windows (PowerShell) — forward slashes, or escaped backslashes
claude mcp add dbboard --scope user -- "$env:LOCALAPPDATA/dbboard/dbboard-mcp.exe"
```

That writes an `mcpServers.dbboard` entry into `~/.claude.json`, which
you can also edit by hand:

```jsonc
{
  "mcpServers": {
    "dbboard": {
      "type": "stdio",
      "command": "C:/path/to/dbboard-mcp.exe",
      "args": [],
      "env": {}
    }
  }
}
```

Verify with `claude mcp list`, then **restart Claude Code** — see
[Restart after any config change](#restart-after-any-config-change).

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

## Restart after any config change

Every MCP client spawns its **own** `dbboard-mcp` process over stdio and
keeps it for the life of the session. Two consequences that account for
most "it still doesn't work" reports:

- Editing `connections.toml`, adding a connection in the GUI, or
  rebuilding the binary does **not** reach a server that is already
  running. Restart the *agent* — Claude Code, Claude Desktop, whatever
  spawned it.
- Restarting the dbboard **desktop app** does nothing to the MCP server.
  They are separate processes that happen to read the same config.

Also note that the binary an agent runs is whatever absolute path its
config points at. Installing a new dbboard release does not update a
path that points into a local `target/release/` build tree.

## Running more than one agent at once

Nothing stops several agents from each running their own `dbboard-mcp`,
but each process opens its **own** connections — including its **own SSH
tunnel** for any connection with a `[connections.ssh]` block
([`docs/connections.md`](../../docs/connections.md)). Three agents
against one tunneled MySQL entry means three SSH sessions and three
connection pools, not one shared.

That matters because the limits you hit first are usually not the
database's:

- `sshd` throttles concurrent and half-open sessions (`MaxStartups`,
  `MaxSessions`). Under that limit connections are dropped, sometimes
  probabilistically, and a retry loop makes it worse rather than better.
- The private key must be readable *now*. A key kept in a
  cloud-synced folder (OneDrive, Dropbox, iCloud Drive) may be a
  placeholder that has to be downloaded first; dbboard warns about such
  paths for exactly this reason.

## Troubleshooting a failed connection

The error text comes from the database driver, so read it as "how far
did we get", not "what is wrong":

| Symptom | What it means | Where to look |
|---|---|---|
| `expected to read N bytes, got 0 bytes at EOF` | The TCP connection was accepted and then closed before the protocol handshake finished. | For a **tunneled** connection this is the SSH hop, not the database — the local forward accepted the socket but the channel died. Check `sshd` limits and the key file. Otherwise: the server's connection limit. |
| `connection refused` / timeout | Nothing accepted the socket. | Host, port, firewall, IP allow-list. |
| A keyring or secret error | The entry was found but its secret could not be read. | The OS keychain entry named by the `keyring_*_ref`. |
| The tool list is empty or stale | The agent is talking to an old process. | [Restart after any config change](#restart-after-any-config-change). |

`RUST_LOG=debug` sends the server's own view of connection setup to
stderr, which the client usually surfaces as MCP server logs.

If one agent can reach a connection and another cannot **from the same
machine**, the database is not the variable — the agent's config,
its process age, and the number of tunnels already open are.

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
  operations, and enforces the row cap, the write policy, and secret
  redaction. Testable against a real (in-memory) adapter with no MCP
  wiring.
- `server.rs` — `DbboardMcp`, the thin `rmcp` `ServerHandler` that wraps
  each service method as a `#[tool]`, serializes results to a JSON text
  block, and maps errors onto the MCP envelope.
- `main.rs` — startup wiring: tracing to stderr, config-path resolution,
  and the stdio serve loop.

## See also

- [ADR-0046](../../docs/decisions.md) — the dbboard-mcp read-only MCP
  server decision; [ADR-0053](../../docs/decisions.md) — the
  `search_schema` tool that extended the surface to six; and
  [ADR-0054](../../docs/decisions.md) — foreign-key introspection and the
  `list_relationships` tool that extends it to seven.
- [ADR-0134](../../docs/decisions.md) — `add_connection`, and why only the
  credential-free kinds can be registered by an agent.
- [ADR-0136](../../docs/decisions.md) — `set_connection_mark` and
  `move_connection`, and why the mark and the order are not behind
  `mcp_write`.
- [ADR-0087](../../docs/decisions.md) — the three-tier write policy and
  the `run_write` / `dump_database` tools that take the surface to nine.
- [ADR-0045](../../docs/decisions.md) — local table/column annotations,
  surfaced by `get_annotations`.
- [ADR-0107](../../docs/decisions.md) — the UI language in
  `ui-settings.toml` and the `get_ui_locale` / `set_ui_locale` pair that
  takes the surface to eleven.
- [ADR-0108](../../docs/decisions.md) — `capture_window`, which takes it to
  twelve: the agent can see the window it is being asked about, so a claim
  about what the interface renders can be checked rather than asserted.
- [ADR-0109](../../docs/decisions.md) — the UI command channel and the
  `set_editor_sql` / `run_query` / `open_ai_panel` / `open_ai_settings`
  verbs that take the surface to sixteen: having seen the window, the
  agent can now work it.
- [`docs/connections.md`](../../docs/connections.md) — `connections.toml`
  schema and the keyring-reference layout.
- [`docs/architecture.md`](../../docs/architecture.md) — where this crate
  sits in the workspace.
