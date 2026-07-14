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
5. The `DBBOARD_D1_*` trio (account id + database id + token).
6. `DBBOARD_TURSO_PATH` (explicit local libSQL path).
7. `DBBOARD_CONNECTION=<id>` matched against `connections.toml`. A
   missing id aborts startup — dbboard refuses to silently fall back to
   a different backend than the user asked for.
8. If `connections.toml` contains exactly one entry, that one is
   auto-selected.
9. Otherwise an in-memory Turso/libSQL database (`:memory:`).

`DBBOARD_AURORA_DSQL_URL`, `DBBOARD_NEON_URL`, and
`DBBOARD_SUPABASE_URL` all outrank `DBBOARD_PG_URL` because they carry
more specific labelling. Among the pg-wire flavors the order is
alphabetical: `aurora-dsql` > `neon` > `supabase`; setting two
flavored vars at once is unusual but the precedence is fully defined.

## TOML schema

```toml
version = 1

[[connections]]
id   = "local-libsql"
name = "Local libSQL"
kind = "turso"
path = ":memory:"

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
```

### Fields

- `version` — currently `1`. dbboard refuses any other value rather
  than guessing at a forward- or backward-incompatible shape.
- `id` — primary key referenced by `DBBOARD_CONNECTION`. Duplicate ids
  are a hard error at load time.
- `name` — display label for the (future) connection picker.
- `kind` — `"turso"`, `"d1"`, `"postgres"`, `"neon"`, `"supabase"`,
  `"aurora-dsql"`, or `"aurora-dsql-iam"`. `"neon"`, `"supabase"`,
  `"aurora-dsql"`, and `"postgres"` share the same wire shape (the
  keyring carries a `postgres://…` URL either way); the only difference
  is the runtime adapter label, which the connection picker and history
  records read. `"aurora-dsql-iam"` is the exception: it carries its
  fields inline (`endpoint`, `region`, `database`, `username`,
  `access_key_id`) and stores only the AWS secret access key in the
  keychain, because dbboard mints the IAM token itself (see below).
- `keyring_*_ref` — opaque account string used to look up the secret
  in the OS keychain. Pick something stable and recognisable; the
  string is what shows in the OS UI alongside the constant service
  name `"dbboard"`.

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
  continuously refreshed inside a live pool. A held-open connection stays
  authenticated indefinitely, but a *cold reconnect* more than ~15
  minutes after the last build fails until you switch connections (or
  restart) to re-mint. Automatic in-pool refresh is a planned follow-up.
  This kind is created by hand-editing `connections.toml`; the in-app
  connection list can connect and delete it, but not yet edit it.

### What the file never contains

- D1 API tokens
- Postgres connection URLs that embed a password
- AWS secret access keys (for `aurora-dsql-iam`)

These live only in the OS keychain. The TOML keeps the references.
(An `aurora-dsql-iam` entry's AWS **access key id** is a public
identifier and *is* kept inline — only the secret access key is a
secret.)

## Seeding secrets

There is no in-app UI for editing the store yet (Phase 2 task). For now,
seed the keychain by hand with your OS's tooling:

- **Linux** (Secret Service, GNOME Keyring / KWallet):
  ```sh
  secret-tool store --label='dbboard cf-d1-prod token' \
    service dbboard account dbboard.cf-d1-prod.token
  ```
- **macOS**:
  ```sh
  security add-generic-password -s dbboard -a dbboard.cf-d1-prod.token -w
  ```
- **Windows**: Credential Manager → Windows Credentials → Add a generic
  credential. *Internet or network address* = the account string
  (e.g. `dbboard.cf-d1-prod.token`), *User name* = anything (ignored),
  *Password* = the secret. The service `dbboard` is prefixed by the
  `keyring` crate automatically.

A missing keychain entry surfaces as `ConfigError::Secret` at startup,
naming the reference that could not be resolved.

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
