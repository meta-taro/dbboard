// Pure logic for the logical-backup (dump) UI (ADR-0049/0050): the huge-DB
// warn threshold, progress arithmetic, and the default file name. Kept free of
// Tauri so it is unit-testable in isolation; the command wrappers live in
// `$lib/api` and the wiring in the backup component.

import { timestampedFileName } from '$lib/export/filename';

// Wire shapes mirroring the backend DTOs (src-tauri/src/dump.rs). snake_case
// because the Rust structs derive `Serialize` with default field names.
export interface DumpTable {
  name: string;
  row_count: number;
}

export interface DumpPlan {
  tables: DumpTable[];
  total_rows: number;
  is_empty_data: boolean;
}

export interface TableFailure {
  table: string;
  message: string;
}

export interface TableTruncation {
  table: string;
  rows_written: number;
}

export interface DumpOutcome {
  tables_dumped: number;
  rows_written: number;
  failures: TableFailure[];
  truncations: TableTruncation[];
  cancelled: boolean;
}

// The `dump:progress` event payload, emitted repeatedly during a run.
export interface DumpProgress {
  tables_total: number;
  tables_done: number;
  rows_total: number;
  rows_done: number;
  current_table: string | null;
}

// Default huge-DB warn threshold. Mirrors `DEFAULT_BACKUP_WARN_ROWS` in
// crates/dbboard-core/src/dump/plan.rs. Warn-and-allow (ADR-0049 Decision 8):
// crossing it prompts a confirmation, it never blocks the dump. Frontend-owned
// (localStorage, like theme/language) so the user can tune it without a config
// file.
export const DEFAULT_WARN_ROWS = 500_000;

const STORAGE_KEY = 'dbboard.backup.warnRows';

/** Clamp an arbitrary number to a valid, whole, non-negative threshold. */
export function clampThreshold(n: number): number {
  if (!Number.isFinite(n) || n < 0) return DEFAULT_WARN_ROWS;
  return Math.floor(n);
}

/** Read the persisted threshold, falling back to the default when absent/invalid. */
export function loadWarnThreshold(): number {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT_WARN_ROWS;
    const n = Number(raw);
    return Number.isFinite(n) && n >= 0 ? Math.floor(n) : DEFAULT_WARN_ROWS;
  } catch {
    return DEFAULT_WARN_ROWS;
  }
}

export function saveWarnThreshold(n: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(clampThreshold(n)));
  } catch {
    // Non-fatal: the choice just won't persist.
  }
}

/**
 * Whether a plan's total row count *exceeds* the threshold — the warn-and-allow
 * gate. Exactly at the threshold does not exceed it, mirroring the backend's
 * `DumpPlan::exceeds_threshold`.
 */
export function exceedsThreshold(plan: DumpPlan, threshold: number): boolean {
  return plan.total_rows > threshold;
}

/**
 * Whole-percent progress (0–100) for the bar. A zero-row dump reads as 100
 * (there is nothing to write) rather than NaN, and an over-count can never push
 * the bar past 100.
 */
export function progressPercent(p: DumpProgress): number {
  if (p.rows_total <= 0) return 100;
  const pct = Math.round((p.rows_done / p.rows_total) * 100);
  return Math.max(0, Math.min(100, pct));
}

/**
 * Default save-dialog file name: `<slug>-dump-<YYYYMMDD-HHMMSS>.sql` from the
 * connection name, or `dbboard-dump-<stamp>.sql` when there is no usable name.
 *
 * The connection name alone does not distinguish two dumps of the *same*
 * database, so the dialog used to propose the name of the existing backup and
 * offer to overwrite it — losing the only copy of the older state on a stray
 * "yes". The stamp also makes a directory of dumps read as a history.
 */
export function defaultDumpFileName(connectionName?: string, now?: Date): string {
  const slug = (connectionName ?? '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
  return timestampedFileName(slug ? `${slug}-dump` : 'dbboard-dump', 'sql', now);
}
