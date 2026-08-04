// Pure logic for the logical-restore (import) UI (ADR-0051): statement-based
// progress arithmetic, the empty-target confirmation gate, and the open-dialog
// file filter. Kept free of Tauri so it is unit-testable in isolation; the
// command wrappers live in `$lib/api` and the wiring in the restore component.
//
// Unlike the dump side there is no warn threshold — a restore's one safety gate
// is the empty-target confirmation (writing into a database that already has
// tables), surfaced by `needsConfirmation`.

// Wire shapes mirroring the backend DTOs (src-tauri/src/restore.rs). snake_case
// because the Rust structs derive `Serialize` with default field names.

/** The preflight summary. Counts are of the *runnable* statements only —
 *  transaction-control statements are stripped by the runner and excluded. */
export interface RestorePlan {
  statements_total: number;
  ddl_count: number;
  data_count: number;
  /** Statements the classifier could not parse; they still run verbatim. */
  unparsed_count: number;
  /** The target's existing user tables. Non-empty ⇒ the run needs `confirmed`. */
  existing_tables: string[];
  is_target_empty: boolean;
}

export interface StatementFailure {
  index: number;
  message: string;
}

export interface RestoreOutcome {
  statements_run: number;
  ddl_run: number;
  data_run: number;
  failures: StatementFailure[];
  cancelled: boolean;
  /** True if the script ran as one atomic batch (all-or-nothing). */
  atomic: boolean;
}

// The `restore:progress` event payload, emitted repeatedly during a run.
export interface RestoreProgress {
  statements_total: number;
  statements_done: number;
  current_index: number | null;
}

/** On-error policy for the per-statement (non-atomic) path. `stop` is safest. */
export type OnError = 'stop' | 'continue';

/**
 * Whole-percent progress (0–100) for the bar, by statement count. An empty
 * script reads as 100 (nothing to run) rather than NaN, and an over-count can
 * never push the bar past 100.
 */
export function restoreProgressPercent(p: RestoreProgress): number {
  if (p.statements_total <= 0) return 100;
  const pct = Math.round((p.statements_done / p.statements_total) * 100);
  return Math.max(0, Math.min(100, pct));
}

/**
 * Whether the run needs the empty-target confirmation: writing into a database
 * that already holds tables. An empty target restores without a prompt.
 */
export function needsConfirmation(plan: RestorePlan): boolean {
  return !plan.is_target_empty;
}

/** Whether the script contains statements the classifier could not parse. */
export function hasUnparsed(plan: RestorePlan): boolean {
  return plan.unparsed_count > 0;
}

/** Whether any statement failed on the per-statement path. */
export function restoreHadFailures(outcome: RestoreOutcome): boolean {
  return outcome.failures.length > 0;
}

/**
 * Coerce an arbitrary string to a known on-error policy. Anything other than
 * the explicit `"continue"` is the safe default — mirrors `on_error_from` in
 * src-tauri/src/restore.rs so the two ends agree.
 */
export function normalizeOnError(raw: string): OnError {
  return raw === 'continue' ? 'continue' : 'stop';
}

/** Open-dialog filter: restore reads a plain `.sql` script. */
export function restoreFileFilters(): { name: string; extensions: string[] }[] {
  return [{ name: 'SQL', extensions: ['sql'] }];
}
