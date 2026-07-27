// Row-limit options for the query editor. The backend read path (McpService)
// defaults to 200 rows and HARD-CAPS at 1000 (MAX_MAX_ROWS in
// crates/dbboard-mcp/src/service.rs) — the read surface is for reconnaissance,
// not bulk export, so asking for more is silently clamped there. We mirror that
// ceiling here so the UI never offers a value the backend won't honour.
export const ROW_LIMIT_HARD_CAP = 1000;

export const ROW_LIMIT_OPTIONS: number[] = [100, 200, 500, 1000];

// Default to the hard cap: real inspection work wants as many rows as the read
// path will return, not the backend's conservative 200-row agent default.
export const DEFAULT_ROW_LIMIT = 1000;

const STORAGE_KEY = 'dbboard.rowLimit';

/** Clamp an arbitrary number to the valid [1, hard-cap] range. */
export function clampLimit(n: number): number {
  if (!Number.isFinite(n) || n < 1) return DEFAULT_ROW_LIMIT;
  return Math.min(Math.floor(n), ROW_LIMIT_HARD_CAP);
}

/** Read the persisted limit, falling back to the default when absent/invalid. */
export function loadRowLimit(): number {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT_ROW_LIMIT;
    return clampLimit(Number(raw));
  } catch {
    return DEFAULT_ROW_LIMIT;
  }
}

export function saveRowLimit(n: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(clampLimit(n)));
  } catch {
    // Non-fatal: the choice just won't persist.
  }
}
