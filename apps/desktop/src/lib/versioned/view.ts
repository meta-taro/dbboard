// What the History tab draws, worked out from what the backend returns
// (ADR-0165). Pure functions, so the marking rules are tested without a
// component: which cell counts as changed, what a removed row shows, which
// commit a diff is taken against.
//
// The shapes mirror `dbboard-core`'s history types, snake_case as on the wire.
import { displayCell, type Cell, type Column } from '$lib/api';
import type { KeyColumn } from '$lib/grid/edit';

export interface Commit {
  hash: string;
  author: string;
  email: string;
  date: string;
  message: string;
}

export interface CommitLogView {
  connection_id: string;
  /** False for a connection that keeps no history; the tab is then hidden. */
  available: boolean;
  /** Newest first. */
  commits: Commit[];
}

export type ChangeKind = 'added' | 'modified' | 'removed';

export interface RowChange {
  kind: ChangeKind;
  before: Cell[] | null;
  after: Cell[] | null;
  changed_columns: number[];
}

export interface CommitDiff {
  columns: Column[];
  changes: RowChange[];
}

export interface RowVersion {
  commit: Commit;
  values: Cell[];
}

export interface RowHistory {
  columns: Column[];
  /** Newest first. */
  versions: RowVersion[];
}

export interface BlameEntry {
  key: Cell[];
  commit: Commit;
}

/** One drawn cell. `before` is set only on a changed cell that had a value
 *  to change from, so the grid can show both without repeating the rest. */
export interface DrawnCell {
  text: string;
  changed: boolean;
  before: string | null;
}

export interface DiffRow {
  kind: ChangeKind;
  cells: DrawnCell[];
  /** The row as it stands on whichever side it exists: after, or before for
   *  a removed row. What a key for its history is read from. */
  values: Cell[];
}

export interface VersionRow {
  commit: Commit;
  cells: DrawnCell[];
}

export interface BlameRow {
  cells: string[];
  key: KeyColumn[];
  commit: Commit;
}

/**
 * Whether a history read was refused because the table keeps only its
 * latest value, as an Umbrachyx table can be declared to (ADR-0173). The
 * screen says so as a note: nothing went wrong, there is no history.
 */
export function keepsNoHistory(error: string): boolean {
  return (
    error.startsWith('capability unavailable:') && error.includes('keeps only its latest value')
  );
}

/** The leading seven characters, as git and Dolt print a hash. */
export function shortHash(hash: string): string {
  return hash.slice(0, 7);
}

/**
 * The commit to diff `hash` against: the next older one in a newest-first
 * log. Null for the oldest, and for a hash the log does not hold.
 *
 * This is the previous commit in the log, which for a merge is the
 * first-parent side — the same answer `git log --first-parent` gives.
 */
export function parentOf(log: Commit[], hash: string): string | null {
  const at = log.findIndex((c) => c.hash === hash);
  if (at < 0 || at + 1 >= log.length) return null;
  return log[at + 1].hash;
}

export function diffRows(diff: CommitDiff): DiffRow[] {
  return diff.changes.map((change) => {
    const values = change.after ?? change.before ?? [];
    // Only a modified row marks cells. An added or removed row changed as a
    // whole, which the row's own marker already says; highlighting every
    // cell of it as well would drown the modified rows that need reading.
    const changed =
      change.kind === 'modified' ? new Set(change.changed_columns) : new Set<number>();
    return {
      kind: change.kind,
      values,
      cells: values.map((cell, i) => drawn(cell, changed.has(i) ? change.before?.[i] : undefined)),
    };
  });
}

/** Each version, with the cells that differ from the next older one marked. */
export function versionRows(history: RowHistory): VersionRow[] {
  return history.versions.map((version, i) => {
    const older = history.versions[i + 1]?.values;
    return {
      commit: version.commit,
      cells: version.values.map((cell, c) =>
        drawn(cell, older && !sameCell(older[c], cell) ? older[c] : undefined),
      ),
    };
  });
}

/**
 * The primary key of `row`, in the key's own column order, ready to ask for
 * the row's history. Null when the table has no primary key or the row lacks
 * one of its columns: a partial key would match other rows too.
 */
export function keyFor(
  columns: Column[],
  primaryKey: string[],
  row: Cell[],
): KeyColumn[] | null {
  if (primaryKey.length === 0) return null;
  const key: KeyColumn[] = [];
  for (const name of primaryKey) {
    const at = columns.findIndex((c) => c.name === name);
    if (at < 0 || at >= row.length) return null;
    key.push({ column: name, value: row[at] });
  }
  return key;
}

/** Blame entries as table rows. Dolt returns the key in primary-key order,
 *  which is the order `primaryKey` is given in. */
export function blameRows(primaryKey: string[], entries: BlameEntry[]): BlameRow[] {
  return entries.map((entry) => ({
    cells: entry.key.map(displayCell),
    key: primaryKey.map((column, i) => ({ column, value: entry.key[i] ?? null })),
    commit: entry.commit,
  }));
}

/** `before` undefined means unchanged; anything else, including null (SQL
 *  NULL), is the value the cell changed from. */
function drawn(cell: Cell, before: Cell | undefined): DrawnCell {
  return {
    text: displayCell(cell),
    changed: before !== undefined,
    before: before === undefined ? null : displayCell(before),
  };
}

function sameCell(a: Cell | undefined, b: Cell): boolean {
  if (a === b) return true;
  if (typeof a === 'object' && typeof b === 'object') {
    return JSON.stringify(a) === JSON.stringify(b);
  }
  return false;
}
