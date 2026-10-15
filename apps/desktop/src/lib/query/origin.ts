// Where the rows on screen came from, and what that allows. Pure, so the rule
// that an edit goes back to the rows' own connection is unit-tested
// (origin.test.ts) instead of living in a `$derived` in the query panel.
import type { TableInfo } from '$lib/api';
import type { EditContext } from '$lib/grid/edit';

/** The connection, and the database within it (ADR-0162), that a result was
 *  read from. Captured when the statement starts, never looked up later: the
 *  sidebar may point somewhere else by the time anyone acts on the rows. */
export interface ResultOrigin {
  connectionId: string;
  database: string | null;
}

/** Whether rows read from `origin` still belong to the selected connection. */
export function belongsTo(origin: ResultOrigin | null, current: string | null): boolean {
  return origin !== null && origin.connectionId === current;
}

/** The grid's edit context, or `null` when the rows on screen must not be
 *  edited: a hand-written query, a table with no primary key, or rows read
 *  from a connection that is no longer the selected one. A primary key only
 *  identifies a row within the table it was read from, so an edit sent to
 *  any other connection would rewrite whichever row there has the same key. */
export function editContextFor(
  origin: ResultOrigin | null,
  current: string | null,
  table: TableInfo | null,
  pk: string[],
): EditContext | null {
  if (!origin || !belongsTo(origin, current) || !table || pk.length === 0) return null;
  return { connectionId: origin.connectionId, database: origin.database, table, pk };
}
