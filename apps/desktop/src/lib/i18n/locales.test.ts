import { describe, expect, it } from 'vitest';

import { preferredLocale } from './locales';

describe('preferredLocale', () => {
  it('prefers the setting file over everything else', () => {
    // `ui-settings.toml` is the only store an MCP client — or the same user on
    // a previous run of a *different* build — can write, so it outranks the
    // per-webview localStorage copy.
    expect(preferredLocale('ko', 'ja', 'de-DE')).toBe('ko');
  });

  it('falls back to the localStorage choice when the file has none', () => {
    // Upgrades land here: the choice was made before the file existed.
    expect(preferredLocale(null, 'ja', 'de-DE')).toBe('ja');
  });

  it('falls back to the OS language when nothing was ever chosen', () => {
    expect(preferredLocale(null, null, 'de-DE')).toBe('de');
  });

  it('ends at English when the OS language is one we do not ship', () => {
    expect(preferredLocale(null, null, 'nl-NL')).toBe('en');
    expect(preferredLocale(null, null, null)).toBe('en');
  });

  it('ignores a stored code this build cannot display', () => {
    // A locale can be dropped between releases; a stale localStorage entry
    // must not leave the UI showing keys.
    expect(preferredLocale(null, 'nl', 'ja-JP')).toBe('ja');
  });

  it('ignores an unsupported code in the setting file', () => {
    expect(preferredLocale('nl', 'ja', 'de-DE')).toBe('ja');
  });
});
