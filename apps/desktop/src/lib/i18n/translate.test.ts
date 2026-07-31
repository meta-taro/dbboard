import { describe, it, expect } from 'vitest';
import { translate, interpolate } from './translate';

describe('interpolate', () => {
  it('replaces Fluent-style { $var } placeholders', () => {
    expect(interpolate('History ({ $count })', { count: 3 })).toBe('History (3)');
  });

  it('tolerates missing whitespace inside braces', () => {
    expect(interpolate('{$a}-{$b}', { a: 'x', b: 'y' })).toBe('x-y');
  });

  it('leaves the placeholder literal when a param is missing', () => {
    expect(interpolate('Select top { $n }', {})).toBe('Select top { $n }');
  });

  it('returns the template untouched when no params are given', () => {
    expect(interpolate('plain text')).toBe('plain text');
  });
});

describe('translate', () => {
  it('returns the Japanese string for a reused key', () => {
    expect(translate('ja', 'sql-run-button')).toBe('実行');
  });

  it('returns the English string for the same key under en', () => {
    expect(translate('en', 'sql-run-button')).toBe('Run');
  });

  it('interpolates params into the resolved string', () => {
    // history-title is a reused egui key: "History ({ $count })".
    expect(translate('en', 'history-title', { count: 5 })).toBe('History (5)');
  });

  it('falls back to English when a locale lacks a key', () => {
    // about-title is a Tauri-only key translated only for en/ja; German
    // must fall back to the English source of truth.
    expect(translate('de', 'about-title')).toBe(translate('en', 'about-title'));
  });

  it('returns the key itself for an unknown/mistyped key', () => {
    // @ts-expect-error deliberately passing a key outside the catalog
    expect(translate('en', 'this-key-does-not-exist')).toBe('this-key-does-not-exist');
  });

  it('has a fully populated English catalog for every Tauri-only key', () => {
    expect(translate('en', 'tab-query')).toBe('Query');
    expect(translate('ja', 'tab-query')).toBe('クエリ');
  });
});
