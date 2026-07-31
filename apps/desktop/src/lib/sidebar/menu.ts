// The table right-click menu, as data. Kept out of the Svelte component so
// the part that matters — *which* actions exist and what SQL each generates —
// is unit-testable without a DOM; the component only maps these onto labels
// and side effects.
import { tableKey, type TableInfo } from '$lib/api';
import { countRows, dialectForKind, selectTopN } from '$lib/sql/build';

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
  const dialect = dialectForKind(kind);
  return [
    { id: 'open-structure' },
    {
      id: 'select-top',
      n: BROWSE_ROWS,
      sql: selectTopN(table, BROWSE_ROWS, dialect),
    },
    { id: 'count-rows', sql: countRows(table, dialect) },
    { id: 'copy-name', text: tableKey(table) },
  ];
}
