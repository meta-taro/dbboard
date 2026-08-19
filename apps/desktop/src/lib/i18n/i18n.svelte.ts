// Reactive i18n store (Svelte 5 runes). Owns the *current locale* and exposes a
// reactive `t()` so any component that reads it re-renders when the locale
// changes. The pure lookup lives in ./translate; this layer only adds reactive
// state, resolution from the environment, and localStorage persistence.
import type { UnlistenFn } from '@tauri-apps/api/event';

import { translate, type TranslateParams } from './translate';
import type { MessageKey } from './messages';
import { DEFAULT_LOCALE, preferredLocale, SUPPORTED_CODES } from './locales';
import { getUiLocale, onUiLocale, setUiLocale } from '$lib/api';

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

/** The OS language, or null where `navigator` is absent (SSR/prerender). */
function osLanguage(): string | null {
  return typeof navigator !== 'undefined' ? navigator.language : null;
}

/**
 * Initial locale from what is readable *synchronously*: the localStorage
 * choice, else the browser/OS language, else English. `ui-settings.toml`
 * outranks both but only arrives over async IPC — see [`I18n.sync`].
 */
function initialLocale(): string {
  return preferredLocale(null, readStored(), osLanguage());
}

class I18n {
  locale = $state<string>(DEFAULT_LOCALE);

  /** Call once on app mount (client-side) to resolve the real locale. */
  init(): void {
    this.locale = initialLocale();
  }

  /**
   * Adopt the persisted locale and keep following it (ADR-0041).
   *
   * Runs after [`init`], which has already painted in the best locale
   * readable without IPC. Two things happen here:
   *
   * - `ui-settings.toml` wins if it names a locale, because it is the store
   *   shared with MCP clients and with any other process.
   * - If it names none, the locale resolved locally is written into it, so
   *   `get_ui_locale` answers with the language actually on screen instead of
   *   "unset" — an agent asked to switch languages needs to know where it is
   *   starting from.
   *
   * Returns the unlisten for the change subscription. Failures are swallowed:
   * a missing settings file must not stop the app from running in the locale
   * it already resolved.
   */
  async sync(): Promise<UnlistenFn | null> {
    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await onUiLocale((code) => this.adopt(code));
      const { locale } = await getUiLocale();
      if (locale) {
        this.adopt(locale);
      } else {
        await setUiLocale(this.locale);
      }
    } catch {
      // Settings are UI chrome; they must not be able to break startup.
    }
    return unlisten;
  }

  /**
   * Apply a locale chosen outside this window — an MCP client, or a hand edit
   * of the settings file. `null` means the choice was cleared, so fall back to
   * the OS language.
   *
   * Deliberately does *not* write back: the value came from the file, and
   * echoing it would make every external change a second write.
   */
  adopt(code: string | null): void {
    // A hand-edited file can name a locale this build dropped. Ignoring it
    // keeps the current language; falling back would yank the UI to the OS
    // language over a typo.
    if (code !== null && !SUPPORTED_CODES.includes(code)) return;
    const next = preferredLocale(code, null, osLanguage());
    if (next === this.locale) return;
    this.locale = next;
    // Mirror into localStorage so the next launch paints this language before
    // the async read of the settings file lands.
    persist(next);
  }

  setLocale(code: string): void {
    if (!SUPPORTED_CODES.includes(code)) return;
    this.locale = code;
    persist(code);
    // Write-through, not awaited: the language is already switched on screen.
    // A failed write costs persistence, not the current session.
    void setUiLocale(code).catch(() => {});
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
