import { describe, expect, it } from 'vitest';

import type {
  ColumnInfo,
  ConnectionView,
  ForeignKeyRef,
  SchemaDiff,
  TableDiff,
  TableInfo,
} from '$lib/api';
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
  foreign_keys_compared: false,
};

/** A TableDiff with nothing in it, so a case names only what it is about. */
const noDiff = (name: string): TableDiff => ({
  table: table(name),
  columns_only_in_left: [],
  columns_only_in_right: [],
  columns_changed: [],
  primary_key: null,
  foreign_keys_only_in_left: [],
  foreign_keys_only_in_right: [],
  foreign_keys_changed: [],
});

const fk = (columns: string[], parent: string): ForeignKeyRef => ({
  columns,
  referenced_table: table(parent),
  referenced_columns: ['id'],
  constraint_name: null,
});

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
        { ...noDiff('orders'), primary_key: [['id'], ['id', 'tenant_id']] },
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
      ...noDiff('orders'),
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
        ...noDiff('orders'),
        columns_changed: [
          { name: 'total', left: col('total'), right: col('total'), fields: ['DeclaredType'] },
        ],
      }),
    ).toBe(1);
  });

  it('counts a foreign-key finding like any other', () => {
    // A key that points somewhere else, or is missing on one side, is one more
    // thing the reader has to look at — the number is what tells them how much
    // there is.
    expect(
      tableDifferenceCount({
        ...noDiff('orders'),
        foreign_keys_only_in_left: [fk(['customer_id'], 'customers')],
        foreign_keys_changed: [
          {
            columns: ['tenant_id'],
            left: fk(['tenant_id'], 'tenants'),
            right: fk(['tenant_id'], 'accounts'),
            fields: ['ReferencedTable'],
          },
        ],
      }),
    ).toBe(2);
  });
});

describe('totalDifferingTables', () => {
  it('counts all three kinds of finding', () => {
    const diff: SchemaDiff = {
      foreign_keys_compared: true,
      tables_only_in_left: [table('refunds')],
      tables_only_in_right: [table('experiments')],
      tables_changed: [
        { ...noDiff('orders'), primary_key: [['id'], ['id', 'tenant_id']] },
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
