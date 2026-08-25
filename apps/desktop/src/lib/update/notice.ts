// Pure logic for the desktop auto-update notice (ADR-0067). No Tauri imports:
// the plugin calls live in `$lib/api`, this module only decides "is it newer"
// and folds download progress, so both are unit-testable in isolation.
//
// The version-compare rules mirror the egui client's update_check.rs
// (ADR-0040): tolerate a leading `v`, fill missing minor/patch with 0, drop
// pre-release/build metadata, and treat any unparseable tag as "not newer" so
// a malformed release never nags with a phantom update.

/** A newer release the notice offers, mapped from the plugin's `Update`. */
export interface AvailableUpdate {
  version: string;
  currentVersion: string;
  notes: string;
  date: string | null;
}

// --- Version comparison (parity with egui update_check.rs) ----------------

/** Strip a single leading `v`/`V` and surrounding whitespace so `v0.4.0` and
 *  `0.4.0` compare identically. */
export function normalizeVersion(raw: string): string {
  return raw.trim().replace(/^[vV]/, '');
}

interface Parsed {
  major: number;
  minor: number;
  patch: number;
}

/**
 * Parse `v0.4.0`, `0.4`, or `1` into numeric components, filling a missing
 * minor/patch with 0 and dropping any `-pre`/`+build` suffix. Returns null when
 * the numeric core does not parse (empty or non-digit), so an unrecognised tag
 * is treated as "no update" rather than a spurious one.
 */
export function parseVersion(raw: string): Parsed | null {
  const core = normalizeVersion(raw).split(/[-+]/)[0];
  const parts = core.split('.');
  const [major, minor, patch] = [parts[0], parts[1] ?? '0', parts[2] ?? '0'];
  const digits = /^\d+$/;
  if (![major, minor, patch].every((p) => digits.test(p))) return null;
  return { major: Number(major), minor: Number(minor), patch: Number(patch) };
}

/**
 * True when `latest` is strictly greater than `current`. When either side fails
 * to parse we return false: a malformed tag must never manufacture a phantom
 * update. The plugin already gates on version, but this stays a defensive guard
 * so a misconfigured endpoint offering the same/older build never nags.
 */
export function isNewer(current: string, latest: string): boolean {
  const a = parseVersion(current);
  const b = parseVersion(latest);
  if (!a || !b) return false;
  if (b.major !== a.major) return b.major > a.major;
  if (b.minor !== a.minor) return b.minor > a.minor;
  return b.patch > a.patch;
}

// --- Download progress fold -----------------------------------------------

/** The plugin's download lifecycle events (mirrors
 *  `@tauri-apps/plugin-updater`'s `DownloadEvent`). */
export type DownloadEvent =
  | { event: 'Started'; data: { contentLength?: number } }
  | { event: 'Progress'; data: { chunkLength: number } }
  | { event: 'Finished' };

/** Running download totals folded from `DownloadEvent`s. `total` stays null
 *  until `Started` (and if the server sent no length), so the bar can show an
 *  indeterminate state instead of a wrong number. */
export interface DownloadState {
  downloaded: number;
  total: number | null;
}

export function emptyDownload(): DownloadState {
  return { downloaded: 0, total: null };
}

/** Fold one event in (pure, non-mutating): `Started` sets the total and resets
 *  the counter, `Progress` adds the chunk, `Finished` leaves totals as-is. */
export function foldDownload(
  state: DownloadState,
  event: DownloadEvent,
): DownloadState {
  switch (event.event) {
    case 'Started':
      return { downloaded: 0, total: event.data.contentLength ?? null };
    case 'Progress':
      return { ...state, downloaded: state.downloaded + event.data.chunkLength };
    case 'Finished':
      return state;
  }
}

/** Percent complete 0..100 (rounded), or null when the total size is unknown or
 *  zero — the UI then shows an indeterminate spinner rather than a wrong bar. */
export function downloadPercent(state: DownloadState): number | null {
  if (state.total === null || state.total <= 0) return null;
  const pct = Math.round((state.downloaded / state.total) * 100);
  return Math.min(100, Math.max(0, pct));
}

/** Where to fetch an installer by hand when the in-place update does not
 *  land. The download page rather than a release tag: it always points at
 *  the current build, so it cannot go stale the way a pinned tag would. */
export const DOWNLOAD_PAGE_URL = 'https://meta-taro.github.io/dbboard/';

/** An update that was started on a previous run and never completed — the
 *  installer took over and the app came back as the old build. Mirrors
 *  `StalledUpdate` in `crates/dbboard-config/src/update_attempt.rs`. */
export interface StalledUpdate {
  /** The version still running: the one the update meant to replace. */
  from: string;
  /** The version that was being installed. */
  to: string;
}
