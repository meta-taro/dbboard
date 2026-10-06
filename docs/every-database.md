# Every database the connection can see

Planned for **v0.20** ([roadmap](roadmap.md)). This note explains why a
connection should stop meaning "one database", what each engine allows, and
the order the work is done in.

## Why

The connection form required a database name for every URL-shaped engine.
No decision record asks for that. The requirement came over from the first
adapter and stayed.

For MySQL it hid a real problem. The adapter listed tables with
`WHERE table_schema = DATABASE()`, on the assumption that a MySQL connection
holds one database. It does not: an account can read every database it has
privileges on. A connection with no default database has `DATABASE()` =
NULL, so the comparison is never true and the table list comes back
**empty, with no error**. The required field was what kept anyone from
seeing that.

The goal is a single tree for every engine. Where an engine can see more than
one database from one connection, dbboard shows them all.

## What is true today

- The form rejects a blank database for every URL-shaped connection
  (`apps/desktop/src/lib/connections/dsn.ts`, `validateDsn`). Pasting a URL
  instead of using the fields skips the check.
- The MySQL adapter connects fine without a database. Only the listing
  assumes one.
- **Half the model already exists.** `TableInfo.schema` carries MySQL's
  database name and Postgres's schema, and the code that qualifies names
  (DDL, write-back, browsing) already uses it. The sidebar is a flat list that
  prints `schema.` as a prefix. Nothing lists databases yet (`SHOW
  DATABASES`, `pg_database`, `listDatabases`).

## What each engine can do

| Engine | One connection can see | What a tree needs |
|---|---|---|
| **MySQL** | Every database the account has privileges on | Drop the `DATABASE()` filter when no database is set, and leave out `mysql`, `information_schema`, `performance_schema` and `sys` |
| **PostgreSQL / Neon / Supabase** | Every schema, but **only inside the database it connected to** | List `pg_database` (no templates, `datallowconn` only), then open a pool **per database** when that database is expanded. Postgres has no cross-database queries |
| **MongoDB** | Every database the user can read (`listDatabases`) | List databases, then collections per database. Today the adapter is pinned to one |
| **Cloudflare D1** | One database per `database_id`, but the **same API token** can list the account's databases | `GET /accounts/{id}/d1/database`, and one HTTP client per database id. This needs the token to carry account-level D1 read access; a token scoped to one database lists one entry |
| **Aurora DSQL** | One fixed database | Nothing: one level |
| **Turso / libSQL, SQLite** | One database | Nothing. Listing a Turso organisation needs a platform token, which a connection does not hold |
| **Firestore** | One database per connection | Listing a project's databases needs the Admin API and a different scope. Not planned |

So the tree covers MySQL, the Postgres family, MongoDB and D1. The other
engines keep one level, and the tree draws that level without a database
node above it rather than a parent with a single child.

## Shape

- **One tree for every engine: database → schema → table.** A level with a
  single member is not drawn. MySQL has no schema level (its database *is*
  the schema). Postgres has both levels. D1 and SQLite have neither.
- **A new core call that lists databases.** By default it returns "just the
  one I am connected to", and only adapters that can see more override it.
  Adapters still depend on `dbboard-core` only.
- **The database name becomes optional** in the form for the engines that
  can list. Blank means "everything this account can see". A name means what
  it means today: that database only. **Existing connections look the same
  as before.**
- **Expanding a database opens it** where opening costs something (a
  Postgres pool, a D1 client). Listing fifty databases must not open fifty
  pools.
- **Hand-written SQL runs against the default database.** On a MySQL
  connection with no default, `db.table` works, and a bare `table` fails with
  the server's own "No database selected". Choosing a default per query tab
  needs its own design: `USE` on a pool of five connections only affects
  whichever connection runs it.

## Order

1. **MySQL.** It is the smallest change, and the engine where the problem
   showed up. The sidebar tree lands with it, because a flat list of every
   table on a server is worse than what it replaces.
2. **The Postgres family.** The per-database pool is the real work.
3. **MongoDB**, then **D1**.

**Status (2026-10-06):** steps 1 and 2 are done (ADR-0161, ADR-0162,
ADR-0163). v0.20 ships with MySQL and the Postgres family, and MongoDB and D1
move to a later slot.

Each step can ship on its own. A step that is not finished when v0.20 is cut
moves to the next slot, as every slot's unfinished content does.

## Constraints

- v0.20 is placed before v1.0 on purpose. The HTTP contract
  (`docs/api-contract.md`) freezes at v1.0, so if the database list is
  exposed there, it has to be in before then.
- Each step gets an ADR and a test-first path per engine.
