import { describe, expect, it } from 'vitest';

import type { Column } from '$lib/api';
import {
  blameRows,
  diffRows,
  keepsNoHistory,
  keyFor,
  parentOf,
  shortHash,
  versionRows,
  type Commit,
  type CommitDiff,
  type RowHistory,
} from './view';

const cols = (...names: string[]): Column[] =>
  names.map((name) => ({ name, declared_type: null }));

const commit = (hash: string, message = 'msg'): Commit => ({
  hash,
  author: 'Sample Author',
  email: 'author@example.com',
  date: '2026-01-02 03:04:05',
  message,
});

describe('shortHash', () => {
  it('keeps the first seven characters, as git and dolt print them', () => {
    expect(shortHash('k2v8h0q5mnb3r1t7')).toBe('k2v8h0q');
  });

  it('leaves a short ref such as HEAD alone', () => {
    expect(shortHash('HEAD')).toBe('HEAD');
  });
});

describe('parentOf', () => {
  const log = [commit('c3'), commit('c2'), commit('c1')];

  it('is the next older commit in a newest-first log', () => {
    expect(parentOf(log, 'c3')).toBe('c2');
    expect(parentOf(log, 'c2')).toBe('c1');
  });

  it('is null for the oldest commit, which has nothing before it', () => {
    expect(parentOf(log, 'c1')).toBeNull();
  });

  it('is null for a hash the log does not hold', () => {
    expect(parentOf(log, 'zz')).toBeNull();
  });
});

describe('diffRows', () => {
  const diff: CommitDiff = {
    columns: cols('id', 'qty', 'note'),
    changes: [
      { kind: 'modified', before: [1, 3, 'a'], after: [1, 2, 'a'], changed_columns: [1] },
      { kind: 'added', before: null, after: [3, 1, 'c'], changed_columns: [0, 1, 2] },
      { kind: 'removed', before: [2, 5, 'b'], after: null, changed_columns: [0, 1, 2] },
    ],
  };

  it('shows the new value of a modified row and keeps the old one beside it', () => {
    const [row] = diffRows(diff);
    expect(row.kind).toBe('modified');
    expect(row.cells.map((c) => c.text)).toEqual(['1', '2', 'a']);
    expect(row.cells.map((c) => c.changed)).toEqual([false, true, false]);
    expect(row.cells[1].before).toBe('3');
    // Only a changed cell carries a before: an unchanged one would just
    // repeat itself.
    expect(row.cells[0].before).toBeNull();
  });

  it('shows what was added, and what was removed', () => {
    const [, added, removed] = diffRows(diff);
    expect(added.cells.map((c) => c.text)).toEqual(['3', '1', 'c']);
    expect(removed.cells.map((c) => c.text)).toEqual(['2', '5', 'b']);
    // A whole new or vanished row is the change; marking every cell of it
    // would only add noise, so the row's kind carries it.
    expect(added.cells.every((c) => !c.changed)).toBe(true);
    expect(removed.cells.every((c) => c.before === null)).toBe(true);
  });

  it('keeps the values a removed row had, so its history can be opened', () => {
    const [, added, removed] = diffRows(diff);
    expect(added.values).toEqual([3, 1, 'c']);
    expect(removed.values).toEqual([2, 5, 'b']);
  });

  it('writes NULL rather than an empty cell', () => {
    const [row] = diffRows({
      columns: cols('id', 'note'),
      changes: [{ kind: 'modified', before: [1, 'x'], after: [1, null], changed_columns: [1] }],
    });
    expect(row.cells[1].text).toBe('NULL');
    expect(row.cells[1].before).toBe('x');
  });
});

describe('versionRows', () => {
  const history: RowHistory = {
    columns: cols('id', 'qty', 'note'),
    versions: [
      { commit: commit('c3', 'third'), values: [1, 2, 'b'] },
      { commit: commit('c2', 'second'), values: [1, 2, 'a'] },
      { commit: commit('c1', 'seed'), values: [1, 3, 'a'] },
    ],
  };

  it('marks each version against the one before it', () => {
    const rows = versionRows(history);
    expect(rows.map((r) => r.cells.map((c) => c.changed))).toEqual([
      [false, false, true],
      [false, true, false],
      // The oldest version has nothing to differ from.
      [false, false, false],
    ]);
    expect(rows[0].cells[2].before).toBe('a');
    expect(rows[0].commit.message).toBe('third');
  });
});

describe('keyFor', () => {
  it('pairs each primary-key column with its value in the row', () => {
    expect(keyFor(cols('id', 'region', 'qty'), ['region', 'id'], [7, 'eu', 3])).toEqual([
      { column: 'region', value: 'eu' },
      { column: 'id', value: 7 },
    ]);
  });

  it('is null without a primary key, so no history is offered', () => {
    expect(keyFor(cols('id'), [], [7])).toBeNull();
  });

  it('is null when a key column is missing from the row', () => {
    expect(keyFor(cols('qty'), ['id'], [3])).toBeNull();
  });
});

describe('blameRows', () => {
  it('lays the key out under the primary-key column names', () => {
    const rows = blameRows(['id'], [{ key: [7], commit: commit('c2', 'second') }]);
    expect(rows[0].cells).toEqual(['7']);
    expect(rows[0].key).toEqual([{ column: 'id', value: 7 }]);
    expect(rows[0].commit.message).toBe('second');
  });
});

describe('keepsNoHistory', () => {
  // The words come from the Redis-compatible adapter's refusal for an
  // Umbrachyx table that keeps only its latest value (ADR-0173).
  it('recognises the refusal for a table that keeps only its latest value', () => {
    expect(
      keepsNoHistory(
        'capability unavailable: pos keeps only its latest value: Umbrachyx records no history for it',
      ),
    ).toBe(true);
  });

  it('leaves every other error an error', () => {
    expect(keepsNoHistory('capability unavailable: describe_table not supported')).toBe(false);
    expect(keepsNoHistory('query failed: keeps only its latest value')).toBe(false);
  });
});
