// Every shipped locale translates every key (ADR-0172).
//
// Nine of the eleven locales used to carry 30 of the keys and fall back to
// English for the rest, which on screen is a window half in one language
// and half in another: the language menu moved, so the switch looked like it
// had worked, and most labels did not (#181). The fallback stays, as a net
// for a key added later; this test is what keeps a release from shipping a
// locale that leans on it.
import { describe, expect, it } from 'vitest';

import { SUPPORTED_CODES } from './locales';
import { catalogs, en, type MessageKey } from './messages';

const KEYS = Object.keys(en) as MessageKey[];

/** The `{ $name }` placeholders in a template, sorted. */
function placeholders(template: string): string[] {
  return [...template.matchAll(/\{\s*\$([a-zA-Z0-9_-]+)\s*\}/g)].map((m) => m[1]).sort();
}

describe.each(SUPPORTED_CODES)('%s', (code) => {
  const catalog = catalogs[code] ?? {};

  it('translates every key', () => {
    const missing = KEYS.filter((k) => !catalog[k]?.trim());
    expect(missing).toEqual([]);
  });

  it('has no key that English lacks', () => {
    const extra = Object.keys(catalog).filter((k) => !(k in en));
    expect(extra).toEqual([]);
  });

  it('keeps every placeholder, and adds none', () => {
    const wrong = KEYS.filter(
      (k) => catalog[k] !== undefined && placeholders(catalog[k]!).join() !== placeholders(en[k]).join(),
    );
    expect(wrong).toEqual([]);
  });
});

// A Windows path in a double-quoted literal loses its backslash: `\k` is just
// `k` in JavaScript, so ".ssh\known_hosts" displayed as ".sshknown_hosts" in
// English and Japanese until the nine translations wrote it as `\\`.
describe('the known_hosts hint', () => {
  it.each(SUPPORTED_CODES)('%s shows the Windows path with its backslash', (code) => {
    expect(catalogs[code]?.['conn-ssh-known-hosts-hint']).toContain('.ssh\\known_hosts');
  });
});
