import { describe, expect, it } from 'vitest';

import { byRecency, defaultQueryName, type SavedQueryView } from './saved';

const q = (name: string, saved_at: number): SavedQueryView => ({
  name,
  sql: 'SELECT 1',
  saved_at,
});

describe('defaultQueryName', () => {
  it('proposes the first line of the statement', () => {
    // The name the operator is most likely to accept is the one they can
    // recognise, and the first line is what they just typed.
    expect(defaultQueryName('SELECT * FROM orders', [])).toBe(
      'SELECT * FROM orders',
    );
  });

  it('takes only the first line of a multi-line statement', () => {
    expect(
      defaultQueryName('SELECT *\n  FROM orders\n  WHERE id = 1', []),
    ).toBe('SELECT *');
  });

  it('collapses runs of whitespace so the name is one readable line', () => {
    expect(defaultQueryName('SELECT   *    FROM   orders', [])).toBe(
      'SELECT * FROM orders',
    );
  });

  it('shortens a long statement rather than proposing a paragraph', () => {
    const sql = `SELECT ${'column_name, '.repeat(20)}1 FROM orders`;
    const name = defaultQueryName(sql, []);
    expect(name.length).toBeLessThanOrEqual(60);
    expect(name.endsWith('…')).toBe(true);
  });

  it('steps around a name that is already taken', () => {
    // Saving twice from the same statement is ordinary — a tweak, then
    // another. Proposing the taken name would send the operator straight into
    // the overwrite confirmation every time.
    const existing = [q('SELECT 1', 1)];
    expect(defaultQueryName('SELECT 1', existing)).toBe('SELECT 1 (2)');
  });

  it('keeps stepping until it finds a free name', () => {
    const existing = [q('SELECT 1', 1), q('SELECT 1 (2)', 2), q('SELECT 1 (3)', 3)];
    expect(defaultQueryName('SELECT 1', existing)).toBe('SELECT 1 (4)');
  });

  it('falls back to a fixed name for a blank statement', () => {
    // The Save button is disabled for blank SQL, so this is the belt to that
    // brace: never propose the empty string, which the store refuses.
    expect(defaultQueryName('   \n  ', [])).toBe('Untitled query');
  });
});

describe('byRecency', () => {
  it('puts the most recently saved first', () => {
    // The file keeps insertion order so its diffs read well; the list a person
    // looks at wants the opposite.
    const list = [q('old', 1), q('new', 3), q('middle', 2)];
    expect(byRecency(list).map((e) => e.name)).toEqual(['new', 'middle', 'old']);
  });

  it('does not mutate the list it was given', () => {
    const list = [q('old', 1), q('new', 3)];
    byRecency(list);
    expect(list.map((e) => e.name)).toEqual(['old', 'new']);
  });

  it('breaks a tie by name so the order never flickers', () => {
    // Two queries saved in the same millisecond is unlikely and not
    // impossible; an unstable comparator would reorder them on every render.
    const list = [q('b', 5), q('a', 5)];
    expect(byRecency(list).map((e) => e.name)).toEqual(['a', 'b']);
  });
});
