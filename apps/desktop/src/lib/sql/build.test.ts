import { describe, it, expect } from 'vitest';
import { quoteIdent, qualifiedName, selectTopN } from './build';

describe('quoteIdent', () => {
  it('double-quotes a plain identifier', () => {
    expect(quoteIdent('users')).toBe('"users"');
  });

  // Doubling embedded quotes is what keeps a hostile or odd table name from
  // breaking out of the identifier — the one injection surface here.
  it('escapes embedded double quotes by doubling them', () => {
    expect(quoteIdent('we"ird')).toBe('"we""ird"');
  });
});

describe('qualifiedName', () => {
  it('quotes a bare (schemaless) table', () => {
    expect(qualifiedName({ schema: null, name: 'orders' })).toBe('"orders"');
  });

  it('quotes schema and name separately', () => {
    expect(qualifiedName({ schema: 'public', name: 'orders' })).toBe(
      '"public"."orders"',
    );
  });
});

describe('selectTopN', () => {
  it('builds a LIMIT-bounded SELECT * for a schemaless table', () => {
    expect(selectTopN({ schema: null, name: 'orders' }, 100)).toBe(
      'SELECT * FROM "orders" LIMIT 100;',
    );
  });

  it('qualifies the schema when present', () => {
    expect(selectTopN({ schema: 'public', name: 'orders' }, 50)).toBe(
      'SELECT * FROM "public"."orders" LIMIT 50;',
    );
  });

  it('floors and clamps n to a positive integer', () => {
    expect(selectTopN({ schema: null, name: 't' }, 0)).toBe(
      'SELECT * FROM "t" LIMIT 1;',
    );
    expect(selectTopN({ schema: null, name: 't' }, 3.9)).toBe(
      'SELECT * FROM "t" LIMIT 3;',
    );
  });
});
