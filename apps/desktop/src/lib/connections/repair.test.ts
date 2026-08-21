import { describe, expect, it } from 'vitest';
import {
  foreignRefFor,
  suggestCopyId,
  validateCopy,
  secretLabelKey,
  type CopyForm,
} from './repair';
import type { ForeignRef } from '$lib/api';

const refs: ForeignRef[] = [
  { id: 'beta', key_ref: 'dbboard.alpha.url', owner: 'alpha' },
  { id: 'gamma', key_ref: 'dbboard.alpha.ssh_password', owner: 'alpha' },
];

describe('foreignRefFor', () => {
  it('finds the entry that carries a foreign slot', () => {
    expect(foreignRefFor(refs, 'beta')).toEqual(refs[0]);
  });

  it('is undefined for a healthy connection', () => {
    // Absence is the common case: the list asks this once per row.
    expect(foreignRefFor(refs, 'alpha')).toBeUndefined();
    expect(foreignRefFor([], 'beta')).toBeUndefined();
  });
});

describe('suggestCopyId', () => {
  it('offers the source id with a copy suffix', () => {
    expect(suggestCopyId('prod', ['prod'])).toBe('prod-copy');
  });

  it('keeps counting while the suggestion is taken', () => {
    // The suggestion is a starting point, not a claim: an id that is already
    // there would be refused by the backend, and pre-filling a doomed value
    // reads as the app proposing something it will then reject.
    expect(suggestCopyId('prod', ['prod', 'prod-copy'])).toBe('prod-copy-2');
    expect(suggestCopyId('prod', ['prod', 'prod-copy', 'prod-copy-2'])).toBe('prod-copy-3');
  });

  it('ignores unrelated ids', () => {
    expect(suggestCopyId('prod', ['staging', 'prod'])).toBe('prod-copy');
  });
});

describe('validateCopy', () => {
  const form = (over: Partial<CopyForm> = {}): CopyForm => ({
    id: 'prod-copy',
    name: 'Prod copy',
    ...over,
  });

  it('accepts a filled form', () => {
    expect(validateCopy(form(), ['prod'])).toEqual([]);
  });

  it('flags a blank id and a blank name', () => {
    expect(validateCopy(form({ id: '  ', name: '' }), ['prod'])).toEqual(['id', 'name']);
  });

  it('flags an id already in the store', () => {
    // Caught here so the dialog can say so beside the field, rather than
    // surfacing the backend's DuplicateId as an error banner.
    expect(validateCopy(form({ id: 'prod' }), ['prod'])).toEqual(['id']);
  });

  it('compares the trimmed id, as the backend does', () => {
    expect(validateCopy(form({ id: ' prod ' }), ['prod'])).toEqual(['id']);
  });
});

describe('secretLabelKey', () => {
  it('names the secret by the field the slot was minted for', () => {
    // The repair dialog asks for one specific value, and which one is
    // readable from the slot name — asking for "the secret" would leave the
    // operator guessing between a URL, a token and an SSH password.
    expect(secretLabelKey('dbboard.alpha.url')).toBe('conn-repair-secret-url');
    expect(secretLabelKey('dbboard.alpha.token')).toBe('conn-repair-secret-token');
    expect(secretLabelKey('dbboard.alpha.service_account')).toBe(
      'conn-repair-secret-service_account',
    );
    expect(secretLabelKey('dbboard.alpha.secret_key')).toBe('conn-repair-secret-secret_key');
    expect(secretLabelKey('dbboard.alpha.ssh_password')).toBe('conn-repair-secret-ssh_password');
    expect(secretLabelKey('dbboard.alpha.ssh_passphrase')).toBe(
      'conn-repair-secret-ssh_passphrase',
    );
  });

  it('falls back to a generic label for a slot it cannot read', () => {
    expect(secretLabelKey('legacy-url')).toBe('conn-repair-secret');
    expect(secretLabelKey('dbboard.alpha.something_new')).toBe('conn-repair-secret');
  });

  it('reads the field from the right, because an id may contain dots', () => {
    // Same rule as the backend's `split_ref`: `dbboard.my.db.url` is the
    // connection `my.db`, not `my`.
    expect(secretLabelKey('dbboard.my.db.url')).toBe('conn-repair-secret-url');
  });
});
