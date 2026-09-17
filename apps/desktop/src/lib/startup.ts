// Telling the shell when the interface first showed something (ADR-0156).
//
// `docs/startup-measurement.md` times launch to a window *existing* — the
// moment the window server hands one over, which is before the webview has
// drawn anything. From outside that is all you can see. So the app says it.
//
// **What counts as painted is a decision, not a detail.** Reporting from
// `onMount` would fire while the screen is still blank: Svelte has built the
// DOM, but the browser has not put pixels on it. Two animation frames later
// is the first moment something has been presented — one frame to schedule
// the paint, one to be after it.

import { invoke } from '@tauri-apps/api/core';

export interface StartupReading {
  /** Milliseconds from the shell starting to this paint. */
  first_paint_ms: number | null;
  /** Milliseconds the process has been up. */
  uptime_ms: number;
}

/** Whether this window has already reported. A reload gets a fresh module, so
 *  this guards repeats within one page life; the shell ignores the rest. */
let reported = false;

/**
 * Report the first paint, once.
 *
 * Failure is swallowed on purpose. This is a measurement, and a measurement
 * that can stop the app from starting is worse than no measurement — the same
 * reasoning as the rollback snapshot (ADR-0155).
 */
export async function reportFirstPaint(
  raf: (cb: () => void) => void = (cb) => requestAnimationFrame(cb),
  send: (cmd: string) => Promise<unknown> = (cmd) => invoke(cmd),
): Promise<void> {
  if (reported) return;
  reported = true;

  await new Promise<void>((resolve) => raf(() => raf(() => resolve())));
  try {
    await send('report_first_paint');
  } catch {
    // Nothing to do and nothing worth saying: the number is a nicety.
  }
}

/** Test seam: forget that this window reported. */
export function resetForTest(): void {
  reported = false;
}
