// Pure SQL-text builders shared by the sidebar context menu and (later) other
// "generate a query for me" affordances. No I/O, no Svelte — unit-tested in
// build.test.ts.
//
// Identifier quoting is dialect-aware. It used to be unconditionally `"…"`,
// which was true while we only shipped Postgres-family and SQLite/libSQL
// adapters — but MySQL (ADR-0068) reads `"orders"` as a *string literal*
// unless the server runs with ANSI_QUOTES, so a generated `SELECT * FROM
// "shop"."orders"` is a syntax error there, not a subtly wrong result.
import type { TableInfo } from '$lib/api';

/** How identifiers are quoted. `ansi` is `"…"` (Postgres family, SQLite,
 *  libSQL, D1); `mysql` is `` `…` ``. */
export type SqlDialect = 'ansi' | 'mysql';

/** Map a `ConnectionView.kind` slug onto its quoting dialect. Unknown and
 *  missing kinds fall back to ANSI: it is what every adapter except MySQL
 *  accepts, so it is the safe guess for an adapter added after this code. */
export function dialectForKind(kind: string | undefined): SqlDialect {
  return kind === 'mysql' ? 'mysql' : 'ansi';
}

/** Quote an identifier for `dialect`, doubling the embedded quote character so
 *  a name can never break out of the quoting. Only the dialect's *own* quote
 *  character is doubled — a `"` inside a back-quoted MySQL identifier is an
 *  ordinary character and must pass through untouched. */
export function quoteIdent(name: string, dialect: SqlDialect = 'ansi'): string {
  if (dialect === 'mysql') {
    return `\`${name.replace(/`/g, '``')}\``;
  }
  return `"${name.replace(/"/g, '""')}"`;
}

/** Quote a table for use in FROM: `"schema"."name"` when schema-qualified,
 *  `"name"` otherwise (back-ticked in the MySQL dialect). */
export function qualifiedName(
  table: TableInfo,
  dialect: SqlDialect = 'ansi',
): string {
  return table.schema
    ? `${quoteIdent(table.schema, dialect)}.${quoteIdent(table.name, dialect)}`
    : quoteIdent(table.name, dialect);
}

/** A read-only `SELECT * ... LIMIT n` for the given table. `n` is floored to a
 *  positive integer so the generated SQL is always well-formed and bounded. */
export function selectTopN(
  table: TableInfo,
  n: number,
  dialect: SqlDialect = 'ansi',
): string {
  const limit = Math.max(1, Math.floor(n));
  return `SELECT * FROM ${qualifiedName(table, dialect)} LIMIT ${limit};`;
}

/** `SELECT COUNT(*)` for the given table — the "how big is this?" starter the
 *  egui build has always offered on the table right-click menu. */
export function countRows(
  table: TableInfo,
  dialect: SqlDialect = 'ansi',
): string {
  return `SELECT COUNT(*) FROM ${qualifiedName(table, dialect)};`;
}

/** Kinds whose query text is not SQL at all. Everything below this line exists
 *  because a dialect is a *spelling* difference, and Firestore's is not one:
 *  it runs a `StructuredQuery` in JSON (ADR-0093), so no amount of quoting
 *  makes `SELECT * FROM …` reach it. */
const STRUCTURED_QUERY_KINDS = ['firestore', 'mongodb'];

/** Whether `kind` states its queries as a `StructuredQuery` instead of SQL.
 *
 *  Also the answer to "can the result grid write a cell back?": the write path
 *  composes an `UPDATE`, so a connection that cannot be sent SQL cannot be
 *  edited inline either, however many primary keys its schema declares. */
export function usesStructuredQuery(kind: string | undefined): boolean {
  return kind !== undefined && STRUCTURED_QUERY_KINDS.includes(kind);
}

/** A bounded Firestore `StructuredQuery`, as the JSON text the editor shows.
 *
 *  Written out rather than `JSON.stringify`d with an indent: this lands in an
 *  editor for a human to extend with `where`/`orderBy`, and one collection per
 *  line reads as a query where a four-line `from` array reads as a data dump.
 *  `table.schema` is ignored — Firestore's `list_tables` reports top-level
 *  collection ids, so the collection id is the whole identity. */
export function structuredQuery(table: TableInfo, n: number): string {
  const limit = Math.max(1, Math.floor(n));
  // The collection id is user data going into JSON, so it is escaped by the
  // serialiser rather than by hand-written quotes.
  const collection = JSON.stringify(table.name);
  return `{\n  "from": [{ "collectionId": ${collection} }],\n  "limit": ${limit}\n}`;
}

/** A bounded MongoDB `find` command, as the JSON text the editor shows.
 *
 *  Also JSON, but not Firestore's JSON: the adapter dispatches on the first
 *  key, so `find` and `from` are two different commands and one builder cannot
 *  serve both. `table.schema` is ignored for the same reason as above — the
 *  collection name is the whole identity. */
export function mongoFindCommand(table: TableInfo, n: number): string {
  const limit = Math.max(1, Math.floor(n));
  const collection = JSON.stringify(table.name);
  return `{\n  "find": ${collection},\n  "limit": ${limit}\n}`;
}

/** The "show me this table's first rows" query for a connection of `kind`,
 *  in whatever language that connection speaks. */
export function browseQuery(
  table: TableInfo,
  n: number,
  kind: string | undefined,
): string {
  if (kind === 'firestore') return structuredQuery(table, n);
  if (kind === 'mongodb') return mongoFindCommand(table, n);
  return selectTopN(table, n, dialectForKind(kind));
}

/** The "how big is this?" query, or `null` where the connection has no way to
 *  answer it. Firestore counts through `:runAggregationQuery`, a separate
 *  endpoint the read-only adapter does not implement, so the caller drops the
 *  affordance instead of offering one that can only fail. MongoDB does have
 *  one — `count` is on the adapter's read allowlist — so this branches per
 *  kind rather than dropping the action for structured queries as a class. */
export function countQuery(
  table: TableInfo,
  kind: string | undefined,
): string | null {
  if (kind === 'firestore') return null;
  if (kind === 'mongodb') return `{ "count": ${JSON.stringify(table.name)} }`;
  return countRows(table, dialectForKind(kind));
}
