import { describe, it, expect } from 'vitest';

import { filterConnections } from './filter';

const list = [
  { id: 'c_01', name: 'shop A production', kind: 'mysql' },
  { id: 'c_02', name: 'shop A staging', kind: 'mysql' },
  { id: 'c_03', name: 'analytics', kind: 'postgres' },
];

describe('filterConnections', () => {
  it('returns the list untouched when nothing has been typed', () => {
    expect(filterConnections(list, '')).toBe(list);
    expect(filterConnections(list, '   ')).toBe(list);
  });

  it('matches on the name, case-insensitively', () => {
    expect(filterConnections(list, 'SHOP').map((c) => c.id)).toEqual(['c_01', 'c_02']);
  });

  it('matches on the id too, so a pasted id finds its row', () => {
    expect(filterConnections(list, 'c_03').map((c) => c.id)).toEqual(['c_03']);
  });

  it('requires every word to match, so a second word narrows further', () => {
    expect(filterConnections(list, 'shop staging').map((c) => c.id)).toEqual(['c_02']);
  });

  it('does not match the kind: typing "my" to find a name would return every MySQL row', () => {
    expect(filterConnections(list, 'mysql')).toEqual([]);
  });

  it('keeps the stored order of whatever survives', () => {
    expect(filterConnections(list, 'a').map((c) => c.id)).toEqual(['c_01', 'c_02', 'c_03']);
  });
});
