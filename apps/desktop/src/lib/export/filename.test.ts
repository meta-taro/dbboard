import { describe, expect, it } from 'vitest';

import { fileTimestamp, timestampedFileName } from './filename';

// A fixed local-time instant. Constructed from parts rather than an ISO string
// so the test asserts against the same wall clock the helper reads, whatever
// zone the machine running it is in.
const AT = new Date(2026, 7, 19, 16, 30, 45); // 2026-08-19 16:30:45 local

describe('fileTimestamp', () => {
  it('is the local wall clock, not UTC', () => {
    // The operator picks a file by "the one I exported this morning". A UTC
    // stamp would read as a different hour — and on either side of midnight,
    // a different day — from the clock they were looking at.
    expect(fileTimestamp(AT)).toBe('20260819-163045');
  });

  it('zero-pads every field so the name sorts chronologically', () => {
    // Sorting by name is the whole point of the stamp: an unpadded `2026-8-9`
    // would sort after `2026-12-1`.
    expect(fileTimestamp(new Date(2026, 0, 2, 3, 4, 5))).toBe('20260102-030405');
  });

  it('separates date from time so a human can read it at a glance', () => {
    const stamp = fileTimestamp(AT);
    expect(stamp).toMatch(/^\d{8}-\d{6}$/);
  });
});

describe('timestampedFileName', () => {
  it('inserts the stamp between the stem and the extension', () => {
    expect(timestampedFileName('dbboard-connections', 'dbbx', AT)).toBe(
      'dbboard-connections-20260819-163045.dbbx',
    );
  });

  it('carries no character Windows forbids in a file name', () => {
    // `:` is the obvious trap — it is what `toISOString` and every clock
    // display use, and it is what makes Windows reject the name outright.
    expect(timestampedFileName('dbboard-result', 'csv', AT)).not.toMatch(/[<>:"/\\|?*]/);
  });

  it('keeps two exports of the same thing distinguishable', () => {
    const a = timestampedFileName('dbboard-connections', 'dbbx', AT);
    const b = timestampedFileName('dbboard-connections', 'dbbx', new Date(2026, 7, 19, 16, 30, 46));
    expect(a).not.toBe(b);
  });

  it('defaults to now when no instant is given', () => {
    // The call sites pass nothing; only the tests pin the clock.
    expect(timestampedFileName('x', 'txt')).toMatch(/^x-\d{8}-\d{6}\.txt$/);
  });
});
