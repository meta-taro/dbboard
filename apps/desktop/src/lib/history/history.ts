// Pure SQL-history logic, framework- and storage-free so it is unit-testable.
// The reactive store (./history.svelte) and localStorage persistence layer on
// top of these functions.

export interface HistoryEntry {
  /** The exact SQL text that was run (trimmed of surrounding whitespace). */
  sql: string;
  /** Epoch milliseconds when it was last run. */
  at: number;
}

/** Default cap on remembered queries per connection. */
export const HISTORY_MAX = 50;

/**
 * Add `sql` to the front of `list`, most-recent-first, de-duplicating on the
 * trimmed text. Re-running an existing query moves it to the front (and updates
 * its timestamp) rather than creating a duplicate. Blank/whitespace SQL is
 * ignored. The result is capped at `max` entries.
 */
export function addEntry(
  list: HistoryEntry[],
  sql: string,
  at: number,
  max: number = HISTORY_MAX,
): HistoryEntry[] {
  const trimmed = sql.trim();
  if (!trimmed) return list;

  const withoutDup = list.filter((e) => e.sql !== trimmed);
  return [{ sql: trimmed, at }, ...withoutDup].slice(0, max);
}

/**
 * Parse a persisted history payload defensively. Anything malformed (not an
 * array, wrong shape) yields an empty list rather than throwing, so a corrupt
 * localStorage value can never crash the editor.
 */
export function parseHistory(raw: string | null): HistoryEntry[] {
  if (!raw) return [];
  try {
    const data = JSON.parse(raw);
    if (!Array.isArray(data)) return [];
    return data
      .filter(
        (e): e is HistoryEntry =>
          !!e && typeof e.sql === 'string' && typeof e.at === 'number',
      )
      .map((e) => ({ sql: e.sql, at: e.at }));
  } catch {
    return [];
  }
}
