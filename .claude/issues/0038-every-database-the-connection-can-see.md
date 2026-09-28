# 0038: every database the connection can see, as a tree

- **Status**: open (planning, nothing built yet)
- **Slot**: v0.20 (`docs/roadmap.md`, added 2026-09-28)
- **Opened**: 2026-09-28
- **From**: the maintainer, while writing a MySQL connection by hand

## The friction

> dbboard って、データベース名を指定したのって、なにか理由ありましたっけ？
> MySQL はユーザー権限次第では、複数 DB 行き来できるので。
> 階層化するはずなんで。できるならそれにこしたことないというか、してほしい。

And, when an agent proposed doing it for MySQL only:

> MySQL だけっていうのは、ちがうとおもいますが。
> Postgres もそうじゃない？ できるやつすべて。

## What is true today

**Nobody decided that a connection holds one database.** No ADR requires it.
It is an assumption that was carried from the first adapter to the rest:

- The form rejects a blank database name for every URL-shaped connection
  (`apps/desktop/src/lib/connections/dsn.ts`, `validateDsn`). The only comment
  there is about the password. Pasting a URL instead skips the check.
- MySQL lists tables `WHERE table_schema = DATABASE()`
  (`crates/dbboard-mysql/src/lib.rs`, `LIST_TABLES_SQL`), commented "`MySQL`
  scopes tables to a single database per connection". **It does not.** An
  account sees every database it has privileges on. With no default database,
  `DATABASE()` is NULL and the list comes back **empty, with no error**. The
  form's required field is what hides that.
- The adapter itself connects without a database; sqlx treats it as optional.

**Half the model is already there.** `TableInfo.schema` carries MySQL's
database name (it selects `table_schema`), Postgres fills it with the schema,
and the qualified-name code (`qualified_ident`, write-back, DDL) already uses
it. The sidebar is a flat list that prints `schema.` as a prefix. Nothing
lists databases (`SHOW DATABASES`, `pg_database`, `listDatabases`).

## What each engine can do

| Engine | One connection can see | What a tree needs |
|---|---|---|
| **MySQL** | Every database the account has privileges on | Drop the `DATABASE()` filter when no database is set; exclude `mysql`, `information_schema`, `performance_schema`, `sys` |
| **PostgreSQL / Neon / Supabase** | Every schema, but **only inside the one database it connected to** | List `pg_database` (not templates, `datallowconn`), then open a pool **per database** when it is expanded. Cross-database queries do not exist in Postgres |
| **MongoDB** | Every database the user can read (`listDatabases`) | List databases; list collections per database. Today it is pinned to one (`self.db()`) |
| **Cloudflare D1** | One database per `database_id`, but the **same API token** can list the account's databases | `GET /accounts/{id}/d1/database`; one HTTP client per database id. Needs the token to carry `D1:Read` at account level; a token scoped to one database gets one entry |
| **Aurora DSQL** | One database, fixed (`postgres`) | Nothing: one level |
| **Turso / libSQL, SQLite** | One database | Nothing. Listing an organisation needs a Turso *platform* token, which the connection does not hold |
| **Firestore** | One database per connection | Listing a project's databases needs the Admin API and a different scope. Not now |

"できるやつすべて" is therefore MySQL, the Postgres family, MongoDB, and D1.
The rest keep one level, and the tree shows that level without a database
node above it rather than a parent with one child.

## Shape

- **One tree for every engine: database → schema → table.** A level with a
  single member is not drawn. MySQL has no schema level (its database *is*
  the schema), Postgres has both, D1 and SQLite have neither.
- **A new core call for listing databases**, with a default of "just the one
  I am connected to". Only adapters that can see more override it. Adapters
  still depend on `dbboard-core` only.
- **The database name becomes optional** in the form for engines that can
  list. Blank means "everything this account can see". A name means the same
  as today: that database only. **Existing connections look the same as before.**
- **Expanding a database is what opens it** where opening costs something
  (a Postgres pool, a D1 client). Listing fifty databases must not open fifty
  pools.
- **Hand-written SQL runs against the default.** On MySQL with no default,
  `db.table` works and a bare `table` fails with the server's own "No database
  selected". Choosing a default per query tab is a separate design (a pooled
  `USE` is not reliable: it lands on one of up to five connections).

## Order

1. **MySQL.** Smallest change, and the engine the friction came from. The tree
   in the sidebar lands with it, because a flat list of every table on a
   server is worse than today.
2. **Postgres family.** The per-database pool is the real work.
3. **MongoDB**, then **D1**.

Each step is shippable alone. A step not finished when v0.20 is cut moves to
the next slot (ADR-0110).

## Before it merges

- **v0.19 is cut first.** Merging into `develop` before then would put this in
  0.19's changelog under "The measurement nobody has taken".
- An ADR for the core call and the tree, and a test-first path per engine.
- The HTTP contract (`docs/api-contract.md`) is frozen at v1.0. If the database
  list is exposed there, it has to be in before then; this slot is ahead of
  v1.0 on purpose.
