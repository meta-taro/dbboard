import { describe, it, expect } from 'vitest';
import { formatElapsed } from './summary';

describe('formatElapsed', () => {
  it('reads whole milliseconds below a second', () => {
    expect(formatElapsed(0)).toBe('0 ms');
    expect(formatElapsed(12)).toBe('12 ms');
    expect(formatElapsed(12.6)).toBe('13 ms');
    expect(formatElapsed(999)).toBe('999 ms');
  });

  // A query that finished faster than the clock can resolve did happen, and
  // "0 ms" reads as "not measured".
  it('does not round a real measurement down to nothing', () => {
    expect(formatElapsed(0.4)).toBe('<1 ms');
  });

  it('switches to seconds at a second, keeping two decimals', () => {
    expect(formatElapsed(1000)).toBe('1.00 s');
    expect(formatElapsed(1236)).toBe('1.24 s');
    expect(formatElapsed(59_994)).toBe('59.99 s');
  });

  it('switches to minutes at a minute', () => {
    expect(formatElapsed(60_000)).toBe('1 m 00 s');
    expect(formatElapsed(65_400)).toBe('1 m 05 s');
    expect(formatElapsed(3_723_000)).toBe('62 m 03 s');
  });

  // The measurement comes from a monotonic clock, but the footer must not be
  // the place where an odd value from one turns into a broken layout.
  it('treats an impossible duration as zero rather than showing it', () => {
    expect(formatElapsed(-1)).toBe('0 ms');
    expect(formatElapsed(Number.NaN)).toBe('0 ms');
    expect(formatElapsed(Number.POSITIVE_INFINITY)).toBe('0 ms');
  });
});
