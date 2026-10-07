import { describe, expect, it } from 'vitest';

import { drawsDatabaseLevel } from './databases';

describe('drawsDatabaseLevel', () => {
  // The promise of docs/every-database.md: a connection saved with a database
  // looks exactly as it did.
  it('keeps a connection saved with a database flat', () => {
    expect(drawsDatabaseLevel({ configured: 'shop', databases: [] })).toBe(false);
  });

  it('draws databases for a connection saved without one that can see several', () => {
    expect(drawsDatabaseLevel({ configured: null, databases: ['crm', 'shop'] })).toBe(true);
  });

  // One database is a parent with a single child: a click with nothing behind it.
  it('stays flat when there is only one database to show', () => {
    expect(drawsDatabaseLevel({ configured: null, databases: ['postgres'] })).toBe(false);
  });

  it('stays flat for an engine that lists none', () => {
    expect(drawsDatabaseLevel({ configured: null, databases: [] })).toBe(false);
    expect(drawsDatabaseLevel(null)).toBe(false);
  });
});
