import { describe, expect, it, beforeEach, vi } from 'vitest';
import {
  DEFAULT_WARN_ROWS,
  clampThreshold,
  loadWarnThreshold,
  saveWarnThreshold,
  exceedsThreshold,
  progressPercent,
  defaultDumpFileName,
  type DumpPlan,
  type DumpProgress,
} from './plan';

const planOf = (counts: number[]): DumpPlan => ({
  tables: counts.map((n, i) => ({ name: `t${i}`, row_count: n })),
  total_rows: counts.reduce((a, b) => a + b, 0),
  is_empty_data: counts.every((n) => n === 0),
});

describe('clampThreshold', () => {
  it('falls back to the default for non-finite or negative input', () => {
    expect(clampThreshold(Number.NaN)).toBe(DEFAULT_WARN_ROWS);
    expect(clampThreshold(-1)).toBe(DEFAULT_WARN_ROWS);
  });

  it('floors a fractional value and keeps a valid one', () => {
    expect(clampThreshold(1000.9)).toBe(1000);
    expect(clampThreshold(0)).toBe(0);
  });
});

describe('exceedsThreshold', () => {
  it('is true only strictly above the threshold (warn-and-allow)', () => {
    const plan = planOf([300_000, 300_000]); // 600k total
    expect(exceedsThreshold(plan, 500_000)).toBe(true);
  });

  it('is false at exactly the threshold', () => {
    const plan = planOf([500_000]);
    expect(exceedsThreshold(plan, 500_000)).toBe(false);
  });

  it('is false below the threshold', () => {
    expect(exceedsThreshold(planOf([10]), 500_000)).toBe(false);
  });
});

describe('progressPercent', () => {
  const prog = (rows_done: number, rows_total: number): DumpProgress => ({
    tables_total: 1,
    tables_done: 0,
    rows_total,
    rows_done,
    current_table: 't',
  });

  it('is 0 when nothing has been written', () => {
    expect(progressPercent(prog(0, 100))).toBe(0);
  });

  it('rounds the mid-run ratio to a whole percent', () => {
    expect(progressPercent(prog(40, 100))).toBe(40);
    expect(progressPercent(prog(1, 3))).toBe(33);
  });

  it('is 100 at completion', () => {
    expect(progressPercent(prog(100, 100))).toBe(100);
  });

  it('treats a zero-row dump as complete rather than dividing by zero', () => {
    // An all-empty database has nothing to write; the bar should read done,
    // not NaN.
    expect(progressPercent(prog(0, 0))).toBe(100);
  });

  it('never exceeds 100 even if a count under-estimates', () => {
    expect(progressPercent(prog(120, 100))).toBe(100);
  });
});

describe('defaultDumpFileName', () => {
  const AT = new Date(2026, 7, 19, 16, 30, 45); // 2026-08-19 16:30:45 local

  it('builds a .sql name from a connection name', () => {
    expect(defaultDumpFileName('Prod DB', AT)).toBe('prod-db-dump-20260819-163045.sql');
  });

  it('falls back to a generic name when no connection name is given', () => {
    expect(defaultDumpFileName(undefined, AT)).toBe('dbboard-dump-20260819-163045.sql');
    expect(defaultDumpFileName('   ', AT)).toBe('dbboard-dump-20260819-163045.sql');
  });

  it('slugifies punctuation and collapses separators', () => {
    expect(defaultDumpFileName('store/A (main)', AT)).toBe('store-a-main-dump-20260819-163045.sql');
  });

  it('proposes a different name for a second dump of the same connection', () => {
    // The one that matters most: a backup silently overwriting the previous
    // backup of the same database loses the only copy of the older state.
    const first = defaultDumpFileName('Prod DB', AT);
    const second = defaultDumpFileName('Prod DB', new Date(2026, 7, 19, 16, 30, 46));
    expect(first).not.toBe(second);
  });
});

describe('loadWarnThreshold / saveWarnThreshold', () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });

  it('returns the default when nothing is persisted', () => {
    expect(loadWarnThreshold()).toBe(DEFAULT_WARN_ROWS);
  });

  it('round-trips a saved threshold', () => {
    saveWarnThreshold(1234);
    expect(loadWarnThreshold()).toBe(1234);
  });

  it('clamps a persisted junk value back to the default', () => {
    localStorage.setItem('dbboard.backup.warnRows', 'not-a-number');
    expect(loadWarnThreshold()).toBe(DEFAULT_WARN_ROWS);
  });
});
