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
