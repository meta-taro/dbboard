import { describe, expect, it, vi } from 'vitest';

import { reportFirstPaint, resetForTest } from './startup';

/** A `requestAnimationFrame` that runs the callback immediately, so the two
 *  frames the real one waits for do not make the test wait for a browser. */
const immediate = (cb: () => void) => cb();

describe('reportFirstPaint', () => {
  it('waits two animation frames before reporting', async () => {
    resetForTest();
    const order: string[] = [];
    const raf = (cb: () => void) => {
      order.push('frame');
      cb();
    };
    await reportFirstPaint(raf, async (cmd) => {
      order.push(cmd);
    });

    // One frame schedules the paint; the second is after it. Reporting from
    // the first would time a DOM that exists but has not been presented.
    expect(order).toEqual(['frame', 'frame', 'report_first_paint']);
  });

  it('reports once, however many times it is called', async () => {
    resetForTest();
    const send = vi.fn(async () => undefined);
    await reportFirstPaint(immediate, send);
    await reportFirstPaint(immediate, send);
    await reportFirstPaint(immediate, send);

    expect(send).toHaveBeenCalledTimes(1);
  });

  it('a failed report does not throw', async () => {
    resetForTest();
    await expect(
      reportFirstPaint(immediate, async () => {
        throw new Error('no shell listening');
      }),
    ).resolves.toBeUndefined();
  });
});
