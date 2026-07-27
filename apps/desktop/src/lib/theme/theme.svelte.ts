// Theme controller: Auto | Light | Dark, Auto being the default (ADR-0041).
//
// "Auto" means follow the OS: we leave `data-theme` off the root and let the
// `prefers-color-scheme` media query in tokens.css drive the palette. An
// explicit Light/Dark choice stamps `data-theme` on the root, which overrides
// the media query in both directions (see tokens.css). The choice persists in
// localStorage so it survives reloads; `app.html` applies it before first
// paint to avoid a flash of the wrong theme.

export type ThemeMode = 'auto' | 'light' | 'dark';

const STORAGE_KEY = 'dbboard-theme';

function isMode(value: string | null): value is ThemeMode {
  return value === 'auto' || value === 'light' || value === 'dark';
}

class ThemeController {
  /** The user's selection. `auto` follows the OS. */
  mode = $state<ThemeMode>('auto');

  /** The palette actually showing — `auto` resolved against the OS. */
  resolved = $state<'light' | 'dark'>('light');

  #media: MediaQueryList | null = null;

  /** Read the persisted choice and start tracking the OS preference. Safe to
   *  call in the browser only (guarded); a no-op during SSR/prerender. */
  init(): void {
    if (typeof window === 'undefined') return;

    const stored = localStorage.getItem(STORAGE_KEY);
    if (isMode(stored)) this.mode = stored;

    this.#media = window.matchMedia('(prefers-color-scheme: dark)');
    // When on Auto, an OS theme switch must repaint live.
    this.#media.addEventListener('change', () => this.#apply());

    this.#apply();
  }

  /** Change the theme and persist it. */
  set(mode: ThemeMode): void {
    this.mode = mode;
    if (typeof window !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, mode);
    }
    this.#apply();
  }

  #apply(): void {
    if (typeof document === 'undefined') return;

    const osDark = this.#media?.matches ?? false;
    this.resolved =
      this.mode === 'auto' ? (osDark ? 'dark' : 'light') : this.mode;

    const root = document.documentElement;
    if (this.mode === 'auto') {
      // Hand control back to the media query.
      root.removeAttribute('data-theme');
    } else {
      root.setAttribute('data-theme', this.mode);
    }
  }
}

export const theme = new ThemeController();
