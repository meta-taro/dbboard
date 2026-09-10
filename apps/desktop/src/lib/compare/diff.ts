// Pure helpers for the Compare tab (ADR-0148 + ADR-0149). The comparison
// itself happens in `dbboard-core`; what is here is what the screen needs on
// top of the result — which connections may be offered, which sidebar rows
// carry a marker, and how many findings a table holds. Free of Svelte and
// Tauri so it is unit-testable.
import { tableKey, type ConnectionView, type SchemaDiff, type TableDiff, type TableInfo } from '$lib/api';

/** Why a table is in the report, from the sidebar's point of view. */
export type DiffMark = 'left-only' | 'right-only' | 'changed';

/**
 * The connections that may be compared with `leftId`: same engine, and not
 * itself.
 *
 * The backend refuses a cross-engine pair before it dials anything (ADR-0148),
 * so offering one here would make the app propose something it is about to
 * decline. An empty result is a fact the picker states ("no other postgres
 * connection"), not an empty menu.
 */
export function comparableWith(
  connections: ConnectionView[],
  leftId: string,
): ConnectionView[] {
  const left = connections.find((c) => c.id === leftId);
  if (!left) return [];
  return connections.filter((c) => c.id !== leftId && c.kind === left.kind);
}

/**
 * Every table the report mentions, keyed exactly as the sidebar keys its rows,
 * so a marker can be looked up per row without re-deriving the name.
 */
export function differingTables(diff: SchemaDiff): Map<string, DiffMark> {
  const marks = new Map<string, DiffMark>();
  for (const t of diff.tables_only_in_left) marks.set(tableKey(t), 'left-only');
  for (const t of diff.tables_only_in_right) marks.set(tableKey(t), 'right-only');
  for (const t of diff.tables_changed) marks.set(tableKey(t.table), 'changed');
  return marks;
}

/**
 * How many findings one table holds.
 *
 * A changed column counts once however many of its attributes differ: the
 * number is there to tell a person how much there is to look at, and they look
 * at a column, not at four field-level facts about it. The primary key counts
 * once when the two sides disagree, and each foreign-key finding once —
 * a key that points somewhere else is one more thing to check, not a footnote
 * to the column it sits on.
 */
export function tableDifferenceCount(t: TableDiff): number {
  return (
    t.columns_only_in_left.length +
    t.columns_only_in_right.length +
    t.columns_changed.length +
    (t.primary_key === null ? 0 : 1) +
    t.foreign_keys_only_in_left.length +
    t.foreign_keys_only_in_right.length +
    t.foreign_keys_changed.length +
    t.indexes_only_in_left.length +
    t.indexes_only_in_right.length +
    t.indexes_changed.length
  );
}

/** How many tables the report mentions at all. */
export function totalDifferingTables(diff: SchemaDiff): number {
  return (
    diff.tables_only_in_left.length +
    diff.tables_only_in_right.length +
    diff.tables_changed.length
  );
}

/**
 * The DOM id of a table's section, so the sidebar can scroll the report to it.
 *
 * Table names may hold spaces, dots and quotes; the id reaches
 * `getElementById`, so the key is encoded rather than interpolated. Base64 of
 * the UTF-8 bytes, made id-safe — reversible in principle, though nothing
 * needs to reverse it.
 */
export function sectionId(table: TableInfo): string {
  const key = tableKey(table);
  const bytes = new TextEncoder().encode(key);
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  const encoded = btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  return `cmp-${encoded}`;
}
