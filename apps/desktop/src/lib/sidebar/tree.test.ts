import { describe, expect, it } from 'vitest';

import type { TableInfo } from '$lib/api';
import { groupTables } from './tree';

const t = (schema: string | null, name: string): TableInfo => ({ schema, name });

describe('groupTables', () => {
  it('puts tables under the database or schema they belong to, in order', () => {
    const groups = groupTables([t('shop', 'orders'), t('crm', 'leads'), t('shop', 'items')]);

    expect(groups.map((g) => g.name)).toEqual(['crm', 'shop']);
    expect(groups[1].tables.map((x) => x.name)).toEqual(['items', 'orders']);
  });

  // One group is today's flat list. Drawing a parent with one child would add
  // a click to every connection that only ever had one database.
  it('says a single group is not worth a level', () => {
    expect(groupTables([t('shop', 'a'), t('shop', 'b')])).toHaveLength(1);
    expect(groupTables([t(null, 'a'), t(null, 'b')])).toHaveLength(1);
  });

  it('keeps unqualified tables together, ahead of named groups', () => {
    const groups = groupTables([t('shop', 'a'), t(null, 'b')]);
    expect(groups.map((g) => g.name)).toEqual([null, 'shop']);
  });

  it('returns nothing for nothing', () => {
    expect(groupTables([])).toEqual([]);
  });
});
