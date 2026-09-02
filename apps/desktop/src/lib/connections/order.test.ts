import { describe, expect, it } from 'vitest';

import { moveTarget } from './order';

describe('moveTarget', () => {
  it('gives the neighbouring position for a step up or down', () => {
    expect(moveTarget(1, -1, 3)).toBe(0);
    expect(moveTarget(1, 1, 3)).toBe(2);
  });

  it('refuses to walk off either end', () => {
    // The buttons are disabled at the ends, so this is the second line of
    // defence: a click that arrives anyway must not send an out-of-range
    // index to the backend, which answers it with an error banner.
    expect(moveTarget(0, -1, 3)).toBeNull();
    expect(moveTarget(2, 1, 3)).toBeNull();
  });

  it('refuses a row index the list does not have', () => {
    // A stale render — the list was re-read while a click was in flight.
    expect(moveTarget(3, -1, 3)).toBeNull();
    expect(moveTarget(-1, 1, 3)).toBeNull();
  });

  it('has nowhere to move a single row', () => {
    expect(moveTarget(0, -1, 1)).toBeNull();
    expect(moveTarget(0, 1, 1)).toBeNull();
  });
});
