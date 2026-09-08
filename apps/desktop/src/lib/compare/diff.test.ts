import { describe, expect, it } from 'vitest';

import type { ColumnInfo, ConnectionView, SchemaDiff, TableInfo } from '$lib/api';
import {
  comparableWith,
  differingTables,
  sectionId,
  tableDifferenceCount,
  totalDifferingTables,
} from './diff';

const conn = (id: string, kind: string): ConnectionView => ({ id, name: id, kind });

const table = (name: string, schema: string | null = null): TableInfo => ({
  schema,
  name,
});

const col = (name: string): ColumnInfo => ({
  name,
  declared_type: 'TEXT',
  nullable: true,
  primary_key: false,
  ordinal: 1,
  default_value: null,
});

const emptyDiff: SchemaDiff = {
  tables_only_in_left: [],
  tables_only_in_right: [],
  tables_changed: [],
};

describe('comparableWith', () => {
  it('offers only connections of the same engine', () => {
    // The backend refuses a cross-engine pair before it dials anything
    // (ADR-0148). Offering the choice anyway would make the app propose
    // something it is about to decline.
    const all = [conn('a', 'postgres'), conn('b', 'postgres'), conn('c', 'turso')];
    expect(comparableWith(all, 'a').map((c) => c.id)).toEqual(['b']);
  });

  it('never offers the connection itself', () => {
    const all = [conn('a', 'postgres'), conn('b', 'postgres')];
    expect(comparableWith(all, 'a').map((c) => c.id)).toEqual(['b']);
    expect(comparableWith(all, 'b').map((c) => c.id)).toEqual(['a']);
  });

  it('is empty when nothing shares the engine', () => {
    // The UI uses this to say why, rather than showing an empty menu.
    const all = [conn('a', 'postgres'), conn('c', 'turso')];
    expect(comparableWith(all, 'a')).toEqual([]);
  });

  it('is empty when the left id is unknown', () => {
    expect(comparableWith([conn('a', 'postgres')], 'ghost')).toEqual([]);
  });
});

describe('differingTables', () => {
  it('marks a table that is missing on the right', () => {
    const diff: SchemaDiff = {
      ...emptyDiff,
      tables_only_in_left: [table('refunds')],
    };
    expect(differingTables(diff)).toEqual(new Map([['refunds', 'left-only']]));
  });

  it('marks a table that is missing on the left', () => {
    const diff: SchemaDiff = {
      ...emptyDiff,
      tables_only_in_right: [table('experiments')],
    };
    expect(differingTables(diff)).toEqual(
      new Map([['experiments', 'right-only']]),
    );
  });

  it('marks a table that exists on both sides but differs', () => {
    const diff: SchemaDiff = {
      ...emptyDiff,
      tables_changed: [
        {
          table: table('orders'),
          columns_only_in_left: [],
          columns_only_in_right: [],
          columns_changed: [],
          primary_key: [['id'], ['id', 'tenant_id']],
        },
      ],
    };
    expect(differingTables(diff)).toEqual(new Map([['orders', 'changed']]));
  });

  it('keys by the schema-qualified name, like the sidebar does', () => {
    // `public.orders` and `staging.orders` are different tables; a bare-name
    // key would put the marker on both.
    const diff: SchemaDiff = {
      ...emptyDiff,
      tables_only_in_left: [table('orders', 'public')],
    };
    expect([...differingTables(diff).keys()]).toEqual(['public.orders']);
  });

  it('is empty for a diff with nothing in it', () => {
    expect(differingTables(emptyDiff).size).toBe(0);
  });
});

describe('tableDifferenceCount', () => {
  it('counts every column finding and the key as one each', () => {
    const count = tableDifferenceCount({
      table: table('orders'),
      columns_only_in_left: [col('shipped_at')],
      columns_only_in_right: [col('note')],
      columns_changed: [
        {
          name: 'total',
          left: col('total'),
          right: col('total'),
          fields: ['DeclaredType', 'Nullable'],
        },
      ],
      primary_key: [['id'], ['id', 'tenant_id']],
    });
    // One column only on the left, one only on the right, one changed column
    // (however many of its attributes differ), one primary key: four findings
    // a person has to look at, not five field-level ones.
    expect(count).toBe(4);
  });

  it('does not count a primary key the two sides agree on', () => {
    expect(
      tableDifferenceCount({
        table: table('orders'),
        columns_only_in_left: [],
        columns_only_in_right: [],
        columns_changed: [
          { name: 'total', left: col('total'), right: col('total'), fields: ['DeclaredType'] },
        ],
        primary_key: null,
      }),
    ).toBe(1);
  });
});

describe('totalDifferingTables', () => {
  it('counts all three kinds of finding', () => {
    const diff: SchemaDiff = {
      tables_only_in_left: [table('refunds')],
      tables_only_in_right: [table('experiments')],
      tables_changed: [
        {
          table: table('orders'),
          columns_only_in_left: [],
          columns_only_in_right: [],
          columns_changed: [],
          primary_key: [['id'], ['id', 'tenant_id']],
        },
      ],
    };
    expect(totalDifferingTables(diff)).toBe(3);
  });

  it('is zero for two matching schemas', () => {
    expect(totalDifferingTables(emptyDiff)).toBe(0);
  });
});

describe('sectionId', () => {
  it('is stable for the same table', () => {
    expect(sectionId(table('orders', 'public'))).toBe(
      sectionId(table('orders', 'public')),
    );
  });

  it('separates two tables of the same name in different schemas', () => {
    expect(sectionId(table('orders', 'public'))).not.toBe(
      sectionId(table('orders', 'staging')),
    );
  });

  it('survives a name that is not valid in an id', () => {
    // Table names can hold spaces, dots and quotes. The id ends up in the DOM
    // and in `getElementById`, so it must not carry them through.
    const id = sectionId(table('my table."x"'));
    expect(id).toMatch(/^cmp-[A-Za-z0-9_-]+$/);
  });
});
