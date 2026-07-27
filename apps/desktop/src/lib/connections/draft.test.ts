import { describe, it, expect } from 'vitest';
import {
  emptyForm,
  fieldsForKind,
  secretFields,
  requiredFields,
  validate,
  buildKindInput,
  buildKindEditInput,
  formForEdit,
  CONNECTION_KINDS,
  type ConnectionForm,
} from './draft';

function form(overrides: Partial<ConnectionForm>): ConnectionForm {
  return { ...emptyForm(), ...overrides };
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
    for (const k of ['postgres', 'neon', 'supabase', 'aurora_dsql'] as const) {
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
    expect(buildKindInput(form({ kind: 'neon', url: 'postgres://h/db' }))).toEqual({
      kind: 'neon',
      url: 'postgres://h/db',
    });
  });
});

describe('buildKindEditInput', () => {
  it('sends a blank secret verbatim (backend reads blank as keep)', () => {
    expect(buildKindEditInput(form({ kind: 'postgres', url: '' }))).toEqual({
      kind: 'postgres',
      url: '',
    });
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

describe('CONNECTION_KINDS', () => {
  it('lists all six kinds with turso first', () => {
    expect(CONNECTION_KINDS).toEqual([
      'turso',
      'd1',
      'postgres',
      'neon',
      'supabase',
      'aurora_dsql',
    ]);
  });
});
