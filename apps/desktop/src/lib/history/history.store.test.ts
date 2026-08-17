// Tests for the reactive history store. The pure list logic underneath is
// covered by ./history.test.ts; what matters here is that reading is a *read*.
import { beforeEach, describe, expect, it } from 'vitest';
import { queryHistory } from './history.svelte';

let store = new Map<string, string>();

function put(connectionId: string, entries: unknown): void {
  store.set(`dbboard.history.${connectionId}`, JSON.stringify(entries));
}

beforeEach(() => {
  store = new Map();
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
    clear: () => store.clear(),
    key: () => null,
    get length() {
      return store.size;
    },
  } as Storage;
});

describe('queryHistory.for()', () => {
  it('returns the persisted entries for a connection', () => {
    put('conn-a', [{ sql: 'SELECT 1', at: 7 }]);

    expect(queryHistory.for('conn-a')).toEqual([{ sql: 'SELECT 1', at: 7 }]);
  });

  it('returns an empty list for a connection with no history', () => {
    expect(queryHistory.for('conn-blank')).toEqual([]);
  });

  // `for()` used to cache the parsed list into reactive state on first read.
  // QueryPanel reads it from a `$derived`, and writing reactive state while a
  // derived is being evaluated throws `state_unsafe_mutation` — which killed
  // the template effect that renders the editor bar, freezing every binding in
  // it (most visibly, the language switch stopped reaching that bar). Reading
  // must therefore stay free of writes, which is observable here as "a later
  // read sees a later value".
  it('does not cache on read', () => {
    expect(queryHistory.for('conn-b')).toEqual([]);

    put('conn-b', [{ sql: 'SELECT 2', at: 11 }]);

    expect(queryHistory.for('conn-b')).toEqual([{ sql: 'SELECT 2', at: 11 }]);
  });
});

describe('queryHistory.record()', () => {
  it('is visible to a reader that saw an empty list first', () => {
    expect(queryHistory.for('conn-c')).toEqual([]);

    queryHistory.record('conn-c', 'SELECT 3', 13);

    expect(queryHistory.for('conn-c')).toEqual([{ sql: 'SELECT 3', at: 13 }]);
  });

  it('persists so the entry survives a reload', () => {
    queryHistory.record('conn-d', 'SELECT 4', 17);

    expect(store.get('dbboard.history.conn-d')).toBe(
      JSON.stringify([{ sql: 'SELECT 4', at: 17 }]),
    );
  });
});
