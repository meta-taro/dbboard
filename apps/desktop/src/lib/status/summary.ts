// How long the last statement took, said in a way a person can compare.
//
// Kept pure and away from Svelte: the rounding boundaries (a sub-millisecond
// query, the switch to seconds, a duration that arrives impossible) are the
// only interesting part, and they are worth testing without a component.

const SECOND = 1000;
const MINUTE = 60 * SECOND;

/**
 * A duration in milliseconds, rendered for the status bar. Unit-suffixed
 * digits rather than words: this is a measurement, and it must read the same
 * in every one of the locales the app ships.
 */
export function formatElapsed(ms: number): string {
  // A NaN or an infinity means the measurement is not a measurement. Saying
  // "0 ms" is wrong but harmless; letting it through would break the layout
  // and imply a number that was never taken.
  if (!Number.isFinite(ms) || ms <= 0) return '0 ms';
  // Faster than the clock resolves is a real outcome; rounding it to `0 ms`
  // would read as "not measured".
  if (ms < 1) return '<1 ms';
  if (ms < SECOND) return `${Math.round(ms)} ms`;
  if (ms < MINUTE) return `${(ms / SECOND).toFixed(2)} s`;

  const minutes = Math.floor(ms / MINUTE);
  const seconds = Math.floor((ms % MINUTE) / SECOND);
  return `${minutes} m ${String(seconds).padStart(2, '0')} s`;
}
