import { describe, it, expect } from 'vitest';
import { clampLimit, ROW_LIMIT_HARD_CAP, DEFAULT_ROW_LIMIT, ROW_LIMIT_OPTIONS } from './limits';

describe('clampLimit', () => {
  it('caps values above the hard cap', () => {
    expect(clampLimit(5000)).toBe(ROW_LIMIT_HARD_CAP);
  });

  it('keeps a valid in-range value', () => {
    expect(clampLimit(500)).toBe(500);
  });

  it('floors fractional values', () => {
    expect(clampLimit(250.9)).toBe(250);
  });

  it('falls back to the default for non-positive or non-finite input', () => {
    expect(clampLimit(0)).toBe(DEFAULT_ROW_LIMIT);
    expect(clampLimit(-10)).toBe(DEFAULT_ROW_LIMIT);
    expect(clampLimit(NaN)).toBe(DEFAULT_ROW_LIMIT);
  });
});

describe('ROW_LIMIT_OPTIONS', () => {
  it('never offers a value above the backend hard cap', () => {
    for (const opt of ROW_LIMIT_OPTIONS) {
      expect(opt).toBeLessThanOrEqual(ROW_LIMIT_HARD_CAP);
    }
  });

  it('includes the default as a selectable option', () => {
    expect(ROW_LIMIT_OPTIONS).toContain(DEFAULT_ROW_LIMIT);
  });
});
