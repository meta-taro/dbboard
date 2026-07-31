import { describe, it, expect } from 'vitest';
import { PATH_FIELDS, isPathField, pickerFilters, pickerTitle } from './file-picker';

describe('isPathField', () => {
  it('accepts the Turso database file', () => {
    expect(isPathField('path')).toBe(true);
  });

  it('accepts both SSH file fields', () => {
    expect(isPathField('ssh_key_path')).toBe(true);
    expect(isPathField('ssh_known_hosts')).toBe(true);
  });

  it.each(['url', 'token', 'account_id', 'ssh_host', 'ssh_password', 'db_host'])(
    'rejects %s, which is not a filesystem path',
    (field) => {
      expect(isPathField(field)).toBe(false);
    },
  );

  it('lists exactly the fields it accepts', () => {
    expect([...PATH_FIELDS]).toEqual(['path', 'ssh_key_path', 'ssh_known_hosts']);
    for (const f of PATH_FIELDS) expect(isPathField(f)).toBe(true);
  });
});

describe('pickerFilters', () => {
  it('offers SQLite extensions for the Turso database file', () => {
    const filters = pickerFilters('path');
    expect(filters.length).toBeGreaterThan(0);
    expect(filters[0].extensions).toEqual(expect.arrayContaining(['db', 'sqlite', 'sqlite3']));
  });

  it('always ends with an all-files entry, so a differently named file is still reachable', () => {
    const last = pickerFilters('path').at(-1);
    expect(last?.extensions).toEqual(['*']);
  });

  it('filters nothing for an OpenSSH private key, which has no extension', () => {
    expect(pickerFilters('ssh_key_path')).toEqual([]);
  });

  it('filters nothing for known_hosts, which has no extension', () => {
    expect(pickerFilters('ssh_known_hosts')).toEqual([]);
  });
});

describe('pickerTitle', () => {
  it('names what is being picked, not just "open"', () => {
    expect(pickerTitle('path')).toBe('conn-field-path');
    expect(pickerTitle('ssh_key_path')).toBe('conn-ssh-key-path');
    expect(pickerTitle('ssh_known_hosts')).toBe('conn-ssh-known-hosts');
  });
});
