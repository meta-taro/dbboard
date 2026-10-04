import { describe, expect, it } from 'vitest';

import { Latency, untilPainted } from './latency.svelte';

/** A clock the test moves by hand. */
function fakeClock(start = 1000) {
  let t = start;
  return {
    now: () => t,
    advance: (ms: number) => {
      t += ms;
    },
  };
}

describe('untilPainted', () => {
  it('stops the clock two frames after the work, not when the work returns', async () => {
    const clock = fakeClock();
    const raf = (cb: () => void) => {
      // Each frame costs 16 ms. A stopwatch read when the promise resolved
      // would miss both — and those frames are the time a person waits for
      // the rows to actually appear.
      clock.advance(16);
      cb();
    };

    const sample = await untilPainted(
      async () => {
        clock.advance(200);
        return 'rows';
      },
      { now: clock.now, raf },
    );

    expect(sample.value).toBe('rows');
    expect(sample.ms).toBe(232);
    expect(sample.failed).toBe(false);
  });

  it('times a failure too, and hands the error back', async () => {
    const clock = fakeClock();
    const boom = new Error('tunnel closed');

    const sample = await untilPainted(
      async () => {
        clock.advance(4000);
        throw boom;
      },
      { now: clock.now, raf: (cb) => cb() },
    );

    // A connection that gave up after four seconds is a different problem
    // from one refused at once; dropping the number would hide which.
    expect(sample.failed).toBe(true);
    expect(sample.error).toBe(boom);
    expect(sample.ms).toBe(4000);
  });
});

describe('Latency', () => {
  it('starts with nothing measured', () => {
    const latency = new Latency();
    expect(latency.tables).toBeNull();
    expect(latency.firstRow).toBeNull();
  });

  it('keeps the latest sample of each kind, labelled by engine', () => {
    const latency = new Latency();
    latency.recordTables({ ms: 900, failed: false }, 'mysql');
    latency.recordTables({ ms: 300, failed: false }, 'postgres');
    latency.recordFirstRow({ ms: 120, failed: false }, 'postgres');

    expect(latency.tables).toEqual({ ms: 300, failed: false, kind: 'postgres' });
    expect(latency.firstRow).toEqual({ ms: 120, failed: false, kind: 'postgres' });
  });

  it('rounds to whole milliseconds, because that is all the clock is worth', () => {
    const latency = new Latency();
    latency.recordTables({ ms: 812.4999, failed: false }, 'd1');
    expect(latency.tables?.ms).toBe(812);
  });
});
