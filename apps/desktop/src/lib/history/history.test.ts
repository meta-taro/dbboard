import { describe, it, expect } from 'vitest';
import { addEntry, parseHistory, HISTORY_MAX, type HistoryEntry } from './history';

describe('addEntry', () => {
  it('prepends a new entry most-recent-first', () => {
    const a = addEntry([], 'SELECT 1', 100);
    const b = addEntry(a, 'SELECT 2', 200);
    expect(b.map((e) => e.sql)).toEqual(['SELECT 2', 'SELECT 1']);
  });

  it('trims surrounding whitespace before storing', () => {
    const list = addEntry([], '  SELECT 1  ', 100);
    expect(list[0].sql).toBe('SELECT 1');
  });

  it('ignores blank or whitespace-only SQL', () => {
    expect(addEntry([], '   ', 100)).toEqual([]);
    expect(addEntry([], '', 100)).toEqual([]);
  });

  it('de-duplicates: re-running moves the entry to the front with a new time', () => {
    let list: HistoryEntry[] = [];
    list = addEntry(list, 'SELECT 1', 100);
    list = addEntry(list, 'SELECT 2', 200);
    list = addEntry(list, 'SELECT 1', 300);
    expect(list.map((e) => e.sql)).toEqual(['SELECT 1', 'SELECT 2']);
    expect(list[0].at).toBe(300);
  });

  it('treats trimmed-equal SQL as the same entry', () => {
    let list = addEntry([], 'SELECT 1', 100);
    list = addEntry(list, '  SELECT 1', 200);
    expect(list).toHaveLength(1);
  });

  it('caps the list at the given max, dropping the oldest', () => {
    let list: HistoryEntry[] = [];
    for (let i = 0; i < 5; i++) list = addEntry(list, `SELECT ${i}`, i, 3);
    expect(list).toHaveLength(3);
    expect(list.map((e) => e.sql)).toEqual(['SELECT 4', 'SELECT 3', 'SELECT 2']);
  });

  it('defaults the cap to HISTORY_MAX', () => {
    let list: HistoryEntry[] = [];
    for (let i = 0; i < HISTORY_MAX + 10; i++) list = addEntry(list, `SELECT ${i}`, i);
    expect(list).toHaveLength(HISTORY_MAX);
  });
});

describe('parseHistory', () => {
  it('returns an empty list for null or malformed input', () => {
    expect(parseHistory(null)).toEqual([]);
    expect(parseHistory('not json')).toEqual([]);
    expect(parseHistory('{"not":"an array"}')).toEqual([]);
  });

  it('round-trips a valid payload and drops malformed entries', () => {
    const raw = JSON.stringify([
      { sql: 'SELECT 1', at: 100 },
      { sql: 'no timestamp' },
      { at: 200 },
      { sql: 'SELECT 2', at: 300 },
    ]);
    expect(parseHistory(raw)).toEqual([
      { sql: 'SELECT 1', at: 100 },
      { sql: 'SELECT 2', at: 300 },
    ]);
  });
});
