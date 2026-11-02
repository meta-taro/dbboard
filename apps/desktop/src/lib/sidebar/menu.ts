// The table right-click menu, as data. Kept out of the Svelte component so
// the part that matters — *which* actions exist and what SQL each generates —
// is unit-testable without a DOM; the component only maps these onto labels
// and side effects.
import { tableKey, type TableInfo } from '$lib/api';
import { browseQuery, countQuery } from '$lib/sql/build';

/** Rows a "select top N" browse fetches. Matches the row cap the query panel
 *  treats as an editable browse (ADR-0042). */
export const BROWSE_ROWS = 100;

export type TableMenuAction =
  | { id: 'open-structure' }
  | { id: 'select-top'; n: number; sql: string }
  | { id: 'count-rows'; sql: string }
  | { id: 'copy-name'; text: string };

/** Actions offered when right-clicking `table` on a connection of `kind`.
 *  Read-only by design (no DELETE/DROP): this ships to a data-collection user,
 *  so a mis-click must never be destructive — which is also what makes it safe
 *  for the browse and count entries to run immediately. */
export function tableMenuActions(
  table: TableInfo,
  kind: string | undefined,
): TableMenuAction[] {
  const count = countQuery(table, kind);
  return [
    { id: 'open-structure' },
    {
      id: 'select-top',
      n: BROWSE_ROWS,
      sql: browseQuery(table, BROWSE_ROWS, kind),
    },
    // Omitted rather than disabled where the connection cannot count at all
    // (Firestore): a greyed-out entry still reads as "this exists somewhere",
    // which is the wrong thing to tell someone about an endpoint we do not
    // implement.
    ...(count === null ? [] : [{ id: 'count-rows' as const, sql: count }]),
    { id: 'copy-name', text: tableKey(table) },
  ];
}
