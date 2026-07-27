// Pure SQL-text builders shared by the sidebar context menu and (later) other
// "generate a query for me" affordances. No I/O, no Svelte — unit-tested in
// build.test.ts. Identifiers are always double-quoted, which every engine we
// target (Postgres-family, SQLite/libSQL) accepts.
import type { TableInfo } from '$lib/api';

/** Double-quote an identifier, doubling any embedded quote so a name can never
 *  break out of the quoting. */
export function quoteIdent(name: string): string {
  return `"${name.replace(/"/g, '""')}"`;
}

/** Quote a table for use in FROM: `"schema"."name"` when schema-qualified,
 *  `"name"` otherwise. */
export function qualifiedName(table: TableInfo): string {
  return table.schema
    ? `${quoteIdent(table.schema)}.${quoteIdent(table.name)}`
    : quoteIdent(table.name);
}

/** A read-only `SELECT * ... LIMIT n` for the given table. `n` is floored to a
 *  positive integer so the generated SQL is always well-formed and bounded. */
export function selectTopN(table: TableInfo, n: number): string {
  const limit = Math.max(1, Math.floor(n));
  return `SELECT * FROM ${qualifiedName(table)} LIMIT ${limit};`;
}
