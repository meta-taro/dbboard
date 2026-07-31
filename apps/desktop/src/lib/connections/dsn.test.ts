import { describe, it, expect } from 'vitest';
import {
  DSN_FIELDS,
  defaultPort,
  schemeFor,
  emptyDsnParts,
  composeDsn,
  validateDsn,
  usesDsnFields,
} from './dsn';
import type { ConnectionKind } from './draft';

const parts = (over: Partial<ReturnType<typeof emptyDsnParts>> = {}) => ({
  ...emptyDsnParts(),
  db_host: 'db.internal',
  db_user: 'app',
  db_password: 'secret',
  db_name: 'shop',
  ...over,
});

describe('usesDsnFields', () => {
  it.each(['postgres', 'mysql', 'neon', 'supabase', 'aurora_dsql'] as ConnectionKind[])(
    'is true for the URL-bearing kind %s',
    (kind) => {
      expect(usesDsnFields(kind)).toBe(true);
    },
  );

  it.each(['turso', 'd1'] as ConnectionKind[])(
    'is false for %s, which has no host:port to describe',
    (kind) => {
      expect(usesDsnFields(kind)).toBe(false);
    },
  );
});

describe('defaultPort', () => {
  it('is 3306 for MySQL', () => {
    expect(defaultPort('mysql')).toBe(3306);
  });

  it.each(['postgres', 'neon', 'supabase', 'aurora_dsql'] as ConnectionKind[])(
    'is 5432 for the Postgres-family kind %s',
    (kind) => {
      expect(defaultPort(kind)).toBe(5432);
    },
  );
});

describe('schemeFor', () => {
  it('is mysql for MySQL', () => {
    expect(schemeFor('mysql')).toBe('mysql');
  });

  it.each(['postgres', 'neon', 'supabase', 'aurora_dsql'] as ConnectionKind[])(
    'is postgres for %s',
    (kind) => {
      expect(schemeFor(kind)).toBe('postgres');
    },
  );
});

describe('DSN_FIELDS', () => {
  it('lists the inputs in the order HeidiSQL-trained hands expect', () => {
    expect(DSN_FIELDS).toEqual([
      'db_host',
      'db_port',
      'db_user',
      'db_password',
      'db_name',
    ]);
  });
});

describe('composeDsn', () => {
  it('builds a MySQL URL with the default port when the port is blank', () => {
    expect(composeDsn('mysql', parts())).toBe('mysql://app:secret@db.internal:3306/shop');
  });

  it('builds a Postgres URL for the Postgres-family kinds', () => {
    expect(composeDsn('postgres', parts())).toBe(
      'postgres://app:secret@db.internal:5432/shop',
    );
  });

  it('honours an explicit port', () => {
    expect(composeDsn('mysql', parts({ db_port: '3307' }))).toBe(
      'mysql://app:secret@db.internal:3307/shop',
    );
  });

  it('omits the password section entirely when no password is set', () => {
    expect(composeDsn('mysql', parts({ db_password: '' }))).toBe(
      'mysql://app@db.internal:3306/shop',
    );
  });

  // A MySQL password with `@` or `/` in it would otherwise split the authority
  // and silently connect to the wrong host — the single most likely way a
  // hand-written DSN goes wrong.
  it('percent-encodes a password containing URL-significant characters', () => {
    expect(composeDsn('mysql', parts({ db_password: 'p@ss/w#rd?' }))).toBe(
      'mysql://app:p%40ss%2Fw%23rd%3F@db.internal:3306/shop',
    );
  });

  it('percent-encodes the user and the database name', () => {
    expect(composeDsn('mysql', parts({ db_user: 'a b', db_name: 'my db' }))).toBe(
      'mysql://a%20b:secret@db.internal:3306/my%20db',
    );
  });

  it('brackets a bare IPv6 host so the colons are not read as a port', () => {
    expect(composeDsn('mysql', parts({ db_host: '::1' }))).toBe(
      'mysql://app:secret@[::1]:3306/shop',
    );
  });

  it('leaves an already-bracketed IPv6 host alone', () => {
    expect(composeDsn('mysql', parts({ db_host: '[::1]' }))).toBe(
      'mysql://app:secret@[::1]:3306/shop',
    );
  });

  it('trims surrounding whitespace pasted in from a terminal', () => {
    expect(
      composeDsn('mysql', parts({ db_host: '  db.internal  ', db_name: ' shop ' })),
    ).toBe('mysql://app:secret@db.internal:3306/shop');
  });
});

describe('validateDsn', () => {
  it('accepts a fully filled set', () => {
    expect(validateDsn(parts())).toEqual([]);
  });

  it('reports every blank required field', () => {
    expect(validateDsn(emptyDsnParts())).toEqual(['db_host', 'db_user', 'db_name']);
  });

  // Blank is legal (a MySQL account may have no password) and means exactly
  // that; the composer omits the section rather than sending an empty one.
  it('does not require a password', () => {
    expect(validateDsn(parts({ db_password: '' }))).toEqual([]);
  });

  it('allows a blank port, which means "use the kind default"', () => {
    expect(validateDsn(parts({ db_port: '' }))).toEqual([]);
  });

  it.each(['0', '65536', 'abc', '-1'])('rejects the out-of-range port %s', (port) => {
    expect(validateDsn(parts({ db_port: port }))).toEqual(['db_port']);
  });

  it('accepts the boundary ports', () => {
    expect(validateDsn(parts({ db_port: '1' }))).toEqual([]);
    expect(validateDsn(parts({ db_port: '65535' }))).toEqual([]);
  });
});
