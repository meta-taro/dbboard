// The 11 locales dbboard ships, matching the egui `dbboard-i18n` crate exactly.
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
