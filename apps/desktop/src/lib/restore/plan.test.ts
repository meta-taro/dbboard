import { describe, expect, it } from 'vitest';
import {
  restoreProgressPercent,
  needsConfirmation,
  hasUnparsed,
  restoreHadFailures,
  restoreFileFilters,
  normalizeOnError,
  type RestorePlan,
  type RestoreOutcome,
  type RestoreProgress,
  type OnError,
} from './plan';

const planOf = (over: Partial<RestorePlan> = {}): RestorePlan => ({
  statements_total: 0,
  ddl_count: 0,
  data_count: 0,
  unparsed_count: 0,
  existing_tables: [],
  is_target_empty: true,
  ...over,
});

describe('restoreProgressPercent', () => {
  const prog = (done: number, total: number): RestoreProgress => ({
    statements_total: total,
    statements_done: done,
    current_index: done,
  });

  it('is 0 before the first statement runs', () => {
    expect(restoreProgressPercent(prog(0, 10))).toBe(0);
  });

  it('rounds the mid-run ratio to a whole percent', () => {
    expect(restoreProgressPercent(prog(4, 10))).toBe(40);
    expect(restoreProgressPercent(prog(1, 3))).toBe(33);
  });

  it('is 100 at completion', () => {
    expect(restoreProgressPercent(prog(10, 10))).toBe(100);
  });

  it('treats an empty script as complete rather than dividing by zero', () => {
    expect(restoreProgressPercent(prog(0, 0))).toBe(100);
  });

  it('never exceeds 100 even if a count is off', () => {
    expect(restoreProgressPercent(prog(12, 10))).toBe(100);
  });
});

describe('needsConfirmation', () => {
  it('is false against an empty target (no gate)', () => {
    expect(needsConfirmation(planOf({ is_target_empty: true }))).toBe(false);
  });

  it('is true when the target already has tables', () => {
    expect(
      needsConfirmation(
        planOf({ is_target_empty: false, existing_tables: ['users'] }),
      ),
    ).toBe(true);
  });
});

describe('hasUnparsed', () => {
  it('flags a plan with statements the classifier could not parse', () => {
    expect(hasUnparsed(planOf({ unparsed_count: 2 }))).toBe(true);
    expect(hasUnparsed(planOf({ unparsed_count: 0 }))).toBe(false);
  });
});

describe('restoreHadFailures', () => {
  const outcome = (over: Partial<RestoreOutcome> = {}): RestoreOutcome => ({
    statements_run: 0,
    ddl_run: 0,
    data_run: 0,
    failures: [],
    cancelled: false,
    atomic: false,
    ...over,
  });

  it('is false for a clean run', () => {
    expect(restoreHadFailures(outcome())).toBe(false);
  });

  it('is true when any statement failed', () => {
    expect(
      restoreHadFailures(
        outcome({ failures: [{ index: 3, message: 'boom' }] }),
      ),
    ).toBe(true);
  });
});

describe('normalizeOnError', () => {
  it('keeps the two known policies', () => {
    expect(normalizeOnError('stop')).toBe<OnError>('stop');
    expect(normalizeOnError('continue')).toBe<OnError>('continue');
  });

  it('falls back to the safe default for anything else', () => {
    expect(normalizeOnError('garbage')).toBe<OnError>('stop');
    expect(normalizeOnError('')).toBe<OnError>('stop');
  });
});

describe('restoreFileFilters', () => {
  it('offers .sql for the open dialog', () => {
    const filters = restoreFileFilters();
    expect(filters).toEqual([{ name: 'SQL', extensions: ['sql'] }]);
  });
});
