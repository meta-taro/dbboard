// Reactive i18n store (Svelte 5 runes). Owns the *current locale* and exposes a
// reactive `t()` so any component that reads it re-renders when the locale
// changes. The pure lookup lives in ./translate; this layer only adds reactive
// state, resolution from the environment, and localStorage persistence.
import { translate, type TranslateParams } from './translate';
import type { MessageKey } from './messages';
import { DEFAULT_LOCALE, resolveLocale, SUPPORTED_CODES } from './locales';

const STORAGE_KEY = 'dbboard.locale';

function readStored(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    // localStorage can throw in locked-down webviews; treat as "no preference".
    return null;
  }
}

function persist(locale: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    // Non-fatal: the choice just won't survive a restart.
  }
}

/**
 * Initial locale: an explicit saved choice wins; otherwise fall back to the
 * browser/OS language; otherwise English. Guarded so it is safe to call during
 * SSR/prerender where `navigator`/`localStorage` are absent.
 */
function initialLocale(): string {
  const stored = readStored();
  if (stored && SUPPORTED_CODES.includes(stored)) return stored;

  const nav = typeof navigator !== 'undefined' ? navigator.language : null;
  return resolveLocale(nav) ?? DEFAULT_LOCALE;
}

class I18n {
  locale = $state<string>(DEFAULT_LOCALE);

  /** Call once on app mount (client-side) to resolve the real locale. */
  init(): void {
    this.locale = initialLocale();
  }

  setLocale(code: string): void {
    if (!SUPPORTED_CODES.includes(code)) return;
    this.locale = code;
    persist(code);
  }

  /**
   * Reactive translate. Reads `this.locale` so consumers re-render on change.
   * Bind as `const t = i18n.t` won't preserve reactivity — call `i18n.t(...)`
   * (or wrap in `$derived`) at the point of use.
   */
  t = (key: MessageKey, params?: TranslateParams): string => {
    return translate(this.locale, key, params);
  };
}

export const i18n = new I18n();
