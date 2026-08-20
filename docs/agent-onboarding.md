# Telling an AI agent in another repository about dbboard

dbboard ships an MCP server, `dbboard-mcp`, so a coding agent working in
*some other* repository can read a database's schema and run queries
without a human pasting `psql` output into the chat.

Getting an agent to actually use it takes one more step than one would
expect. An agent does not go looking for tools it has not been told
about, and a web search for "dbboard" is not enough — one agent that
searched concluded dbboard was "not a publicly available tool". So the
instruction has to be written into the repository the agent works in,
usually its `CLAUDE.md` or `AGENTS.md`.

This page is that instruction, ready to copy. It lives here so it does
not have to be rewritten from memory each time a new repository needs
it.

> **Availability.** The binary is published from **v0.5.0** onward.
> Earlier releases carried no MCP asset at all, so the install steps
> below would have named a file that did not exist.

---

## Copy this into the other repository

````markdown
## dbboard MCP (database access)

Use this MCP server to inspect schemas and run read queries. Do not
shell out to `psql` / `mysql` and do not paste credentials into the
chat — the server reads the credentials dbboard already holds in the OS
keychain.

### One-time setup

1. Download the MCP server from <https://meta-taro.github.io/dbboard/>
   (the *Releases* link) — `dbboard-mcp-windows-x86_64.exe` on Windows,
   `dbboard-mcp-macos-universal` on macOS. It is a **separate download
   from the desktop app**; the installer does not contain it.

2. Put it somewhere stable, because the path goes into the agent's
   config.

   ```powershell
   # Windows (PowerShell)
   mkdir -Force "$env:LOCALAPPDATA\dbboard"
   Move-Item -Force ~\Downloads\dbboard-mcp-windows-x86_64.exe "$env:LOCALAPPDATA\dbboard\dbboard-mcp.exe"
   ```

   ```sh
   # macOS
   mkdir -p ~/.local/bin
   mv ~/Downloads/dbboard-mcp-macos-universal ~/.local/bin/dbboard-mcp
   chmod +x ~/.local/bin/dbboard-mcp
   xattr -d com.apple.quarantine ~/.local/bin/dbboard-mcp   # unsigned build
   ```

3. Register it. `--scope user` makes it available in every project on
   the machine.

   ```powershell
   # Windows (PowerShell)
   claude mcp add dbboard --scope user -- "$env:LOCALAPPDATA/dbboard/dbboard-mcp.exe"
   ```

   ```sh
   # macOS / Linux
   claude mcp add dbboard --scope user -- "$HOME/.local/bin/dbboard-mcp"
   ```

4. **Restart the agent.** Each client spawns its own `dbboard-mcp`
   process and holds it for the session, so an already-running agent
   keeps talking to the old one. Restarting the dbboard *desktop app*
   does nothing to it — they are separate processes.

### Tools

`list_connections` · `list_tables` · `describe_table` · `search_schema` ·
`list_relationships` · `run_read_query` · `get_annotations` ·
`dump_database` — all read-only.

`get_ui_locale` · `set_ui_locale` change dbboard's display language and
reach no database at all (ADR-0107). Use them only when asked: the effect
lands on someone's screen.

`capture_window` photographs the running dbboard window and returns it as
a PNG (ADR-0108) — the way to check what the app actually renders rather
than assert it. It reads nothing. What it returns is someone's real
screen, real connection names and all, so describe what you see but do not
paste the image anywhere public without asking.

`set_editor_sql` · `run_query` · `open_ai_panel` · `open_ai_settings`
work that same window (ADR-0109): put SQL in the editor, press Run, open
the AI panel, open the provider settings inside it. With `capture_window`
they are how you set up and check what the app *shows*. `run_query` is not
a cheaper `run_read_query` — it runs whatever the editor holds, against
the connection that window has selected, and leaves the rows on the
operator's screen. All four fail outright when dbboard is not running, and
retrying does not open it.

`get_server_info` names the build answering you (ADR-0116). This binary is
installed by hand and never updates itself, so it can be older than the
fix for whatever you are looking at — a bug you are about to report may
already have been closed. Call it before reporting anything odd, and put
the version in the report. It reaches no database, reads no configuration,
and returns nothing about the machine it runs on.

`open_ai_settings` is also refused while the AI panel is shut, because the
part of the window that owns that verb is not mounted then. Read that
refusal as an answer rather than an obstacle: it is what tells you the
provider settings are reached *through* the panel and not from the top
level.

`run_write` exists but is **refused unless the connection is opted in**
with `mcp_write = true`, and a permanently-closed list (grants, user and
role DDL, `TRUNCATE`, `DROP` of anything but an index) is refused
whatever that flag says. On a `firestore` or `mongodb` connection the
flag opens nothing at all — neither adapter implements a write path, so
those two are read-only for every caller.

Start with `list_connections`; the id it returns is the handle every
other tool takes.

### Where the connection details come from

With no arguments the server reads the same per-user `connections.toml`
the dbboard GUI writes, and pulls secrets from the OS keychain. Pass
`--config` (or set `DBBOARD_CONFIG`) to point at a curated,
least-privilege subset instead.

If writing credentials to a file is not allowed here, pass them as
`DBBOARD_*` environment variables in this repository's own MCP config
block — see *Credentials without writing a file* in the crate README.
````

---

## Two things worth saying out loud when you paste it

**The binary is unsigned.** macOS Gatekeeper blocks it until the
quarantine attribute is removed (step 2 above). On Windows, an
aggressive antivirus may quarantine a freshly downloaded unsigned
executable; the checksums published alongside the release are how to
verify what was downloaded.

**Connection ids reach the model.** `list_connections` returns each
connection's id and name, and those are whatever the person who created
them typed — often a host or a customer name. They end up in the agent's
transcript and in its model provider's logs. If that matters, set
`mcp_alias` on the connection (dbboard: *Connections → Edit → AI agent
access*). The alias then replaces both the id and the name, and the real
id stops working as a handle, so one learned from an older session
cannot be handed back.

## Related

- [`crates/dbboard-mcp/README.md`](../crates/dbboard-mcp/README.md) —
  the full server reference: every tool's arguments, the write policy,
  the environment variables, and connection-failure symptoms.
- [`README.md`](../README.md) — setup for humans.
- [ADR-0090](decisions.md) — why the binary is published at all, and why
  the download page deliberately does not list it.
- [ADR-0087](decisions.md) (write policy), [ADR-0088](decisions.md)
  (`mcp_alias`).
