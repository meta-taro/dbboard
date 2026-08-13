# Demo profile

A dbboard profile that holds nothing of yours: three fictional tables, no
credentials, no query history, and a connection list with one entry that
points at a local file.

It exists because a screenshot of dbboard as you actually run it shows your
connection list, and a connection list is a list of real hosts. Rather than
blurring things out afterwards — which is a judgement call made under time
pressure, every time — there is a second profile that has nothing to hide.

The download page's screenshots are taken against this. So is anything else
that shows the app to people who are not you: a walkthrough, a bug report, a
conference slide.

## Build the database

Requires Node 22 or newer (`node:sqlite` is used to write the file; nothing is
installed).

```sh
node scripts/demo-profile/seed.mjs C:/claude/dbboard-demo/demo.db
```

It writes three tables — `stations` (12 rows), `readings` (480) and `alerts`
(9) — of weather-station data that belongs to nobody. `readings` is
deliberately over a hundred rows so that a screenshot can show the row limit
holding something back.

The corpus is generated from a fixed seed, so re-running it produces byte-identical
data. Two releases' screenshots then differ only where the app differs.

`scripts/demo-profile/seed.test.mjs` (`node --test`) checks both halves of
that: that there is enough data for the app to look like itself, and that no
string in it is shaped like an email address, an IP address or a domain.

It is a plain SQLite file on purpose. libSQL opens it unchanged, so the
Turso/libSQL adapter reads it with no server and no container — a screenshot
should not be blocked on a Docker daemon.

## Point dbboard at it

`DBBOARD_CONFIG_DIR` replaces the per-user config directory, and every file
dbboard owns moves with it (ADR-0097), so the demo profile can never end up
holding one file from your real one. Give it an empty directory:

```powershell
$env:DBBOARD_CONFIG_DIR = "C:/claude/dbboard-demo/config"
$env:DBBOARD_NO_UPDATE_CHECK = "1"
& "C:/claude/dbboard/target/release/dbboard-desktop.exe"
```

The app starts with no connections. Add one **through the app's own UI** —
Manage → Add connection — rather than by writing `connections.toml` by hand,
so that what gets photographed is a connection the app made:

| Field | Value |
|---|---|
| ID | `weather-demo` |
| Name | `Weather demo` |
| Kind | Turso / libSQL |
| Database path | the `demo.db` written above |

The OS keychain is **not** redirected by `DBBOARD_CONFIG_DIR`. It does not need
to be: secrets are keyed by connection id, so a profile with its own ids has no
entries to find. Nothing here has a secret anyway.

## Pick the directory before you launch, not after

Put the profile somewhere with no username in the path — `C:/claude/dbboard-demo`,
not a temp directory under your home. dbboard prints the resolved
`connections.toml` path in its empty state, so the first screenshot anyone takes
of a fresh profile is a screenshot of that path. On Windows the per-user temp
directory contains the OS username, which is denylisted PII.

## Before publishing anything captured here

See [`docs/maintainer/screenshots.md`](../../docs/maintainer/screenshots.md).
