# Connection Store

dbboard remembers your saved database connections in a small TOML file
plus your OS keychain. The TOML carries the non-secret shape of each
connection (host, ids, paths); the actual tokens and URLs live in the
keychain. See [ADR-0013](decisions.md) for the rationale.

## File location

| OS | Path |
|---|---|
| Linux | `$XDG_CONFIG_HOME/dbboard/connections.toml` (default `~/.config/dbboard/connections.toml`) |
| macOS | `~/Library/Application Support/dev.dbboard.dbboard/connections.toml` |
| Windows | `%APPDATA%\dbboard\dbboard\config\connections.toml` |

Resolved via the
[`directories`](https://crates.io/crates/directories) crate. dbboard
creates the file on first save with mode `0o600` on Unix.

### Pointing dbboard at a different config dir

`DBBOARD_CONFIG_DIR=<path>` replaces the per-user lookup above. Every
file dbboard owns moves with it — `connections.toml`, `history.jsonl`,
`ai-providers.toml`, `annotations.toml`, `ui-settings.toml` — so a
throwaway profile can never end up holding one file from your real one.
A blank or whitespace-only value is ignored and the per-user lookup
applies, so an exported-but-empty variable does not silently write
credentials into whatever directory the app was started from.

This exists for demos, screenshots and walkthroughs: launch dbboard
against an empty directory and it starts with no connections and no
query history, which is the only way to show the app without showing
your own hosts. On Windows there was previously no way to do this at
all — the `directories` lookup reads the known-folder API, not
`%APPDATA%`, so redirecting the environment variable opens the real
profile anyway. See [ADR-0097](decisions.md).

The OS keychain is **not** redirected. Secrets are keyed by connection
id, so a demo profile with its own ids simply has no keychain entries
to find; it does not read your real ones.

## Resolution order

At startup the binary picks a backend in this order:

1. `DBBOARD_AURORA_DSQL_URL` (Aurora DSQL-flavored Postgres-wire — see
   [ADR-0021](decisions.md); the adapter is labelled `aurora-dsql` at
   runtime).
2. `DBBOARD_NEON_URL` (Neon-flavored Postgres-wire — see [ADR-0018](decisions.md);
   the adapter is labelled `neon`).
3. `DBBOARD_SUPABASE_URL` (Supabase-flavored Postgres-wire — see
   [ADR-0019](decisions.md); the adapter is labelled `supabase`).
4. `DBBOARD_PG_URL` (generic PostgreSQL-wire — CockroachDB, self-hosted
   Postgres; the adapter is labelled `postgres`).
5. `DBBOARD_MYSQL_URL` (MySQL / MariaDB — a distinct SQL dialect, not a
   pg-wire flavor; the adapter is labelled `mysql`. See
   [ADR-0068](decisions.md)).
6. The `DBBOARD_D1_*` trio (account id + database id + token).
7. `DBBOARD_FIRESTORE_PROJECT_ID` (Cloud Firestore over REST, read-only
   — see [ADR-0091](decisions.md), [ADR-0093](decisions.md); the adapter
   is labelled `firestore`).
8. `DBBOARD_MONGODB_URI` (MongoDB over the official driver, read-only —
   see [ADR-0095](decisions.md), [ADR-0096](decisions.md); the adapter is
   labelled `mongodb`).
9. `DBBOARD_TURSO_PATH` (explicit local libSQL path).
10. `DBBOARD_CONNECTION=<id>` matched against `connections.toml`. A
    missing id aborts startup — dbboard refuses to silently fall back to
    a different backend than the user asked for.
11. If `connections.toml` contains exactly one entry, that one is
    auto-selected.
12. Otherwise an in-memory Turso/libSQL database (`:memory:`).

`DBBOARD_AURORA_DSQL_URL`, `DBBOARD_NEON_URL`, and
`DBBOARD_SUPABASE_URL` all outrank `DBBOARD_PG_URL` because they carry
more specific labelling. Among the pg-wire flavors the order is
alphabetical: `aurora-dsql` > `neon` > `supabase`; setting two
flavored vars at once is unusual but the precedence is fully defined.

## TOML schema

```toml
version = 1

# Optional. Present only if you have decided to let an AI agent pack
# connections for another machine; the directory you name here is the
# permission itself. Absent means the MCP export verb is closed. See
# ADR-0140 and "Letting an agent make the bundle" below.
# [mcp_export]
# dir = "C:/Users/you/dbboard-bundles"

[[connections]]
id   = "local-libsql"
name = "Local libSQL"
kind = "turso"
path = ":memory:"

[[connections]]
id                = "turso-cloud-prod"
name              = "Turso Cloud (prod)"
kind              = "turso-remote"
# The endpoint the Turso dashboard shows. Not a secret, so it stays inline
# and the connection list can show where this points.
url               = "libsql://my-db-myorg.turso.io"
# The auth token lives in your OS keychain under (service="dbboard",
# account=keyring_token_ref).
keyring_token_ref = "dbboard.turso-cloud-prod.token"

[[connections]]
id                 = "cf-d1-prod"
name               = "Cloudflare D1 (prod)"
kind               = "d1"
account_id         = "1234abcd..."
database_id        = "uuid-of-the-database"
# Optional API root override; default is https://api.cloudflare.com/client/v4
# base_url         = "https://api.cloudflare.com/client/v4"
# The actual token lives in your OS keychain under (service="dbboard",
# account=keyring_token_ref).
keyring_token_ref  = "dbboard.cf-d1-prod.token"

[[connections]]
id              = "cockroach-prod"
name            = "CockroachDB (prod)"
kind            = "postgres"
# The full connection URL (with password) lives in your OS keychain
# under (service="dbboard", account=keyring_url_ref).
keyring_url_ref = "dbboard.cockroach-prod.url"

[[connections]]
id              = "neon-prod"
name            = "Neon (prod)"
kind            = "neon"
# Wire shape is identical to "postgres"; the discriminator only affects
# the runtime adapter id ("neon" vs "postgres") so the connection picker
# and history records can label the connection precisely. See ADR-0018.
keyring_url_ref = "dbboard.neon-prod.url"

[[connections]]
id              = "supabase-prod"
name            = "Supabase (prod)"
kind            = "supabase"
# Same pg-wire shape as "postgres" / "neon"; the discriminator labels
# the adapter "supabase" at runtime. Both the direct (:5432) and
# transaction-pooler (:6543) endpoints fit here — the URL itself picks
# the path. See ADR-0019.
keyring_url_ref = "dbboard.supabase-prod.url"

[[connections]]
id              = "aurora-dsql-prod"
name            = "Aurora DSQL (prod)"
kind            = "aurora-dsql"
# Same pg-wire shape as the other Postgres flavors; the discriminator
# labels the adapter "aurora-dsql" at runtime. The keyring URL's
# password segment must carry a short-lived IAM authentication token
# (~15 min TTL); an expired token surfaces as a connection error at
# startup. See ADR-0021.
keyring_url_ref = "dbboard.aurora-dsql-prod.url"

[[connections]]
id              = "shop-mysql"
name            = "Shop (MySQL)"
kind            = "mysql"
# MySQL / MariaDB — a genuinely different SQL dialect, not a pg-wire
# flavor. Same secret shape as the Postgres family (the keyring carries a
# "mysql://…" URL), but served by the dbboard-mysql adapter. See ADR-0068.
keyring_url_ref = "dbboard.shop-mysql.url"
# Opt this connection in to MCP writes. Absent means false. See ADR-0087.
mcp_write       = true

[[connections]]
id                     = "aurora-dsql-iam-prod"
name                   = "Aurora DSQL (IAM, prod)"
kind                   = "aurora-dsql-iam"
# Unlike "aurora-dsql", dbboard mints the ~15-min IAM token itself at
# connect time from the AWS credentials below, so you never hand-refresh
# a token. This is the kind to use for a 24/7 connection. The AWS access
# key id is a public identifier (not a secret) and lives inline; only the
# AWS secret access key is stored in the keychain. See ADR-0036.
endpoint               = "abc123xyz.dsql.ap-northeast-1.on.aws"
region                 = "ap-northeast-1"
database               = "postgres"
username               = "admin"
access_key_id          = "AKIAEXAMPLE1234567890"
keyring_secret_key_ref = "dbboard.aurora-dsql-iam-prod.secret_key"

[[connections]]
id                          = "firestore-prod"
name                        = "Firestore (prod)"
kind                        = "firestore"
project_id                  = "my-gcp-project"
# Optional. Absent means Firestore's own default database, "(default)".
# database_id               = "analytics"
# Optional REST root override; absent means production. This is how the
# local emulator is reached.
# base_url                  = "http://127.0.0.1:8080/v1"
# Optional — and the only optional keychain reference in the file. Absent
# means "this connection points at the emulator, which wants no
# credential", not "not filled in yet". See ADR-0094.
keyring_service_account_ref = "dbboard.firestore-prod.service_account"

[[connections]]
id              = "orders-mongo"
name            = "Orders (MongoDB)"
kind            = "mongodb"
# The whole "mongodb://…" or "mongodb+srv://…" URI lives in the keychain,
# because the password rides in its authority. Same secret shape as the
# Postgres family. See ADR-0096.
keyring_url_ref = "dbboard.orders-mongo.url"
# Optional. Omit it when the URI's path already names a database
# ("mongodb://host/orders"). When neither names one the adapter refuses
# rather than guessing.
database        = "orders"
```

### Fields

- `version` — currently `1`. dbboard refuses any other value rather
  than guessing at a forward- or backward-incompatible shape.
- `[mcp_export]` — optional top-level table, absent by default. Its one
  key, `dir`, names an existing directory. Naming one lets `dbboard-mcp`
  seal connections into a bundle there and *is* the permission to do so;
  removing the table closes the verb again. Nothing dbboard writes to this
  file ever adds it — a human does. See below.
- `id` — primary key referenced by `DBBOARD_CONNECTION`. Duplicate ids
  are a hard error at load time.
- `name` — display label for the (future) connection picker.
- `kind` — `"turso"`, `"turso-remote"`, `"d1"`, `"postgres"`, `"neon"`,
  `"supabase"`, `"aurora-dsql"`, `"aurora-dsql-iam"`, `"mysql"`,
  `"firestore"`, or `"mongodb"`. `"turso"` is a libSQL database **file**
  and takes a `path`; `"turso-remote"` is a **networked** libSQL endpoint
  — Turso Cloud or a self-hosted `sqld` — and takes a `url` plus an auth
  token in the keychain. They are separate kinds rather than one field
  that means a path or a URL depending on what you typed (ADR-0111).
  `"neon"`,
  `"supabase"`, `"aurora-dsql"`, and `"postgres"` share the same wire
  shape (the keyring carries a `postgres://…` URL either way); the only
  difference is the runtime adapter label, which the connection picker and
  history records read. `"mysql"` is a distinct dialect (ADR-0068) served
  by its own adapter, but stores its secret the same way — the keyring
  carries a `mysql://…` URL. `"aurora-dsql-iam"` is the exception: it
  carries its
  fields inline (`endpoint`, `region`, `database`, `username`,
  `access_key_id`) and stores only the AWS secret access key in the
  keychain, because dbboard mints the IAM token itself (see below).
  `"firestore"` and `"mongodb"` are the two document stores, both
  read-only (ADR-0091). `"firestore"` carries `project_id` inline and is
  the one kind whose keychain reference is *optional* — absent means the
  connection points at the emulator, which wants no credential at all
  (ADR-0094). `"mongodb"` stores a `mongodb://…` URI in the keychain
  exactly like the SQL families, with an optional inline `database` for
  when the URI's path does not name one.
- `keyring_*_ref` — opaque account string used to look up the secret
  in the OS keychain. Pick something stable and recognisable; the
  string is what shows in the OS UI alongside the constant service
  name `"dbboard"`.
- `mcp_write` — optional `bool`, default `false`. Lets the
  `dbboard-mcp` server write to this connection. Cross-cutting like
  `ssh`: valid on any `kind`. See below.
- `mcp_alias` — optional string, absent by default. The name
  `dbboard-mcp` shows an AI agent *instead of* this entry's `id` **and**
  `name`. Must be unique across every entry's alias and id. Also
  cross-cutting. See below.

### The MCP write gate (`mcp_write`)

`dbboard-mcp` hands your connections to an AI agent. Reading is always
allowed; **writing is off until you say otherwise, per connection**:

```toml
[[connections]]
id              = "staging-pg"
name            = "Staging"
kind            = "postgres"
keyring_url_ref = "dbboard.staging-pg.url"
mcp_write       = true
```

Omitting the key means `false`, so every connection that predates this
feature stays read-only across the upgrade — the gate exists to be off.

Turning it on does not turn everything on. `run_write` still parses the
statement and accepts only `INSERT` / `UPDATE` / `DELETE` / `MERGE` and
`CREATE TABLE` / `CREATE VIEW` / `CREATE INDEX` / `CREATE SCHEMA` /
`ALTER TABLE`; anything else — `COMMENT ON` included — is refused.
`GRANT` / `REVOKE` / `DENY`, user and role DDL, `SET PASSWORD`,
`TRUNCATE`, and `DROP` of anything at all (an index included) are
refused **whatever this flag says**. The full policy is ADR-0087 and
[`crates/dbboard-mcp/README.md`](../crates/dbboard-mcp/README.md).

The flag is also editable in the app — *Connections → Edit → AI agent
access*. The desktop app itself ignores it: it governs the MCP server
only, not what you can do by hand in dbboard.

### The agent-facing alias (`mcp_alias`)

`id` and `name` are whatever you typed, and `dbboard-mcp` used to hand
both to the agent verbatim. If your ids name a customer, a site, or a
host, that string ends up in the agent's transcript, in the model
provider's logs, and in every bug report anyone pastes it into.

`mcp_alias` replaces both:

```toml
[[connections]]
id              = "acme-prod"
name            = "Acme Inc. (production)"
kind            = "mysql"
keyring_url_ref = "dbboard.acme-prod.url"
mcp_alias       = "store-a"
```

The agent now sees exactly one string, `store-a`, as both the id and the
name — and **`acme-prod` stops working as a handle**. That is the
point: an id an agent picked up from an earlier session cannot be handed
back and echoed into the new one. Everything below the MCP boundary is
untouched, so `annotations.toml`, `DBBOARD_CONNECTION`, and the app's
connection list keep using the real id.

Aliases must be unique across every alias *and* every id — a handle that
matched two connections would send a query to the wrong database. Using an
entry's own id as its alias is allowed and means "show my id, nothing
else".

Absent means no alias: the agent sees the real id and name, exactly as
before. Editable in the app in the same place as the write gate —
*Connections → Edit → AI agent access*. Error messages are out of scope
and may still name the real id (ADR-0088).

### Aurora DSQL: `aurora-dsql` vs `aurora-dsql-iam`

Both connect to the same Postgres-wire Aurora DSQL endpoint; they differ
only in where the ~15-minute IAM auth token comes from:

- **`aurora-dsql`** — *you* pre-generate the token (e.g. with the AWS
  CLI) and store the whole `postgres://…` URL, token embedded, under
  `keyring_url_ref`. Simple, but the token expires in ~15 minutes, so
  this suits short interactive sessions where you can re-seed the URL.
- **`aurora-dsql-iam`** — *dbboard* mints a fresh token at connect time
  from stored AWS credentials, so you never hand-refresh. Use this for a
  long-lived / 24/7 connection. Only the AWS secret access key is a
  secret (in the keychain); the access key id, endpoint, region,
  database, and username are non-secret and live inline in the TOML.

  Current limitation (v1): the token is minted when the connection is
  first built (at startup and on each connection switch), not
  continuously refreshed inside a live pool. Any physical connection
  opened more than ~15 minutes after the last build fails — this includes
  a cold reconnect after the app idles **and** a long-running pool, since
  Aurora DSQL closes idle server-side connections and the pool then
  re-opens them with the now-expired token, surfacing as
  `unable to accept connection, access denied`. **Recovery:** click
  **Reconnect** on the active row in the connections window (or restart)
  to rebuild the adapter with a fresh token. Automatic in-pool refresh is
  a planned follow-up so unattended 24/7 use needs no manual clicks. This
  kind is created by hand-editing `connections.toml`; the in-app
  connection list can connect/reconnect and delete it, but not yet edit
  it.

### SSH tunnel (`[connections.ssh]`)

A connection to a database that only listens on a bastion's `localhost`
can reach it through an **SSH tunnel**: dbboard opens a local port
forward over SSH, then rewrites the connection URL's host/port to the
local end of that forward before the adapter dials. The tunnel is a
cross-cutting `ssh` sub-table on any connection whose `kind` supports it
(`postgres`, `neon`, `supabase`, `aurora-dsql`, `mysql`). The rest refuse
it rather than ignoring it, for three different reasons: `turso` is a
local file and `d1`, `firestore` and `aurora-dsql-iam` are HTTPS APIs or
mint their own endpoint, so none of them dial a `host:port` a forward
could stand in for. `turso-remote` *is* a `host:port` we could forward,
but the client asks for the host named in the URL, so a forward would
present a certificate for the wrong name — and that same URL is what the
token is scoped to; fronting it means teaching the tunnel to preserve
SNI, which is a larger change than this exclusion (ADR-0111).
`mongodb` is refused despite being TCP — one URI may list
several hosts and `mongodb+srv://` discovers a whole replica set from
DNS, so rewriting a single host would leave the driver failing over to
members the tunnel never covered: working at first, then silently not
(ADR-0096). See [ADR-0069](decisions.md) for the design.

```toml
[[connections]]
id              = "work-mysql"
name            = "Work MySQL (via bastion)"
kind            = "mysql"
keyring_url_ref = "dbboard.work-mysql.url"

  [connections.ssh]
  host = "bastion.example.com"
  port = 22
  user = "deploy"
  # --- auth: exactly one of key_path or keyring_password_ref ---
  key_path               = "/home/user/.ssh/id_ed25519"
  # passphrase for an encrypted key (omit for an unencrypted key):
  keyring_passphrase_ref = "dbboard.work-mysql.ssh_passphrase"
  # --- host-key policy: exactly one of fingerprint or known_hosts ---
  fingerprint            = "SHA256:abc123def456..."
```

- `host` / `port` / `user` — the bastion to dial. `port` defaults to 22.
- **Forward target** — the DB host and port to forward *to* are taken
  from the connection URL itself (e.g. `127.0.0.1:3306` from the
  `mysql://…@127.0.0.1:3306/…` URL in the keychain), **not** stored in
  the `ssh` block. Point the URL at the address the DB listens on *from
  the bastion's side* (usually `127.0.0.1`).
- **Auth — exactly one of:**
  - `key_path` — path to a private key for public-key auth. If the key
    is encrypted, `keyring_passphrase_ref` names the keychain entry
    holding its passphrase; omit it for an unencrypted key.
  - `keyring_password_ref` — keychain reference to an SSH password for
    password auth. Mutually exclusive with `key_path`.
- **Host-key policy — exactly one of** (a tunnel with neither is a load
  error; dbboard never blindly trusts an unverified host key):
  - `fingerprint` — a pinned `SHA256:…` server host-key fingerprint. The
    server is rejected on any mismatch.
  - `known_hosts` — path to an OpenSSH `known_hosts` file to verify the
    server key against.

The key **passphrase** and the SSH **password** are the only secrets;
they live in the keychain under `ssh_passphrase` / `ssh_password`
references (e.g. `dbboard.work-mysql.ssh_passphrase`). The bastion
host/port/user, the key **path**, and the host-key **fingerprint** are
non-secret and stay inline.

The desktop app edits all of this from the connection form: for a
tunnel-capable kind an **SSH tunnel** section appears with an enable
toggle, the bastion host/port/user, a key/password auth switch, and a
fingerprint/known-hosts host-key switch. As with every other secret, an
SSH passphrase or password field left blank when **editing** keeps the
stored secret untouched (see [ADR-0016](decisions.md)); for an encrypted
key, the *"key is encrypted"* checkbox tells dbboard to keep the stored
passphrase when you leave the field blank.

### What the file never contains

- D1 API tokens
- Postgres connection URLs that embed a password
- AWS secret access keys (for `aurora-dsql-iam`)
- SSH key passphrases and SSH passwords (for a tunneled connection)

These live only in the OS keychain. The TOML keeps the references.
(An `aurora-dsql-iam` entry's AWS **access key id** is a public
identifier and *is* kept inline — only the secret access key is a
secret. Likewise an SSH key **path** and host-key **fingerprint** are
non-secret and stay inline.)

## Seeding secrets

The connection window can add most kinds for you (typing the secret into a
masked field and seeding the keychain automatically), but the
`aurora-dsql-iam` kind is config-file-only, and some setups seed secrets
ahead of first launch. To seed the keychain by hand, use your OS's tooling.
The service is always `dbboard`; the *account* is the `keyring_*_ref` value
from the file (e.g. `dbboard.cf-d1-prod.token`).

- **Linux** (Secret Service, GNOME Keyring / KWallet):
  ```sh
  secret-tool store --label='dbboard cf-d1-prod token' \
    service dbboard account dbboard.cf-d1-prod.token
  ```
- **macOS**:
  ```sh
  security add-generic-password -s dbboard -a dbboard.cf-d1-prod.token -w
  ```
- **Windows** (`cmdkey`, PowerShell or cmd): the `keyring` crate maps
  `(service, account)` to a **Generic** credential whose target name is
  `<account>.dbboard` — the account first, with `.dbboard` **appended**
  (not prefixed). The credential's *user* is ignored on read, but set it to
  the account so the entry is self-describing:
  ```powershell
  cmdkey /generic:dbboard.cf-d1-prod.token.dbboard `
         /user:dbboard.cf-d1-prod.token `
         /pass:"THE_SECRET"
  ```
  The Credential Manager GUI works too: *Add a generic credential* →
  *Internet or network address* = `dbboard.cf-d1-prod.token.dbboard` (the
  `.dbboard` suffix is required — the GUI does not add it), *Password* =
  the secret.

A missing keychain entry surfaces as `ConfigError::Secret` at startup,
naming the reference that could not be resolved.

For a complete, worked Windows setup covering all three collector
connections, see [`collector-setup/README.md`](collector-setup/README.md).

## Moving connections between machines (encrypted bundle)

`connections.toml` alone is **useless on another machine** — it holds only
keyring *references*, and the keychain entries they point at do not exist
elsewhere. To move a whole connection set, export an **encrypted bundle**: a
single `.dbbx` file that carries the connection metadata **and** the secrets
it references, sealed with a passphrase you deliver out-of-band. See
[ADR-0038](decisions.md).

- **Format.** `.dbbx` is an [`age`](https://crates.io/crates/age)
  passphrase-encrypted blob (scrypt KDF + ChaCha20-Poly1305 AEAD). It is
  safe at rest and in transit; tampering is detected as corruption and a
  wrong passphrase is reported distinctly. Anyone with **both** the file and
  the passphrase has every secret — so keep the two on separate channels.

- **Export.** In the connection window, click **Export**, enter a passphrase
  (minimum 8 characters) and confirm it, then choose where to write the
  `.dbbx`. The bundle contains **all** saved connections plus every secret
  they reference, resolved from the keychain at export time. Since 0.11.0
  the connection list in that dialog has checkboxes, so a bundle can carry
  one connection rather than the whole store ([ADR-0105](decisions.md)).

- **Import.** Click **Import**, pick a `.dbbx` file, and enter its
  passphrase. dbboard decrypts the bundle, adds each connection, and seeds
  the secrets into this machine's keychain. The result reports how many
  connections were imported and how many were **skipped**:

  - a connection whose `id` already exists here is skipped (never
    overwritten), and
  - a connection whose keyring reference would target an already-claimed
    keychain slot is skipped (a guard against a crafted bundle overwriting
    an existing connection's secret).

  Skipped ids are listed so you can reconcile them by hand if needed.

The passphrase material and the decrypted plaintext (which briefly holds
every secret in the clear) are zeroized after use; the plaintext is never
written to disk unencrypted.

### Letting an agent make the bundle (`[mcp_export]`)

The five steps above — pick the connections, pick a directory, invent a
passphrase, save, keep the passphrase — are the right shape of work for an
agent and a tedious one by hand. `dbboard-mcp`'s `export_connections` does
the first four, and it is **off** until you name a directory for it:

```toml
version = 1

[mcp_export]
dir = "C:/Users/you/dbboard-bundles"
```

Three things to know before you add that table.

- **Naming the directory is the whole permission.** There is no separate
  switch to leave on afterwards; delete the table and the verb is closed.
  It lives here, in `connections.toml`, rather than in an environment
  variable on the MCP server, because that server's launcher config is
  usually a file the agent itself can edit — and a permission a tool can
  grant itself is not one ([ADR-0087](decisions.md)).

- **You never see the passphrase, and neither does the agent.** dbboard
  generates one, files it in this machine's keychain under
  `dbboard.export.<file stem>`, and reports only that name. **That entry is
  the only copy**: clear it before the bundle has been opened and the bundle
  is unopenable. The reason it works this way is that a tool result is
  plaintext in the agent's transcript, and a bundle that travels with its own
  key is not encrypted in any useful sense.

- **The agent names every connection it seals.** There is no export-everything
  form of this verb, so the request in the transcript is the record of what
  left the machine.

dbboard never creates the directory — a path that is not there is refused, on
the grounds that it usually means a typo and the wrong response to a typo is
a new folder full of credentials. Bundles are not overwritten and not pruned.
See [ADR-0140](decisions.md).

Importing is not exposed to agents at all. Reading a bundle back writes
credentials into this machine's keychain, so it stays something you do in the
app.

## File permissions and at-rest posture (ADR-0024)

dbboard tightens the per-user config files it creates against the
*"laptop lost or stolen"* threat model.

- **Unix (Linux, macOS):** `connections.toml`, its `connections.toml.tmp`
  sibling, and `history.jsonl` are created with mode `0o600`
  (owner-read-write only). On every append, `history.jsonl` is
  defensively re-tightened so a file that pre-dates ADR-0024 gets
  fixed automatically on the next write.
- **Windows:** files inherit the DACL of
  `%APPDATA%\Roaming\<user>\`, which grants
  `SYSTEM Full`, `Administrators Full`, `<user> Full`, and denies
  inheritance to other limited-priv accounts on the same machine.
  dbboard does not set an explicit DACL on each file — the workspace
  forbids `unsafe` (see `Cargo.toml`'s `unsafe_code = "forbid"`) and
  the inherited ACL is already restrictive on every supported
  Windows version.
- **OneDrive / iCloud Drive / Dropbox / Google Drive:** if the
  resolved config dir traverses a known cloud-sync vendor folder
  (e.g. OneDrive *Known Folder Move* relocates `%APPDATA%\Roaming\`
  under `%OneDrive%\`), dbboard logs one stderr warning at startup
  naming the vendor and the path. The binary keeps running — the
  user might want this — but the warning makes the cloud
  replication of `history.jsonl` visible. To exclude the dbboard
  config dir from OneDrive sync, follow Microsoft's *"Choose folders
  to sync"* guidance and uncheck the `dbboard\dbboard\config`
  subtree.
- **The single most effective hardening on a lost laptop is
  full-disk encryption.** Enable BitLocker (Windows), FileVault
  (macOS), or LUKS/dm-crypt (Linux). NTFS / POSIX permissions are
  only meaningful while the OS is booted; an attacker with the raw
  disk bypasses them.

The OS keychain (`KeyringStore`) is unaffected by any of the above —
secrets there are encrypted by the OS (DPAPI on Windows, Keychain on
macOS, Secret Service on Linux) and are not readable from a powered-off
disk even without full-disk encryption.
