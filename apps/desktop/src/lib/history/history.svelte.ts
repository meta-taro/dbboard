// Reactive, per-connection SQL history backed by localStorage. Wraps the pure
// logic in ./history with Svelte 5 reactive state so the history dropdown
// updates the moment a query runs, and persists across restarts.
import { addEntry, parseHistory, type HistoryEntry } from './history';

const KEY_PREFIX = 'dbboard.history.';

function storageKey(connectionId: string): string {
  return KEY_PREFIX + connectionId;
}

function read(connectionId: string): HistoryEntry[] {
  try {
    return parseHistory(localStorage.getItem(storageKey(connectionId)));
  } catch {
    return [];
  }
}

function write(connectionId: string, entries: HistoryEntry[]): void {
  try {
    localStorage.setItem(storageKey(connectionId), JSON.stringify(entries));
  } catch {
    // Non-fatal: history just won't persist (private mode / quota).
  }
}

class QueryHistory {
  // Keyed by connection id. A plain reactive object is enough — we replace the
  // whole per-connection array on each change so runes see the update.
  private byConnection = $state<Record<string, HistoryEntry[]>>({});

  /** History for one connection, most-recent-first. Lazily loaded from disk. */
  for(connectionId: string): HistoryEntry[] {
    if (!(connectionId in this.byConnection)) {
      this.byConnection[connectionId] = read(connectionId);
    }
    return this.byConnection[connectionId];
  }

  /** Record a successfully-run query. `at` defaults to now. */
  record(connectionId: string, sql: string, at: number = Date.now()): void {
    const next = addEntry(this.for(connectionId), sql, at);
    this.byConnection[connectionId] = next;
    write(connectionId, next);
  }

  /** Forget all history for one connection. */
  clear(connectionId: string): void {
    this.byConnection[connectionId] = [];
    write(connectionId, []);
  }
}

export const queryHistory = new QueryHistory();
