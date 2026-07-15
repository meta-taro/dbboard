# Collector setup pack (Windows)

This folder is everything needed to stand up dbboard on the data-collection
machine. It targets one operator running three connections:

| Connection id     | Database          | `kind`            |
| ----------------- | ----------------- | ----------------- |
| `store-cabaret`   | Cloudflare D1     | `d1`              |
| `store-lovehotel` | Aurora DSQL (IAM) | `aurora-dsql-iam` |
| `vegas-gift`      | Supabase          | `supabase`        |

**No secret ever goes in a file.** `connections.toml` holds only non-secret
fields and *names* of secrets; the secret material (a Cloudflare API token,
an AWS secret access key, and a Supabase connection URL) lives in the
Windows Credential Manager. The config file is therefore safe to copy, sync,
or attach to a bug report.

The Aurora DSQL connection mints its own short-lived IAM token and refreshes
it automatically before expiry (ADR-0037, 段階B), so it stays up unattended
around the clock — no manual reconnect on the 15-minute token cycle.

---

## Prerequisites

- `dbboard.exe` (a single self-contained binary — no installer required).
- The three secret values on hand:
  - Cloudflare API token with D1 read access,
  - AWS secret access key for the DSQL IAM user (the matching **access key
    id** is non-secret and goes in the config file),
  - the full Supabase Postgres connection URL, e.g.
    `postgresql://user:password@host:5432/postgres`.

All commands below are PowerShell.

---

## Step 1 — Place the config file

The per-user config path on Windows is:

```
%APPDATA%\dbboard\dbboard\config\connections.toml
```

Copy the template into place and open it for editing:

```powershell
$dir = "$env:APPDATA\dbboard\dbboard\config"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item .\connections.template.toml "$dir\connections.toml"
notepad "$dir\connections.toml"
```

Replace every `UPPER_CASE` placeholder with the real value:

- `store-cabaret`: `account_id`, `database_id`.
- `store-lovehotel`: `endpoint`, `region` (default `ap-northeast-1`),
  `database` (default `postgres`), `username` (default `admin`),
  `access_key_id`.
- `vegas-gift`: nothing — its only real value is the URL, and that is a
  secret seeded in Step 2.

Leave every `keyring_*_ref` line exactly as shipped; those names are what
Step 2 seeds against.

---

## Step 2 — Seed the three secrets

dbboard reads secrets from the Windows Credential Manager. Each secret is a
**Generic** credential whose *target name* is `<ref>.dbboard` and whose
*user* is `<ref>`, where `<ref>` is the `keyring_*_ref` from the config file.
`cmdkey` writes exactly the credential format dbboard reads (verified against
the shipping keyring backend).

Run these three commands, pasting each real secret in place of the
double-quoted placeholder. Keep the quotes — the Supabase URL and some tokens
contain characters PowerShell would otherwise interpret.

```powershell
# store-cabaret — Cloudflare API token
cmdkey /generic:dbboard.store-cabaret.token.dbboard `
       /user:dbboard.store-cabaret.token `
       /pass:"PASTE_CLOUDFLARE_API_TOKEN"

# store-lovehotel — AWS secret access key
cmdkey /generic:dbboard.store-lovehotel.secret_key.dbboard `
       /user:dbboard.store-lovehotel.secret_key `
       /pass:"PASTE_AWS_SECRET_ACCESS_KEY"

# vegas-gift — full Supabase Postgres URL
cmdkey /generic:dbboard.vegas-gift.url.dbboard `
       /user:dbboard.vegas-gift.url `
       /pass:"postgresql://user:password@host:5432/postgres"
```

Because the secret is on the command line, clear it from the session history
afterwards:

```powershell
Clear-History
Remove-Item (Get-PSReadlineOption).HistorySavePath -ErrorAction SilentlyContinue
```

> **Friendlier alternative for D1 and Supabase.** The connection window has
> an **Add** form for the `d1` and `supabase` kinds that takes the secret in
> a masked field and writes both the credential and the config entry for you
> — no `cmdkey`, no editing TOML. If you use it, remove the matching
> `[[connections]]` block from `connections.toml` first so you do not end up
> with two entries sharing one `id`. The `aurora-dsql-iam` kind has **no**
> Add form, so `store-lovehotel` must be configured in the file (Step 1) and
> seeded with `cmdkey` (above) regardless.

---

## Step 3 — Launch and pick a connection

```powershell
.\dbboard.exe
```

Open the connection window, select one of the three connections, and connect.
`store-lovehotel` will keep its IAM token fresh on its own; if credentials are
ever rotated out from under a live session, the connection window's
**Reconnect** button rebuilds it with a freshly minted token.

To launch straight into a specific connection, set `DBBOARD_CONNECTION` to its
`id` first:

```powershell
$env:DBBOARD_CONNECTION = "store-lovehotel"
.\dbboard.exe
```

---

## Verifying a seeded secret

List what dbboard owns in the Credential Manager (all entries end in
`.dbboard`):

```powershell
cmdkey /list | Select-String "dbboard"
```

You should see three targets:

```
dbboard.store-cabaret.token.dbboard
dbboard.store-lovehotel.secret_key.dbboard
dbboard.vegas-gift.url.dbboard
```

## Updating or rotating a secret

Re-running the same `cmdkey /generic:...` command overwrites the stored value.
To remove one entirely:

```powershell
cmdkey /delete:dbboard.store-lovehotel.secret_key.dbboard
```

## Troubleshooting

- **"no secret stored for reference …" at connect** — the `keyring_*_ref` in
  the config file and the seeded credential's *user*/*target* disagree.
  Confirm the target is exactly `<ref>.dbboard` and the user is exactly
  `<ref>`.
- **Aurora DSQL "access denied" right after launch** — the `access_key_id`,
  `region`, or `username` in the config file is wrong, or the seeded AWS
  secret key does not match that access key id. These four must belong to the
  same IAM identity with `DbConnectAdmin` on the cluster.
- **Supabase connects but times out** — check whether the URL points at the
  direct port (`:5432`) or the pooler (`:6543`); the URL itself selects it.

---

## How it fits together

- The config schema and secret-reference indirection are ADR-0013 /
  ADR-0024; the file never holds secret material.
- Aurora DSQL IAM token minting is ADR-0036; in-pool auto-refresh is
  ADR-0037.
- `connections.template.toml` in this folder is covered by
  `crates/dbboard-config/tests/collector_template.rs`, so a schema change
  that would break the template fails the test suite rather than the
  operator's launch.

## Where this comes from

The project lives at <https://github.com/meta-taro/dbboard> — latest
builds, full docs, and where to file a bug report. The running app also
links it under **Help → Project on GitHub**, so the version and its
source are always one click apart.
