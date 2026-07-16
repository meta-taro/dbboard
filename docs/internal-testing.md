# Internal testing guide

Thanks for helping test **dbboard**, a desktop database client. This page
is for internal testers on **Windows** who received a build to try out and
report back. It takes about five minutes to get running.

> If instead you are setting up the app to do data-collection work against
> the three fixed connections, follow
> [`collector-setup/README.md`](collector-setup/README.md) — it is the
> task-specific setup. This guide is the general "try it and give feedback"
> path.

---

## What you received

- **`dbboard.exe`** — the whole app in one file. No installer, no
  admin rights, nothing to add to PATH. You can run it from anywhere
  (Downloads, Desktop, a USB stick).
- **Optionally, a `.dbbx` file and a passphrase** — a pre-packaged set of
  connections. You will get the passphrase through a *different* channel
  than the file (that is intentional; the file is encrypted). Skip the
  import section below if you did not receive one.

The only thing this needs the internet for is your database connections —
plus one small, optional check: on startup dbboard asks GitHub whether a
newer version has been released, so the **Help** menu can tell you when an
update is available. It never downloads or installs anything on its own,
it stays silent if you are offline, and you can switch it off entirely by
setting `DBBOARD_NO_UPDATE_CHECK` to any value.

---

## Run it

Double-click `dbboard.exe`, or from PowerShell:

```powershell
.\dbboard.exe
```

The window should open and paint. There is no login and no telemetry.

> **Windows SmartScreen** may warn that the publisher is unrecognized —
> the build is unsigned. Click **More info → Run anyway**. If your antivirus
> quarantines it, restore it and allow it; the binary is a plain Rust GUI
> app with no installer. Its only outbound call of its own is the optional
> update check described above (off via `DBBOARD_NO_UPDATE_CHECK`);
> everything else on the network is the database connections you configure.

Set the interface language from the **Language** menu if you prefer
Japanese; English is the default and the fallback for anything not yet
translated.

---

## Connect to a database

### If you received a `.dbbx` bundle

1. Open the connection window (the **Connections** entry in the menu bar).
2. Click **Import**, pick the `.dbbx` file, and enter the passphrase you
   were given separately.
3. The connections appear in the list, secrets and all. Select one and
   click **Connect**.

Re-importing the same bundle is safe — anything whose id already exists is
skipped and reported.

### If you are pointing it at your own database

Open the connection window and click **Add**. dbboard speaks to Turso /
libSQL, Cloudflare D1, and Postgres-compatible databases (Supabase,
Neon, Aurora DSQL). Fill in the fields for your database kind; the secret
field is masked and is stored in the Windows Credential Manager, never in
a file.

---

## What to try

A quick tour that exercises the main paths:

- **Browse** — pick a table in the left sidebar; right-click it for quick
  "Select all rows" / "Count rows" starters that run on the spot (the SQL
  also lands in the editor so you can tweak and re-run it).
- **Query** — type SQL in the editor and press **Run** (or Ctrl+Enter).
  Bare `SELECT`s get an automatic `LIMIT` so a huge table cannot freeze the
  UI; you can override or uncheck it.
- **Results** — sort, select rows, and copy them or export to CSV. Click a
  long/multi-line cell to see its full value.
- **Structure** — switch to the Structure tab to inspect a table's columns,
  types, and keys.
- **AI assistant** (only if a provider was configured) — try "Explain SQL"
  and "Suggest SQL".

---

## Reporting feedback

When something is wrong or awkward, the single most useful thing you can
send is the **error text**. Every error in dbboard is now shown in your
language **with the original English beneath it**, and there is a **Copy**
button on the error (the text is also selectable, so Ctrl+C works). Copy
the whole thing into your report — the English half is what a maintainer or
a search engine will match on.

Please include:

1. **What you did** — the click path or the SQL you ran.
2. **What you expected** vs **what happened**.
3. **The copied error text**, if any.
4. The **version** — open **Help** in the menu bar; it shows
   `dbboard <version>`. **Help → Project on GitHub** is the same place bugs
   are tracked.
5. A **screenshot** if the problem is visual.

Send it wherever the maintainer asked you to (the handoff message will say).

---

## Privacy notes

- The app stores connection secrets in the **Windows Credential Manager**,
  not in any file it writes.
- Config files (`connections.toml` and friends) live under
  `%APPDATA%\dbboard\dbboard\config`. They contain connection *settings*
  and the *names* of secrets, never the secret values.
- A `.dbbx` bundle **does** contain secrets (encrypted). Treat it like a
  password: do not forward it, and delete it once you have imported it.
