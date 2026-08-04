import { describe, it, expect } from 'vitest';
import {
  buildRowUpdates,
  cellKey,
  displayWidth,
  needsWideEditor,
  INLINE_EDITOR_COLUMNS,
  type StagedValue,
} from './edit';
import type { Cell, Column } from '$lib/api';

const col = (name: string): Column => ({ name, declared_type: null });

// A 3-column result (id PK, name, note) with distinct per-row values so a
// test can prove the WHERE key is read from the RIGHT row, not row 0.
const COLUMNS: Column[] = [col('id'), col('name'), col('note')];
const ROWS: Cell[][] = [
  [1, 'a', 'first'],
  [2, 'b', null],
  [3, 'c', 'third'],
];

function staged(entries: [number, number, StagedValue][]): Map<string, StagedValue> {
  return new Map(entries.map(([r, c, v]) => [cellKey(r, c), v]));
}

describe('cellKey', () => {
  it('is distinct per (row, col) and does not collide across the boundary', () => {
    // "1" + "23" vs "12" + "3" must not produce the same key.
    expect(cellKey(1, 23)).not.toBe(cellKey(12, 3));
    expect(cellKey(2, 1)).toBe(cellKey(2, 1));
  });
});

describe('displayWidth (ADR-0082)', () => {
  it('counts ASCII as one column each', () => {
    expect(displayWidth('hello')).toBe(5);
    expect(displayWidth('')).toBe(0);
  });

  it('counts CJK and kana as two columns each', () => {
    // The whole reason the inline editor felt cramped: 10 Japanese characters
    // take the space of 20 Latin ones.
    expect(displayWidth('融和者')).toBe(6);
    expect(displayWidth('あいう')).toBe(6);
    expect(displayWidth('가나')).toBe(4);
  });

  it('mixes half- and full-width in one string', () => {
    expect(displayWidth('/top と mtext')).toBe(4 + 1 + 2 + 1 + 5);
  });

  it('counts an astral code point once, not twice', () => {
    // An emoji is two UTF-16 units but one character, two columns wide. A
    // `.length`-based count would have said four.
    expect(displayWidth('🙂')).toBe(2);
  });
});

describe('needsWideEditor (ADR-0082)', () => {
  it('keeps a short value in the inline editor', () => {
    expect(needsWideEditor('')).toBe(false);
    expect(needsWideEditor('/top')).toBe(false);
    expect(needsWideEditor('078-578-2619')).toBe(false);
  });

  it('sends a value wider than the inline editor to the dialog', () => {
    expect(needsWideEditor('a'.repeat(INLINE_EDITOR_COLUMNS))).toBe(false);
    expect(needsWideEditor('a'.repeat(INLINE_EDITOR_COLUMNS + 1))).toBe(true);
  });

  it('reaches the threshold twice as fast in Japanese', () => {
    // Half as many characters, same decision — the point of measuring display
    // width instead of `.length`.
    const half = '本'.repeat(INLINE_EDITOR_COLUMNS / 2 + 1);
    expect(half.length).toBeLessThan(INLINE_EDITOR_COLUMNS);
    expect(needsWideEditor(half)).toBe(true);
  });

  it('always sends a multi-line value to the dialog, however short', () => {
    // Not cosmetic: a single-line <input> strips CR/LF from its value, so
    // editing "a\nb" inline would commit "ab" and silently destroy the row.
    expect(needsWideEditor('a\nb')).toBe(true);
    expect(needsWideEditor('\n')).toBe(true);
  });
});

describe('buildRowUpdates', () => {
  it('builds one update per touched row, keyed by the primary key', () => {
    const updates = buildRowUpdates(staged([[1, 1, 'B']]), ROWS, COLUMNS, ['id']);
    expect(updates).toEqual([
      { key: [{ column: 'id', value: 2 }], edits: [{ column: 'name', value: 'B' }] },
    ]);
  });

  it('reads each row-key value from its OWN row, not row zero', () => {
    // Edit row index 2 (id=3): the key must be id=3, proving the lookup is
    // per-row and uses the original (pre-sort) index.
    const updates = buildRowUpdates(staged([[2, 2, 'THIRD']]), ROWS, COLUMNS, ['id']);
    expect(updates[0].key).toEqual([{ column: 'id', value: 3 }]);
  });

  it('emits rows in ascending original-index order and edits in column order', () => {
    const updates = buildRowUpdates(
      staged([
        [2, 2, 'z'],
        [0, 2, 'y'],
        [0, 1, 'x'],
      ]),
      ROWS,
      COLUMNS,
      ['id'],
    );
    expect(updates.map((u) => u.key[0].value)).toEqual([1, 3]);
    // Row 0's two edits come out in column order (name before note).
    expect(updates[0].edits).toEqual([
      { column: 'name', value: 'x' },
      { column: 'note', value: 'y' },
    ]);
  });

  it('passes an explicit NULL through as null, not an empty string', () => {
    const updates = buildRowUpdates(staged([[0, 2, null]]), ROWS, COLUMNS, ['id']);
    expect(updates[0].edits).toEqual([{ column: 'note', value: null }]);
  });

  it('supports a composite primary key', () => {
    const updates = buildRowUpdates(staged([[1, 2, 'x']]), ROWS, COLUMNS, ['id', 'name']);
    expect(updates[0].key).toEqual([
      { column: 'id', value: 2 },
      { column: 'name', value: 'b' },
    ]);
  });

  it('refuses to build an update for a table with no primary key', () => {
    expect(() => buildRowUpdates(staged([[0, 1, 'x']]), ROWS, COLUMNS, [])).toThrow();
  });

  it('refuses when a primary-key column is missing from the result columns', () => {
    // The browse projected columns without "id" (e.g. SELECT name, note): the
    // row can't be safely keyed, so building must fail loudly.
    const projected = [col('name'), col('note')];
    const projectedRows: Cell[][] = [['a', 'first']];
    expect(() => buildRowUpdates(staged([[0, 0, 'x']]), projectedRows, projected, ['id'])).toThrow();
  });
});
