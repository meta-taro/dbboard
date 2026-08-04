import { describe, it, expect } from 'vitest';
import {
  DSN_FIELDS,
  SSL_MODES,
  defaultPort,
  schemeFor,
  emptyDsnParts,
  composeDsn,
  validateDsn,
  usesDsnFields,
  sslModeFromUrl,
  withSslMode,
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

describe('SSL_MODES', () => {
  it('offers require and disable, and nothing that silently falls back', () => {
    expect([...SSL_MODES]).toEqual(['require', 'disable']);
  });

  it('defaults a new form to require', () => {
    expect(emptyDsnParts().db_ssl).toBe('require');
  });
});

describe('composeDsn TLS mode', () => {
  // The default composes exactly the URL it composed before this option
  // existed: no query string, and the adapter's own hardening turns sqlx's
  // plaintext-falling-back default into Required.
  it('adds nothing when TLS is required', () => {
    expect(composeDsn('mysql', parts({ db_ssl: 'require' }))).toBe(
      'mysql://app:secret@db.internal:3306/shop',
    );
    expect(composeDsn('postgres', parts({ db_ssl: 'require' }))).toBe(
      'postgres://app:secret@db.internal:5432/shop',
    );
  });

  it('spells the MySQL parameter the way sqlx parses it', () => {
    expect(composeDsn('mysql', parts({ db_ssl: 'disable' }))).toBe(
      'mysql://app:secret@db.internal:3306/shop?ssl-mode=disabled',
    );
  });

  // Postgres uses a different parameter name *and* a different value spelling
  // from MySQL. Getting either wrong is a config error at connect time, not a
  // silently insecure connection, but it is still a dead end for the user.
  it.each(['postgres', 'neon', 'supabase', 'aurora_dsql'] as ConnectionKind[])(
    'spells the Postgres parameter for %s the way sqlx parses it',
    (kind) => {
      expect(composeDsn(kind, parts({ db_ssl: 'disable' }))).toBe(
        `postgres://app:secret@db.internal:5432/shop?sslmode=disable`,
      );
    },
  );

  it('keeps the database name encoded when a parameter follows it', () => {
    expect(composeDsn('mysql', parts({ db_name: 'my db', db_ssl: 'disable' }))).toBe(
      'mysql://app:secret@db.internal:3306/my%20db?ssl-mode=disabled',
    );
  });
});

describe('sslModeFromUrl', () => {
  it('reads a hand-written URL as requiring TLS when it says nothing', () => {
    expect(sslModeFromUrl('mysql://app@db.internal:3306/shop')).toBe('require');
  });

  it('recognises the MySQL spelling', () => {
    expect(sslModeFromUrl('mysql://app@db:3306/shop?ssl-mode=disabled')).toBe('disable');
  });

  it('recognises the Postgres spelling', () => {
    expect(sslModeFromUrl('postgres://app@db:5432/shop?sslmode=disable')).toBe('disable');
  });

  it('is case-insensitive, as sqlx is', () => {
    expect(sslModeFromUrl('mysql://app@db:3306/shop?ssl-mode=DISABLED')).toBe('disable');
  });

  it('accepts sslmode on a mysql URL, which sqlx also accepts', () => {
    expect(sslModeFromUrl('mysql://app@db:3306/shop?sslmode=disabled')).toBe('disable');
  });

  it('finds the parameter among others', () => {
    expect(sslModeFromUrl('mysql://app@db:3306/shop?charset=utf8mb4&ssl-mode=disabled')).toBe(
      'disable',
    );
  });

  // REQUIRED / VERIFY_CA / VERIFY_IDENTITY all mean "encrypted"; the select
  // only distinguishes off from on, and must not misreport a stricter mode.
  it.each(['required', 'verify_ca', 'verify_identity', 'preferred'])(
    'reports %s as require, since only disabled is the off switch',
    (mode) => {
      expect(sslModeFromUrl(`mysql://app@db:3306/shop?ssl-mode=${mode}`)).toBe('require');
    },
  );

  it('does not throw on a URL that is still being typed', () => {
    expect(sslModeFromUrl('mysql://')).toBe('require');
    expect(sslModeFromUrl('')).toBe('require');
    expect(sslModeFromUrl('not a url at all')).toBe('require');
  });
});

describe('withSslMode', () => {
  it('leaves a blank URL alone, so the select cannot invent one', () => {
    expect(withSslMode('mysql', '', 'disable')).toBe('');
    expect(withSslMode('mysql', '   ', 'disable')).toBe('   ');
  });

  it('appends the MySQL parameter to a URL that has no query', () => {
    expect(withSslMode('mysql', 'mysql://app@db:3306/shop', 'disable')).toBe(
      'mysql://app@db:3306/shop?ssl-mode=disabled',
    );
  });

  it('appends the Postgres parameter to a URL that has no query', () => {
    expect(withSslMode('postgres', 'postgres://app@db:5432/shop', 'disable')).toBe(
      'postgres://app@db:5432/shop?sslmode=disable',
    );
  });

  it('appends after an existing parameter rather than replacing the query', () => {
    expect(withSslMode('mysql', 'mysql://app@db:3306/shop?charset=utf8mb4', 'disable')).toBe(
      'mysql://app@db:3306/shop?charset=utf8mb4&ssl-mode=disabled',
    );
  });

  it('removes the parameter when TLS goes back to required', () => {
    expect(withSslMode('mysql', 'mysql://app@db:3306/shop?ssl-mode=disabled', 'require')).toBe(
      'mysql://app@db:3306/shop',
    );
  });

  it('keeps the other parameters when it removes its own', () => {
    expect(
      withSslMode('mysql', 'mysql://app@db:3306/shop?ssl-mode=disabled&charset=utf8mb4', 'require'),
    ).toBe('mysql://app@db:3306/shop?charset=utf8mb4');
  });

  it('replaces rather than duplicates an existing entry', () => {
    expect(withSslMode('mysql', 'mysql://app@db:3306/shop?ssl-mode=required', 'disable')).toBe(
      'mysql://app@db:3306/shop?ssl-mode=disabled',
    );
  });

  it('is idempotent', () => {
    const once = withSslMode('mysql', 'mysql://app@db:3306/shop', 'disable');
    expect(withSslMode('mysql', once, 'disable')).toBe(once);
  });

  it('round-trips through sslModeFromUrl', () => {
    const url = 'mysql://app@db:3306/shop';
    expect(sslModeFromUrl(withSslMode('mysql', url, 'disable'))).toBe('disable');
    expect(sslModeFromUrl(withSslMode('mysql', url, 'require'))).toBe('require');
  });

  // A half-typed URL must survive keystroke-by-keystroke: rewriting it into
  // something the user did not type would fight the cursor.
  it('leaves an unparseable URL exactly as typed', () => {
    expect(withSslMode('mysql', 'mysql://app@', 'disable')).toBe('mysql://app@');
    expect(withSslMode('mysql', 'still typing', 'disable')).toBe('still typing');
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
