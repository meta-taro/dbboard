import { describe, it, expect } from 'vitest';
import {
  quoteIdent,
  qualifiedName,
  selectTopN,
  countRows,
  dialectForKind,
  structuredQuery,
  browseQuery,
  countQuery,
  usesStructuredQuery,
} from './build';

describe('dialectForKind', () => {
  it('maps the MySQL adapter kind to the back-tick dialect', () => {
    expect(dialectForKind('mysql')).toBe('mysql');
  });

  // Every other adapter we ship (Postgres family, SQLite/libSQL, D1) accepts
  // the ANSI double quote, so they share one dialect.
  it.each(['postgres', 'neon', 'supabase', 'aurora-dsql', 'aurora-dsql-iam', 'turso', 'd1'])(
    'maps %s to the ANSI dialect',
    (kind) => {
      expect(dialectForKind(kind)).toBe('ansi');
    },
  );

  // An unknown or not-yet-loaded connection must not silently produce
  // back-ticks; ANSI is the safe default because it is what most engines take.
  it('falls back to ANSI for an unknown or missing kind', () => {
    expect(dialectForKind('something-new')).toBe('ansi');
    expect(dialectForKind(undefined)).toBe('ansi');
  });
});

describe('quoteIdent', () => {
  it('double-quotes a plain identifier', () => {
    expect(quoteIdent('users')).toBe('"users"');
  });

  // Doubling embedded quotes is what keeps a hostile or odd table name from
  // breaking out of the identifier — the one injection surface here.
  it('escapes embedded double quotes by doubling them', () => {
    expect(quoteIdent('we"ird')).toBe('"we""ird"');
  });

  it('back-quotes for MySQL, which rejects "…" without ANSI_QUOTES', () => {
    expect(quoteIdent('users', 'mysql')).toBe('`users`');
  });

  it('escapes embedded back-ticks by doubling them', () => {
    expect(quoteIdent('we`ird', 'mysql')).toBe('`we``ird`');
  });

  // A double quote is an ordinary character inside a back-quoted MySQL
  // identifier, so it must pass through untouched rather than be doubled.
  it('leaves double quotes alone in the MySQL dialect', () => {
    expect(quoteIdent('we"ird', 'mysql')).toBe('`we"ird`');
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

  // The MySQL adapter schema-qualifies every table with its database name.
  it('back-quotes both parts for MySQL', () => {
    expect(qualifiedName({ schema: 'shop', name: 'orders' }, 'mysql')).toBe(
      '`shop`.`orders`',
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

  it('uses back-ticks for MySQL', () => {
    expect(selectTopN({ schema: 'shop', name: 'orders' }, 100, 'mysql')).toBe(
      'SELECT * FROM `shop`.`orders` LIMIT 100;',
    );
  });
});

describe('countRows', () => {
  it('counts every row of a schema-qualified table', () => {
    expect(countRows({ schema: 'public', name: 'orders' })).toBe(
      'SELECT COUNT(*) FROM "public"."orders";',
    );
  });

  it('counts every row of a schemaless table', () => {
    expect(countRows({ schema: null, name: 'orders' })).toBe(
      'SELECT COUNT(*) FROM "orders";',
    );
  });

  it('uses back-ticks for MySQL', () => {
    expect(countRows({ schema: 'shop', name: 'orders' }, 'mysql')).toBe(
      'SELECT COUNT(*) FROM `shop`.`orders`;',
    );
  });
});

describe('structuredQuery', () => {
  it('builds the bounded StructuredQuery the Firestore adapter runs', () => {
    expect(structuredQuery({ schema: null, name: 'orders' }, 100)).toBe(
      '{\n  "from": [{ "collectionId": "orders" }],\n  "limit": 100\n}',
    );
  });

  // Firestore has no schema layer: `list_tables` reports top-level collection
  // ids, so anything in `schema` is not part of the collection's identity and
  // must not leak into the query.
  it('names the collection alone, ignoring any schema', () => {
    expect(structuredQuery({ schema: 'public', name: 'orders' }, 100)).toBe(
      '{\n  "from": [{ "collectionId": "orders" }],\n  "limit": 100\n}',
    );
  });

  // A collection id is user data, and the query is JSON: an unescaped quote
  // would not be a wrong result but an unparseable request.
  it('escapes a collection id containing JSON-significant characters', () => {
    expect(structuredQuery({ schema: null, name: 'we"ird\\one' }, 1)).toBe(
      '{\n  "from": [{ "collectionId": "we\\"ird\\\\one" }],\n  "limit": 1\n}',
    );
  });

  it('floors the limit to a positive integer, as selectTopN does', () => {
    expect(structuredQuery({ schema: null, name: 't' }, 3.9)).toContain('"limit": 3');
    expect(structuredQuery({ schema: null, name: 't' }, 0)).toContain('"limit": 1');
  });
});

describe('usesStructuredQuery', () => {
  it('is true for Firestore, whose query text is JSON', () => {
    expect(usesStructuredQuery('firestore')).toBe(true);
  });

  it.each(['postgres', 'mysql', 'turso', 'd1', 'neon', undefined])(
    'is false for the SQL engine %s',
    (kind) => {
      expect(usesStructuredQuery(kind)).toBe(false);
    },
  );
});

describe('browseQuery', () => {
  it.each(['postgres', 'turso', 'd1', undefined])(
    'generates ANSI SQL for %s',
    (kind) => {
      expect(browseQuery({ schema: 'public', name: 'orders' }, 100, kind)).toBe(
        'SELECT * FROM "public"."orders" LIMIT 100;',
      );
    },
  );

  it('generates MySQL-quoted SQL for MySQL', () => {
    expect(browseQuery({ schema: 'shop', name: 'orders' }, 100, 'mysql')).toBe(
      'SELECT * FROM `shop`.`orders` LIMIT 100;',
    );
  });

  // Firestore takes a StructuredQuery in JSON (ADR-0093); SQL is not a dialect
  // it is bad at, it is a language it does not have.
  it('generates a StructuredQuery for Firestore', () => {
    expect(browseQuery({ schema: null, name: 'orders' }, 100, 'firestore')).toBe(
      structuredQuery({ schema: null, name: 'orders' }, 100),
    );
  });
});

describe('countQuery', () => {
  it('counts rows with SQL on the SQL engines', () => {
    expect(countQuery({ schema: 'public', name: 'orders' }, 'postgres')).toBe(
      'SELECT COUNT(*) FROM "public"."orders";',
    );
    expect(countQuery({ schema: 'shop', name: 'orders' }, 'mysql')).toBe(
      'SELECT COUNT(*) FROM `shop`.`orders`;',
    );
  });

  // Counting in Firestore is `:runAggregationQuery`, a different endpoint the
  // read-only adapter does not implement. Offering the action anyway would put
  // a menu entry there that can only ever fail.
  it('has no counterpart on Firestore', () => {
    expect(countQuery({ schema: null, name: 'orders' }, 'firestore')).toBeNull();
  });
});
