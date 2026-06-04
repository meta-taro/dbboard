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
```

### Fields

- `version` — currently `1`. dbboard refuses any other value rather
  than guessing at a forward- or backward-incompatible shape.
- `id` — primary key referenced by `DBBOARD_CONNECTION`. Duplicate ids
  are a hard error at load time.
- `name` — display label for the (future) connection picker.
- `kind` — `"turso"`, `"d1"`, `"postgres"`, `"neon"`, `"supabase"`,
  or `"aurora-dsql"`. `"neon"`, `"supabase"`, `"aurora-dsql"`, and
  `"postgres"` share the same wire shape (the keyring carries a
  `postgres://…` URL either way); the only difference is the runtime
  adapter label, which the connection picker and history records read.
- `keyring_*_ref` — opaque account string used to look up the secret
  in the OS keychain. Pick something stable and recognisable; the
  string is what shows in the OS UI alongside the constant service
  name `"dbboard"`.

### What the file never contains

- D1 API tokens
- Postgres connection URLs that embed a password

These live only in the OS keychain. The TOML keeps the references.

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
