// The 11 locales dbboard ships (ADR-0015).
// Native labels are what each language calls itself so the switcher is legible
// regardless of the current UI locale.
export interface LocaleMeta {
  code: string;
  /** The language's own name for itself, shown in the switcher. */
  native: string;
}

export const LOCALES: LocaleMeta[] = [
  { code: 'en', native: 'English' },
  { code: 'ja', native: '日本語' },
  { code: 'ko', native: '한국어' },
  { code: 'zh-CN', native: '简体中文' },
  { code: 'zh-TW', native: '繁體中文' },
  { code: 'de', native: 'Deutsch' },
  { code: 'fr', native: 'Français' },
  { code: 'es', native: 'Español' },
  { code: 'pt-BR', native: 'Português (Brasil)' },
  { code: 'ru', native: 'Русский' },
  { code: 'it', native: 'Italiano' },
];

export const DEFAULT_LOCALE = 'en';

export const SUPPORTED_CODES: string[] = LOCALES.map((l) => l.code);

/**
 * Which locale to show, given the three places a preference can come from:
 * `ui-settings.toml` (written by this app and by MCP clients, ADR-0041), this
 * webview's localStorage, and the OS language.
 *
 * The file wins because it is the only one shared across processes — an agent
 * may have switched the language while the app was closed. localStorage is
 * kept as the second rank so an install that predates the file keeps its
 * choice, and so the first synchronous paint has something to use before the
 * file has been read.
 *
 * Codes this build cannot display are skipped rather than honoured: a locale
 * dropped between releases would otherwise leave the UI showing message keys.
 */
export function preferredLocale(
  persisted: string | null,
  stored: string | null,
  osLanguage: string | null,
): string {
  if (persisted && SUPPORTED_CODES.includes(persisted)) return persisted;
  if (stored && SUPPORTED_CODES.includes(stored)) return stored;
  return resolveLocale(osLanguage) ?? DEFAULT_LOCALE;
}

/**
 * Resolve an arbitrary language tag (e.g. from `navigator.language`) to a
 * supported locale code. Tries the exact tag, then the primary subtag against
 * both exact codes and their language prefix (so `ja-JP` → `ja`, `zh` → the
 * first `zh-*`). Returns `null` when nothing matches.
 */
export function resolveLocale(tag: string | null | undefined): string | null {
  if (!tag) return null;
  if (SUPPORTED_CODES.includes(tag)) return tag;

  const primary = tag.split('-')[0].toLowerCase();
  // Exact primary subtag (e.g. "ja").
  const exact = SUPPORTED_CODES.find((c) => c.toLowerCase() === primary);
  if (exact) return exact;
  // Language prefix of a regioned code (e.g. "zh" → "zh-CN").
  const prefixed = SUPPORTED_CODES.find((c) => c.toLowerCase().startsWith(primary + '-'));
  return prefixed ?? null;
}
