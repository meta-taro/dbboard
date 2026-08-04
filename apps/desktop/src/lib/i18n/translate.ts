// Pure translation core, framework-free so it is trivially unit-testable and
// reusable by the runes store. The store owns the *current locale* reactive
// state; this module only knows how to turn (locale, key, params) into a
// string against the catalogs.
import { catalogs, en, type MessageKey } from './messages';
import { DEFAULT_LOCALE } from './locales';

export type TranslateParams = Record<string, string | number>;

/**
 * Interpolate Fluent-style `{ $var }` placeholders. Whitespace inside the
 * braces is tolerant so both `{$x}` and `{ $x }` work. Missing params are left
 * as the literal placeholder rather than throwing, so a partial translation
 * degrades visibly instead of crashing the UI.
 */
export function interpolate(template: string, params?: TranslateParams): string {
  if (!params) return template;
  return template.replace(/\{\s*\$([a-zA-Z0-9_-]+)\s*\}/g, (whole, name: string) => {
    const value = params[name];
    return value === undefined ? whole : String(value);
  });
}

/**
 * Look up `key` for `locale`, falling back to English, then to the raw key.
 * Falling back to the key (rather than empty string) makes an untranslated or
 * mistyped key obvious in the UI instead of silently rendering blank.
 */
export function translate(locale: string, key: MessageKey, params?: TranslateParams): string {
  const primary = catalogs[locale]?.[key];
  const fallback = primary ?? catalogs[DEFAULT_LOCALE]?.[key] ?? en[key];
  const template = fallback ?? key;
  return interpolate(template, params);
}
