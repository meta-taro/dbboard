// Reactive state + side effects for the custom (frameless) title bar.
// decorations:false means we draw our own bar and drive the window over Tauri
// IPC. Run outside Tauri (plain `vite dev` in a browser) it silently no-ops so
// the bar still renders — only the buttons go inert.
let maximized = $state(false);

function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// Lazy import: never touch the Tauri API module when running in a browser.
async function appWindow() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return getCurrentWindow();
}

export const titlebarController = {
  /** Whether the window is maximized (reactive when read in a template). */
  get isMaximized(): boolean {
    return maximized;
  },

  /** Restore vs maximize glyph — a filled square when maximized, else outline. */
  get maxGlyph(): string {
    return maximized ? '❐' : '▢';
  },

  /** Adopt the current maximized state on mount and track resizes. */
  async init(): Promise<void> {
    if (!inTauri()) return;
    try {
      const w = await appWindow();
      maximized = await w.isMaximized();
      await w.onResized(async () => {
        try {
          maximized = await w.isMaximized();
        } catch {
          // Without the permission we just stop tracking; display only.
        }
      });
    } catch {
      // Tauri API unavailable = browser run. Bar renders, buttons inert.
    }
  },

  async minimize(): Promise<void> {
    if (!inTauri()) return;
    try {
      await (await appWindow()).minimize();
    } catch {
      // noop
    }
  },

  async toggleMaximize(): Promise<void> {
    if (!inTauri()) return;
    try {
      const w = await appWindow();
      await w.toggleMaximize();
      maximized = await w.isMaximized();
    } catch {
      // noop
    }
  },

  async close(): Promise<void> {
    if (!inTauri()) return;
    try {
      await (await appWindow()).close();
    } catch {
      // noop
    }
  },
};
