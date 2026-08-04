// Pure sizing rules for the sidebar/main splitter (ADR-0083). The divider
// component owns the pointer plumbing; this module owns the two things worth
// testing — what widths are legal, and how one survives a restart.

/** Where the divider sits before anyone touches it, and where a double-click
 *  puts it back. Matches the sidebar's original fixed width. */
export const SIDEBAR_DEFAULT_WIDTH = 260;

/** Narrow enough to give the grid room, wide enough that a table name is still
 *  readable rather than an ellipsis. */
export const SIDEBAR_MIN_WIDTH = 160;

/** Absolute ceiling, for schema-qualified names on a wide monitor. */
export const SIDEBAR_MAX_WIDTH = 640;

/** On a narrow window the absolute ceiling is meaningless — the result grid is
 *  the point of the app, so the sidebar never takes more than this share. */
const MAX_VIEWPORT_FRACTION = 0.5;

const STORAGE_KEY = 'dbboard.sidebarWidth';

/**
 * Clamp a candidate sidebar width to something usable in a `viewportWidth`-wide
 * window.
 *
 * The minimum wins over the viewport cap: on a window too narrow for both
 * panes, a cramped sidebar beats an unreadable one, and the grid can scroll.
 */
export function clampSidebarWidth(width: number, viewportWidth = Number.POSITIVE_INFINITY): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT_WIDTH;
  const viewportCap = Number.isFinite(viewportWidth)
    ? Math.floor(viewportWidth * MAX_VIEWPORT_FRACTION)
    : SIDEBAR_MAX_WIDTH;
  const ceiling = Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, viewportCap));
  return Math.round(Math.min(Math.max(width, SIDEBAR_MIN_WIDTH), ceiling));
}

/** Read the persisted width, falling back to the default when absent, invalid,
 *  or out of range. */
export function loadSidebarWidth(): number {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return SIDEBAR_DEFAULT_WIDTH;
    const n = Number(raw);
    if (!Number.isFinite(n)) return SIDEBAR_DEFAULT_WIDTH;
    return clampSidebarWidth(n);
  } catch {
    return SIDEBAR_DEFAULT_WIDTH;
  }
}

export function saveSidebarWidth(width: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(clampSidebarWidth(width)));
  } catch {
    // Non-fatal: the divider just won't stay put across restarts.
  }
}

/** Forget the stored width and report the default, for the double-click reset. */
export function resetSidebarWidth(): number {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Non-fatal, as above.
  }
  return SIDEBAR_DEFAULT_WIDTH;
}
