// Pure sizing rules for the horizontal split inside the sidebar — the
// connection list above, the table list below (ADR-0131). Same division of
// labour as `splitter.ts`: the component owns the pointer plumbing, this
// module owns what is worth testing.

/** One connection row, including the 1px gap the list puts under it. Kept in
 *  step with `.nav-row`'s padding and `.nav-list`'s gap in Sidebar.svelte. */
export const CONNECTION_ROW_HEIGHT = 30;

/** Two rows' worth. Below this the pane is too short to be a list: there is
 *  nowhere to drop a dragged row, and no way to tell a scroll from a stutter. */
export const CONNECTIONS_MIN_HEIGHT = 60;

/** Fourteen rows. Past this a connection list is scrolled, not scanned, so
 *  more height buys nothing and the tables lose it. */
export const CONNECTIONS_MAX_HEIGHT = 420;

/** On a short window the table list is the one being scrolled all day, so the
 *  connections never take more than this share of the sidebar. */
const MAX_SIDEBAR_FRACTION = 0.5;

const STORAGE_KEY = 'dbboard.connectionsHeight';

/** The tallest the pane may be in a sidebar of this height — the absolute
 *  ceiling, lowered on a short window, but never below the minimum. */
function ceilingFor(sidebarHeight: number): number {
  const share = Number.isFinite(sidebarHeight)
    ? Math.floor(sidebarHeight * MAX_SIDEBAR_FRACTION)
    : CONNECTIONS_MAX_HEIGHT;
  return Math.max(CONNECTIONS_MIN_HEIGHT, Math.min(CONNECTIONS_MAX_HEIGHT, share));
}

export function clampConnectionsHeight(
  height: number,
  sidebarHeight = Number.POSITIVE_INFINITY,
): number {
  if (!Number.isFinite(height)) return CONNECTIONS_MIN_HEIGHT;
  const ceiling = ceilingFor(sidebarHeight);
  return Math.round(Math.min(Math.max(height, CONNECTIONS_MIN_HEIGHT), ceiling));
}

/** Where the divider sits before anyone touches it, and where a double-click
 *  puts it back. Derived from the count rather than fixed: two connections and
 *  twenty want different amounts of room, and neither user should have to
 *  drag to get it. */
export function defaultConnectionsHeight(
  count: number,
  sidebarHeight = Number.POSITIVE_INFINITY,
): number {
  const wanted = Math.max(0, Math.floor(count)) * CONNECTION_ROW_HEIGHT;
  return clampConnectionsHeight(wanted, sidebarHeight);
}

/** The height to render. `chosen` is null until the divider is dragged, and
 *  that null is the feature: an untouched sidebar keeps making room for new
 *  connections by itself, and stops the moment the user has an opinion. */
export function resolveConnectionsHeight(
  chosen: number | null,
  count: number,
  sidebarHeight = Number.POSITIVE_INFINITY,
): number {
  if (chosen === null) return defaultConnectionsHeight(count, sidebarHeight);
  return clampConnectionsHeight(chosen, sidebarHeight);
}

/** null means "never dragged" — see `resolveConnectionsHeight`. A stored value
 *  that no longer parses is treated the same way, since the alternative is
 *  pinning the pane to a number nobody chose. */
export function loadConnectionsHeight(): number | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return null;
    const n = Number(raw);
    if (!Number.isFinite(n)) return null;
    return clampConnectionsHeight(n);
  } catch {
    return null;
  }
}

export function saveConnectionsHeight(height: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(clampConnectionsHeight(height)));
  } catch {
    // Non-fatal: the divider just won't stay put across restarts.
  }
}

/** Hand the pane back to the connection count, for the double-click reset. */
export function resetConnectionsHeight(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Non-fatal, as above.
  }
}
