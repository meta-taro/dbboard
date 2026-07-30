# dbboard desktop v0.4.0 — internal release note

This is the material for the **internal share** of the new desktop app
(`apps/desktop/`, Tauri 2 + SvelteKit) at version **0.4.0**. It is written to
be lifted into an internal issue by a separate agent: it covers the download,
the externally visible feature set, and step-by-step operation.

> This is the **new-design** desktop client, distinct from the older single-exe
> egui build described in [`internal-testing.md`](internal-testing.md). v0.4.0
> reaches feature parity with egui (see
> [`desktop-parity.md`](desktop-parity.md)) and adds SSH-tunnel editing on top.

---

## What you received

A Windows installer — pick **one**:

| File | Type | Size | Notes |
|---|---|---|---|
| `dbboard-desktop_0.4.0_x64-setup.exe` | NSIS installer | ~7.4 MB | **Recommended.** Per-user install, no admin rights, adds a Start-menu shortcut. |
| `dbboard-desktop_0.4.0_x64_en-US.msi` | MSI installer | ~12 MB | Alternative for MSI-based deployment. |

- x64 Windows only.
- **Optionally, a `.dbbx` file and a passphrase** — a pre-packaged set of
  connections. The passphrase comes through a *different* channel than the file
  (the file is encrypted). Skip the import step if you did not receive one.

### Two caveats for this hand-shared build

1. **Unsigned.** Windows SmartScreen will say the publisher is unrecognized.
   Click **More info → Run anyway**. If antivirus quarantines it, restore and
   allow it — it is a plain desktop app with no bundled installer tricks.
2. **No auto-update on this build.** The signed, self-updating release is
   produced by CI once the signing key is in place; this hand-shared installer
   has updater signing disabled. The app still *checks* GitHub on startup and
   the **About** dialog will say if a newer version exists, but it will not
   install anything itself — you update by installing the next handed-out build.
   The update check is off entirely if you set `DBBOARD_NO_UPDATE_CHECK` to any
   value.

---

## Install & run

1. Double-click `dbboard-desktop_0.4.0_x64-setup.exe`.
2. If SmartScreen warns, **More info → Run anyway**.
3. It installs per-user and drops a **dbboard-desktop** shortcut in the Start
   menu. Launch it from there.

The window opens with no login and no telemetry. Set the interface language
from the **language menu** in the top bar (English default, Japanese complete;
other locales fall back to English). Pick **Light / Dark / Auto** from the
theme toggle — Auto follows your Windows setting and the choice is remembered.

---

## What's new / what it does (externally visible features)

Everything the app can do, in user terms. All write actions live in the app UI
only — external tooling stays read-only.

**Connections**
- List, **add, edit, delete** connections from the connection window.
- Speaks to **Turso / libSQL**, **Cloudflare D1**, and **Postgres-compatible**
  databases (Supabase, Neon, Aurora DSQL).
- **Import / export** a passphrase-encrypted `.dbbx` bundle to move a whole set
  of connections between machines.
- **SSH tunnel** (new — desktop leads egui): connect through a bastion host.
  The connection form has an SSH section — bastion host/port/user, key **or**
  password auth, and a mandatory host-key pin (fingerprint or known_hosts).
  Secrets go to the Windows Credential Manager, never to a file.

**Browse & query**
- Sidebar table list with row counts; **schema search** across tables and
  columns.
- **Structure** tab: columns, types, primary keys, and foreign-key
  relationships.
- SQL editor with **Run** (Ctrl+Enter). Bare `SELECT`s get an automatic
  `LIMIT` so a huge table can't freeze the UI; the cap (100/200/500/1000) is
  adjustable and the grid shows "capped at N".
- **Query history** per connection — click to reload a past query.
- Right-click a table for quick "select all rows" / "count rows" starters that
  also drop the SQL into the editor.

**Results & data**
- Sort, multi-select rows, copy as TSV/CSV, or **export** to a file
  (CSV / CSV-with-BOM / TSV). Click a long cell to see its full value.
- **Inline cell editing** — edit a value in place (UPDATE only; requires a
  declared primary key; commits only when exactly one row is affected).
- **Local annotations** — add notes to a table or a column (stored locally,
  never written to the database).

**Backup / restore**
- **Backup**: dump a whole connection to a `.sql` file, with a progress bar and
  cancel.
- **Restore**: apply a chosen `.sql` script, with progress, cancel, and an
  empty-target confirmation.

**AI assistant** (only if an AI provider is configured)
- **Explain SQL** and **Suggest SQL**, streamed. It never runs SQL and never
  sends row data — Explain sends the SQL text, Suggest sends schema names only.
  The API key is stored in the Credential Manager, never in a file.

---

## A five-minute tour to try

1. **Connect** — open the connection window (top bar). If you got a `.dbbx`,
   click **Import**, choose the file, enter the passphrase; the connections
   appear. Otherwise **Add** one for your own database. Select a connection and
   **Connect**.
2. **Browse** — pick a table in the sidebar; right-click for "select all" /
   "count rows".
3. **Query** — type SQL in the editor, **Run** (Ctrl+Enter). Try changing the
   row-limit selector.
4. **Results** — sort a column, select some rows, copy them, then **export** to
   CSV.
5. **Structure** — switch to the Structure tab to see columns, types, keys, and
   foreign keys.
6. **Edit a cell** — on a table with a primary key, double-click a value, change
   it, and confirm.
7. **Annotate** — add a note to a column on the Structure tab.
8. **AI** (if configured) — try **Explain SQL** on a query.
9. **SSH tunnel** (if you connect through a bastion) — in Add/Edit, enable the
   SSH section and fill in the bastion host, user, and key or password.

---

## Reporting feedback

The single most useful thing to send is the **error text**. Errors are shown in
your language with the original English beneath, and there is a **Copy** button —
copy the whole thing; the English half is what a maintainer will match on.

Please include:

1. **What you did** — the click path or the SQL you ran.
2. **What you expected** vs **what happened**.
3. **The copied error text**, if any.
4. The **version** — open the **About** dialog; it shows `dbboard 0.4.0`.
5. A **screenshot** if the problem is visual.

Send it wherever the handoff message says.

---

## Privacy & security notes

- Connection secrets, SSH key passphrases / passwords, and AI API keys live in
  the **Windows Credential Manager**, never in a file the app writes.
- Config files (`connections.toml` and friends) live under
  `%APPDATA%\dbboard\dbboard\config`. They hold connection *settings* and the
  *names* of secrets, never the secret values.
- A `.dbbx` bundle **does** contain secrets (encrypted). Treat it like a
  password: do not forward it, and delete it once imported.
- SSH host-key verification is mandatory — the app pins a fingerprint or uses
  `known_hosts`; it never blindly accepts an unknown host key.
