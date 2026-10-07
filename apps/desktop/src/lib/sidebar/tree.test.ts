import { describe, expect, it } from 'vitest';

import type { TableInfo } from '$lib/api';
import { groupTables, repeatsItsDatabase } from './tree';

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

describe('repeatsItsDatabase', () => {
  // Inside a MySQL database node the schema *is* the database, so `shop.orders`
  // under `shop` says the same thing twice.
  it('is true when a table sits under a database of the same name', () => {
    expect(repeatsItsDatabase(t('shop', 'orders'), 'shop')).toBe(true);
  });

  // A Postgres schema is a level of its own and still worth naming.
  it('is false for a schema inside a database, or with no database level', () => {
    expect(repeatsItsDatabase(t('public', 'orders'), 'shop')).toBe(false);
    expect(repeatsItsDatabase(t('shop', 'orders'), null)).toBe(false);
  });
});
