import { describe, it, expect } from 'vitest';
import {
  emptyForm,
  fieldsForKind,
  secretFields,
  requiredFields,
  validate,
  validateDsnFields,
  isEditableInApp,
  canProbeHostKey,
  buildKindInput,
  buildKindEditInput,
  formForEdit,
  keepStoredPassword,
  CONNECTION_KINDS,
  supportsSshTunnel,
  validateSsh,
  buildSshInput,
  buildSshEditInput,
  parseSshPort,
  type ConnectionForm,
} from './draft';

function form(overrides: Partial<ConnectionForm>): ConnectionForm {
  return { ...emptyForm(), ...overrides };
}

// A complete, valid key-auth tunnel form on top of a postgres connection.
function tunnelForm(overrides: Partial<ConnectionForm> = {}): ConnectionForm {
  return form({
    kind: 'postgres',
    name: 'PG',
    url: 'postgres://h/db',
    ssh_enabled: true,
    ssh_host: 'bastion.example',
    ssh_user: 'deploy',
    ssh_auth_method: 'key',
    ssh_key_path: '/home/deploy/.ssh/id_ed25519',
    ssh_host_key_policy: 'fingerprint',
    ssh_fingerprint: 'SHA256:abc',
    ...overrides,
  });
}

describe('fieldsForKind', () => {
  it('turso needs only a path', () => {
    expect(fieldsForKind('turso')).toEqual(['path']);
  });
  it('d1 needs account/database/base_url/token', () => {
    expect(fieldsForKind('d1')).toEqual([
      'account_id',
      'database_id',
      'base_url',
      'token',
    ]);
  });
  it('every postgres-family kind is a single url', () => {
    for (const k of ['postgres', 'mysql', 'neon', 'supabase', 'aurora_dsql'] as const) {
      expect(fieldsForKind(k)).toEqual(['url']);
    }
  });
});

describe('secretFields', () => {
  it('turso has no secret', () => {
    expect(secretFields('turso')).toEqual([]);
  });
  it('d1 token is secret', () => {
    expect(secretFields('d1')).toEqual(['token']);
  });
  it('the url is the secret for postgres-family kinds', () => {
    expect(secretFields('neon')).toEqual(['url']);
  });
});

describe('requiredFields', () => {
  it('add requires id + name + non-secret kind fields', () => {
    expect(requiredFields('turso', 'add')).toEqual(['id', 'name', 'path']);
  });
  it('add treats base_url as optional but token as required', () => {
    expect(requiredFields('d1', 'add')).toEqual([
      'id',
      'name',
      'account_id',
      'database_id',
      'token',
    ]);
  });
  it('edit drops id and the secret field (blank secret keeps the stored one)', () => {
    // url is the secret for postgres → not required on edit.
    expect(requiredFields('postgres', 'edit')).toEqual(['name']);
    // d1: token drops out on edit, non-secret account/database stay.
    expect(requiredFields('d1', 'edit')).toEqual([
      'name',
      'account_id',
      'database_id',
    ]);
  });
});

describe('validate', () => {
  it('flags every blank required field', () => {
    expect(validate(emptyForm(), 'add')).toEqual(['id', 'name', 'path']);
  });
  it('passes a complete turso add form', () => {
    expect(
      validate(form({ id: 'c', name: 'C', kind: 'turso', path: ':memory:' }), 'add'),
    ).toEqual([]);
  });
  it('treats a whitespace-only value as blank', () => {
    expect(
      validate(form({ id: '  ', name: 'C', kind: 'turso', path: ':memory:' }), 'add'),
    ).toEqual(['id']);
  });
  it('lets an edit through without resupplying the secret url', () => {
    expect(
      validate(form({ name: 'C', kind: 'postgres', url: '' }), 'edit'),
    ).toEqual([]);
  });
});

describe('buildKindInput', () => {
  it('shapes a turso add payload', () => {
    expect(buildKindInput(form({ kind: 'turso', path: ':memory:' }))).toEqual({
      kind: 'turso',
      path: ':memory:',
    });
  });
  it('collapses a blank d1 base_url to null', () => {
    expect(
      buildKindInput(
        form({
          kind: 'd1',
          account_id: 'a',
          database_id: 'b',
          base_url: '   ',
          token: 't',
        }),
      ),
    ).toEqual({
      kind: 'd1',
      account_id: 'a',
      database_id: 'b',
      base_url: null,
      token: 't',
    });
  });
  it('keeps a non-blank d1 base_url', () => {
    const payload = buildKindInput(
      form({ kind: 'd1', account_id: 'a', database_id: 'b', base_url: 'https://x', token: 't' }),
    );
    expect(payload.base_url).toBe('https://x');
  });
  it('tags a neon payload with its own discriminator, not postgres', () => {
    expect(
      buildKindInput(form({ kind: 'neon', use_url: true, url: 'postgres://h/db' })),
    ).toEqual({
      kind: 'neon',
      url: 'postgres://h/db',
    });
  });
  it('tags a mysql payload with the mysql discriminator', () => {
    expect(
      buildKindInput(form({ kind: 'mysql', use_url: true, url: 'mysql://h/db' })),
    ).toEqual({
      kind: 'mysql',
      url: 'mysql://h/db',
    });
  });
});

describe('buildKindEditInput', () => {
  it('sends a blank secret verbatim (backend reads blank as keep)', () => {
    expect(
      buildKindEditInput(form({ kind: 'postgres', use_url: true, url: '' })),
    ).toEqual({
      kind: 'postgres',
      url: '',
    });
  });
});

// Structured host/port/user/password/database entry (the default for an add).
// The raw DSN stays available as an escape hatch for options a form can't
// express (`?sslmode=`, socket paths, multi-host).
describe('DSN field mode', () => {
  const mysqlParts = {
    kind: 'mysql' as const,
    id: 'm',
    name: 'M',
    db_host: '127.0.0.1',
    db_port: '3307',
    db_user: 'app',
    db_password: 'pw',
    db_name: 'shop',
  };

  it('an add form starts in fields mode', () => {
    expect(emptyForm().use_url).toBe(false);
  });

  // Only when the backend could not hand back the parts. With them, edit opens
  // in the same structured form as add (ADR-0080).
  it('an edit falls back to url mode when the backend sends no parts', () => {
    expect(formForEdit('p', 'P', { kind: 'neon' }).use_url).toBe(true);
  });

  it('requiredFields drops the url in fields mode', () => {
    expect(requiredFields('mysql', 'add', false)).toEqual(['id', 'name']);
  });

  it('requiredFields keeps the url in url mode', () => {
    expect(requiredFields('mysql', 'add', true)).toEqual(['id', 'name', 'url']);
  });

  it('validate ignores a blank url when the parts are being used', () => {
    expect(validate(form({ ...mysqlParts, use_url: false, url: '' }), 'add')).toEqual([]);
  });

  it('validateDsnFields reports the blank parts in fields mode', () => {
    expect(validateDsnFields(form({ kind: 'mysql', use_url: false }))).toEqual([
      'db_host',
      'db_user',
      'db_name',
    ]);
  });

  it('validateDsnFields stays silent in url mode', () => {
    expect(validateDsnFields(form({ kind: 'mysql', use_url: true }))).toEqual([]);
  });

  it.each(['turso', 'd1'] as const)(
    'validateDsnFields stays silent for %s, which has no DSN',
    (kind) => {
      expect(validateDsnFields(form({ kind, use_url: false }))).toEqual([]);
    },
  );

  it('buildKindInput composes the DSN from the parts', () => {
    expect(buildKindInput(form({ ...mysqlParts, use_url: false }))).toEqual({
      kind: 'mysql',
      url: 'mysql://app:pw@127.0.0.1:3307/shop',
    });
  });

  it('buildKindEditInput composes the DSN too — in fields mode an edit is a real replacement, never a keep', () => {
    expect(buildKindEditInput(form({ ...mysqlParts, use_url: false }))).toEqual({
      kind: 'mysql',
      url: 'mysql://app:pw@127.0.0.1:3307/shop',
    });
  });

  it('leaves turso and d1 payloads untouched by the mode flag', () => {
    expect(buildKindInput(form({ kind: 'turso', use_url: false, path: ':memory:' }))).toEqual(
      { kind: 'turso', path: ':memory:' },
    );
  });
});

describe('formForEdit', () => {
  it('seeds a turso path and leaves secrets blank', () => {
    const f = formForEdit('c', 'C', { kind: 'turso', path: ':memory:' });
    expect(f.id).toBe('c');
    expect(f.name).toBe('C');
    expect(f.kind).toBe('turso');
    expect(f.path).toBe(':memory:');
    expect(f.token).toBe('');
    expect(f.url).toBe('');
  });
  it('seeds d1 non-secret fields and coerces a null base_url to blank', () => {
    const f = formForEdit('d', 'D', {
      kind: 'd1',
      account_id: 'a',
      database_id: 'b',
      base_url: null,
    });
    expect(f.account_id).toBe('a');
    expect(f.database_id).toBe('b');
    expect(f.base_url).toBe('');
    expect(f.token).toBe('');
  });
  it('leaves the url blank for a postgres-family edit (secret is kept)', () => {
    const f = formForEdit('p', 'P', { kind: 'neon' });
    expect(f.kind).toBe('neon');
    expect(f.url).toBe('');
    // A blank-url edit validates (keep the stored secret).
    expect(validate(f, 'edit')).toEqual([]);
  });
});

// The user's report: "「編集」で開くと URL モードは追加の時のフォームが変わるので
// 困りますね" — add asked for host/port/user/password/database, edit asked for a
// raw URL, and the same connection looked like two different products.
describe('formForEdit with DSN parts (ADR-0080)', () => {
  const dsn = {
    host: 'db.internal',
    port: 3307,
    user: 'app',
    database: 'shop',
    query: '',
  };

  it('opens in the same structured mode the add form uses', () => {
    const f = formForEdit('m', 'M', { kind: 'mysql', dsn });
    expect(f.use_url).toBe(false);
  });

  it('prefills every part the backend sent', () => {
    const f = formForEdit('m', 'M', { kind: 'mysql', dsn });
    expect(f.db_host).toBe('db.internal');
    expect(f.db_port).toBe('3307');
    expect(f.db_user).toBe('app');
    expect(f.db_name).toBe('shop');
  });

  // The one field that must stay empty: the backend never sends it, and blank
  // is what tells the save path to keep the stored one.
  it('leaves the password blank', () => {
    expect(formForEdit('m', 'M', { kind: 'mysql', dsn }).db_password).toBe('');
  });

  it('leaves the port blank when the stored URL omitted it', () => {
    const f = formForEdit('m', 'M', { kind: 'mysql', dsn: { ...dsn, port: null } });
    expect(f.db_port).toBe('');
  });

  it('restores the TLS choice from the stored query string', () => {
    const off = formForEdit('m', 'M', {
      kind: 'mysql',
      dsn: { ...dsn, query: 'ssl-mode=disabled' },
    });
    expect(off.db_ssl).toBe('disable');
    expect(formForEdit('m', 'M', { kind: 'mysql', dsn }).db_ssl).toBe('require');
  });

  it('validates with the password left alone', () => {
    expect(validateDsnFields(formForEdit('m', 'M', { kind: 'mysql', dsn }))).toEqual([]);
  });

  it('still applies the ssh prefill alongside the parts', () => {
    const f = formForEdit('m', 'M', {
      kind: 'mysql',
      dsn,
      ssh: {
        host: 'bastion.example',
        port: 2222,
        user: 'ops',
        auth: { method: 'key', key_path: '/k', encrypted: false },
        host_key: { policy: 'fingerprint', fingerprint: 'SHA256:x' },
      },
    });
    expect(f.ssh_enabled).toBe(true);
    expect(f.ssh_host).toBe('bastion.example');
    expect(f.db_host).toBe('db.internal');
  });
});

describe('keepStoredPassword', () => {
  const edited = (over: Partial<ConnectionForm> = {}) =>
    form({ kind: 'mysql', use_url: false, db_host: 'h', db_user: 'u', db_name: 'd', ...over });

  it('is true when an edit leaves the password box untouched', () => {
    expect(keepStoredPassword(edited(), 'edit')).toBe(true);
  });

  it('is false once the user types a new password', () => {
    expect(keepStoredPassword(edited({ db_password: 'new' }), 'edit')).toBe(false);
  });

  // On add there is no stored password to keep — a blank one means the account
  // genuinely has none.
  it('is false on add', () => {
    expect(keepStoredPassword(edited(), 'add')).toBe(false);
  });

  // URL mode already has its own keep signal: a blank URL keeps the whole
  // stored secret, password included.
  it('is false in url mode', () => {
    expect(keepStoredPassword(edited({ use_url: true }), 'edit')).toBe(false);
  });

  it('is false for a kind that stores no DSN', () => {
    expect(keepStoredPassword(edited({ kind: 'turso' }), 'edit')).toBe(false);
  });
});

// The list shows the backend's display slug (hyphenated), which is a different
// namespace from the form's `ConnectionKind` (underscored) — `aurora-dsql-iam`
// has no form representation at all, which is exactly why it can't be edited.
describe('isEditableInApp', () => {
  it('rejects Aurora DSQL (IAM), which lives only in connections.toml', () => {
    expect(isEditableInApp('aurora-dsql-iam')).toBe(false);
  });

  it.each([
    'turso',
    'd1',
    'postgres',
    'mysql',
    'neon',
    'supabase',
    'aurora-dsql',
  ])('accepts %s', (slug) => {
    expect(isEditableInApp(slug)).toBe(true);
  });

  // Static-credential Aurora DSQL is editable; only the IAM variant is not, and
  // one is a prefix of the other.
  it('does not confuse aurora-dsql with aurora-dsql-iam', () => {
    expect(isEditableInApp('aurora-dsql')).toBe(true);
    expect(isEditableInApp('aurora-dsql-iam')).toBe(false);
  });

  it('treats an unknown slug as editable, so a new backend kind is not silently locked out', () => {
    expect(isEditableInApp('cockroach')).toBe(true);
  });
});

describe('CONNECTION_KINDS', () => {
  it('lists all seven kinds with turso first', () => {
    expect(CONNECTION_KINDS).toEqual([
      'turso',
      'd1',
      'postgres',
      'mysql',
      'neon',
      'supabase',
      'aurora_dsql',
    ]);
  });
});

describe('supportsSshTunnel', () => {
  it('is true only for the TCP/URL engines', () => {
    for (const k of ['postgres', 'mysql', 'neon', 'supabase', 'aurora_dsql'] as const) {
      expect(supportsSshTunnel(k)).toBe(true);
    }
    expect(supportsSshTunnel('turso')).toBe(false);
    expect(supportsSshTunnel('d1')).toBe(false);
  });
});

describe('parseSshPort', () => {
  it('parses a valid port', () => {
    expect(parseSshPort('2222')).toBe(2222);
  });
  it('defaults a blank or out-of-range value to 22', () => {
    expect(parseSshPort('')).toBe(22);
    expect(parseSshPort('  ')).toBe(22);
    expect(parseSshPort('0')).toBe(22);
    expect(parseSshPort('70000')).toBe(22);
    expect(parseSshPort('nope')).toBe(22);
  });
});

describe('validateSsh', () => {
  it('is a no-op when the tunnel is off', () => {
    expect(validateSsh(tunnelForm({ ssh_enabled: false }), 'add')).toEqual([]);
  });
  it('is a no-op for a kind that cannot tunnel even if the toggle is on', () => {
    expect(validateSsh(form({ kind: 'turso', ssh_enabled: true }), 'add')).toEqual([]);
  });
  it('passes a complete key-auth tunnel', () => {
    expect(validateSsh(tunnelForm(), 'add')).toEqual([]);
  });
  it('flags a missing host, user, key path and host-key fingerprint', () => {
    const bad = validateSsh(
      tunnelForm({ ssh_host: '', ssh_user: '', ssh_key_path: '', ssh_fingerprint: '' }),
      'add',
    );
    expect(bad).toEqual(['ssh_host', 'ssh_user', 'ssh_key_path', 'ssh_fingerprint']);
  });
  it('rejects a non-numeric or out-of-range port but allows blank (defaults 22)', () => {
    expect(validateSsh(tunnelForm({ ssh_port: 'abc' }), 'add')).toEqual(['ssh_port']);
    expect(validateSsh(tunnelForm({ ssh_port: '99999' }), 'add')).toEqual(['ssh_port']);
    expect(validateSsh(tunnelForm({ ssh_port: '' }), 'add')).toEqual([]);
  });
  it('requires the password on add', () => {
    const pw = tunnelForm({ ssh_auth_method: 'password', ssh_password: '' });
    expect(validateSsh(pw, 'add')).toEqual(['ssh_password']);
  });
  it('on edit keeps a stored password when one exists (blank ok)', () => {
    const pw = tunnelForm({
      ssh_auth_method: 'password',
      ssh_password: '',
      ssh_had_password: true,
    });
    expect(validateSsh(pw, 'edit')).toEqual([]);
  });
  it('on edit requires a password when switching to password auth (nothing to keep)', () => {
    const pw = tunnelForm({
      ssh_auth_method: 'password',
      ssh_password: '',
      ssh_had_password: false,
    });
    expect(validateSsh(pw, 'edit')).toEqual(['ssh_password']);
  });
  it('on edit requires a passphrase when a key is freshly marked encrypted', () => {
    const k = tunnelForm({
      ssh_key_encrypted: true,
      ssh_passphrase: '',
      ssh_had_key_passphrase: false,
    });
    expect(validateSsh(k, 'edit')).toEqual(['ssh_passphrase']);
  });
  it('on edit keeps a stored passphrase for an already-encrypted key (blank ok)', () => {
    const k = tunnelForm({
      ssh_key_encrypted: true,
      ssh_passphrase: '',
      ssh_had_key_passphrase: true,
    });
    expect(validateSsh(k, 'edit')).toEqual([]);
  });
  it('requires known_hosts when that policy is chosen', () => {
    const kh = tunnelForm({ ssh_host_key_policy: 'known_hosts', ssh_known_hosts: '' });
    expect(validateSsh(kh, 'add')).toEqual(['ssh_known_hosts']);
  });
});

describe('buildSshInput', () => {
  it('returns null when the tunnel is off', () => {
    expect(buildSshInput(tunnelForm({ ssh_enabled: false }))).toBeNull();
  });
  it('returns null for a non-tunnelable kind', () => {
    expect(buildSshInput(form({ kind: 'turso', ssh_enabled: true }))).toBeNull();
  });
  it('shapes a key-auth payload with a null passphrase for an unencrypted key', () => {
    expect(buildSshInput(tunnelForm({ ssh_port: '2222' }))).toEqual({
      host: 'bastion.example',
      port: 2222,
      user: 'deploy',
      auth: { method: 'key', key_path: '/home/deploy/.ssh/id_ed25519', passphrase: null },
      host_key: { policy: 'fingerprint', fingerprint: 'SHA256:abc' },
    });
  });
  it('sends an inline passphrase when the key is encrypted', () => {
    const p = buildSshInput(tunnelForm({ ssh_passphrase: 'unlock' }));
    expect(p?.auth).toEqual({
      method: 'key',
      key_path: '/home/deploy/.ssh/id_ed25519',
      passphrase: 'unlock',
    });
  });
  it('shapes a password-auth payload', () => {
    const p = buildSshInput(
      tunnelForm({ ssh_auth_method: 'password', ssh_password: 's3cr3t' }),
    );
    expect(p?.auth).toEqual({ method: 'password', password: 's3cr3t' });
  });
});

describe('buildSshEditInput', () => {
  it('disables the tunnel when the toggle is off', () => {
    expect(buildSshEditInput(tunnelForm({ ssh_enabled: false }))).toEqual({ action: 'disable' });
  });
  it('disables for a non-tunnelable kind', () => {
    expect(buildSshEditInput(form({ kind: 'turso', ssh_enabled: true }))).toEqual({
      action: 'disable',
    });
  });
  it('sets a key-auth tunnel, carrying the encrypted flag and a null (keep) passphrase', () => {
    expect(buildSshEditInput(tunnelForm({ ssh_key_encrypted: true, ssh_passphrase: '' }))).toEqual({
      action: 'set',
      host: 'bastion.example',
      port: 22,
      user: 'deploy',
      auth: {
        method: 'key',
        key_path: '/home/deploy/.ssh/id_ed25519',
        encrypted: true,
        passphrase: null,
      },
      host_key: { policy: 'fingerprint', fingerprint: 'SHA256:abc' },
    });
  });
  it('sends a blank password as null (keep the stored one)', () => {
    const p = buildSshEditInput(
      tunnelForm({ ssh_auth_method: 'password', ssh_password: '' }),
    );
    expect(p.auth).toEqual({ method: 'password', password: null });
  });
});

describe('formForEdit with an SSH prefill', () => {
  it('seeds the tunnel fields and never a secret', () => {
    const f = formForEdit('p', 'P', {
      kind: 'postgres',
      ssh: {
        host: 'bastion.example',
        port: 2222,
        user: 'deploy',
        auth: { method: 'key', key_path: '/k', encrypted: true },
        host_key: { policy: 'known_hosts', known_hosts: '/kh' },
      },
    });
    expect(f.ssh_enabled).toBe(true);
    expect(f.ssh_host).toBe('bastion.example');
    expect(f.ssh_port).toBe('2222');
    expect(f.ssh_user).toBe('deploy');
    expect(f.ssh_auth_method).toBe('key');
    expect(f.ssh_key_path).toBe('/k');
    expect(f.ssh_key_encrypted).toBe(true);
    expect(f.ssh_host_key_policy).toBe('known_hosts');
    expect(f.ssh_known_hosts).toBe('/kh');
    // Secrets are never prefilled.
    expect(f.ssh_passphrase).toBe('');
    expect(f.ssh_password).toBe('');
    // An encrypted key has a stored passphrase to keep.
    expect(f.ssh_had_key_passphrase).toBe(true);
    expect(f.ssh_had_password).toBe(false);
  });
  it('records no stored passphrase for an unencrypted key prefill', () => {
    const f = formForEdit('p', 'P', {
      kind: 'postgres',
      ssh: {
        host: 'h',
        port: 22,
        user: 'u',
        auth: { method: 'key', key_path: '/k', encrypted: false },
        host_key: { policy: 'fingerprint', fingerprint: 'SHA256:abc' },
      },
    });
    expect(f.ssh_key_encrypted).toBe(false);
    expect(f.ssh_had_key_passphrase).toBe(false);
  });
  it('records a stored password for a password-auth prefill', () => {
    const f = formForEdit('p', 'P', {
      kind: 'postgres',
      ssh: {
        host: 'h',
        port: 22,
        user: 'u',
        auth: { method: 'password' },
        host_key: { policy: 'fingerprint', fingerprint: 'SHA256:abc' },
      },
    });
    expect(f.ssh_auth_method).toBe('password');
    expect(f.ssh_had_password).toBe(true);
    expect(f.ssh_had_key_passphrase).toBe(false);
  });
  it('leaves the tunnel off when no ssh block is returned', () => {
    const f = formForEdit('p', 'P', { kind: 'postgres' });
    expect(f.ssh_enabled).toBe(false);
  });
});

describe('canProbeHostKey', () => {
  const base = (): ConnectionForm => ({
    ...emptyForm(),
    kind: 'mysql',
    ssh_enabled: true,
    ssh_host: 'bastion.example.com',
    ssh_port: '22',
    ssh_host_key_policy: 'fingerprint',
  });

  it('allows the probe once the host is known', () => {
    expect(canProbeHostKey(base())).toBe(true);
  });

  it('needs a host — there is nothing to ask otherwise', () => {
    expect(canProbeHostKey({ ...base(), ssh_host: '   ' })).toBe(false);
  });

  it('accepts a blank port, which means the SSH default', () => {
    expect(canProbeHostKey({ ...base(), ssh_port: '' })).toBe(true);
  });

  it('rejects a port outside the valid range rather than probing the default', () => {
    expect(canProbeHostKey({ ...base(), ssh_port: '70000' })).toBe(false);
    expect(canProbeHostKey({ ...base(), ssh_port: '0' })).toBe(false);
    expect(canProbeHostKey({ ...base(), ssh_port: 'ssh' })).toBe(false);
  });

  it('is off when the tunnel itself is off', () => {
    expect(canProbeHostKey({ ...base(), ssh_enabled: false })).toBe(false);
  });

  it('is off for the known_hosts policy, which pins nothing to fetch', () => {
    expect(canProbeHostKey({ ...base(), ssh_host_key_policy: 'known_hosts' })).toBe(false);
  });

  it('is off for a kind that cannot tunnel at all', () => {
    expect(canProbeHostKey({ ...base(), kind: 'd1' })).toBe(false);
  });
});
