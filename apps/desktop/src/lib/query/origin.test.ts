import { describe, expect, it } from 'vitest';

import type { TableInfo } from '$lib/api';
import { belongsTo, editContextFor, type ResultOrigin } from './origin';

const products: TableInfo = { schema: 'public', name: 'products' };
const staging: ResultOrigin = { connectionId: 'staging', database: null };

describe('the rows on screen', () => {
  it('belong to the connection they were read from', () => {
    expect(belongsTo(staging, 'staging')).toBe(true);
  });

  it('stop belonging once another connection is selected', () => {
    expect(belongsTo(staging, 'prod')).toBe(false);
    expect(belongsTo(staging, null)).toBe(false);
  });

  it('belong to nothing before anything has run', () => {
    expect(belongsTo(null, 'staging')).toBe(false);
  });
});

describe('the edit context', () => {
  it('sends an edit back to the connection and database the rows came from', () => {
    const origin: ResultOrigin = { connectionId: 'shop-pg', database: 'crm' };
    expect(editContextFor(origin, 'shop-pg', products, ['id'])).toEqual({
      connectionId: 'shop-pg',
      database: 'crm',
      table: products,
      pk: ['id'],
    });
  });

  // The bug this exists for: browse `products` on staging, select prod, edit a
  // cell, save. The context used to take the *selected* connection, so the
  // UPDATE keyed on staging's primary keys landed on prod's rows.
  it('is withheld once a different connection is selected', () => {
    expect(editContextFor(staging, 'prod', products, ['id'])).toBeNull();
  });

  it('is withheld for a table with no primary key', () => {
    expect(editContextFor(staging, 'staging', products, [])).toBeNull();
  });

  it('is withheld for a hand-written query', () => {
    expect(editContextFor(staging, 'staging', null, ['id'])).toBeNull();
  });
});
