import { describe, it, expect } from 'vitest';
import { placePopover } from './popover';

// A 28px-tall chip near the bottom of a 700px window: plenty of room above.
const ROOMY = { top: 500, bottom: 528, left: 40 };
const VIEWPORT = { width: 1000, height: 700 };
const OPTS = { width: 460, preferredHeight: 320 };

describe('placePopover (ADR-0083)', () => {
  it('opens upward when the space above fits the popover', () => {
    const p = placePopover(ROOMY, VIEWPORT, OPTS);
    expect(p.placement).toBe('above');
    expect(p.maxHeight).toBe(320);
    // Anchored by its BOTTOM edge, so a short list hugs the button instead of
    // floating a preferred-height gap below it.
    expect(p.top).toBeNull();
    expect(p.bottom).toBe(VIEWPORT.height - ROOMY.top + 6);
  });

  it('flips below when the button sits too close to the top', () => {
    // The real bug: the editor bar was ~120px down the window, so an upward
    // popover ran off the top and its newest entries were unreachable.
    const p = placePopover({ top: 120, bottom: 148, left: 40 }, VIEWPORT, OPTS);
    expect(p.placement).toBe('below');
    expect(p.bottom).toBeNull();
    expect(p.top).toBe(148 + 6);
  });

  it('shrinks to the available space when neither side fits', () => {
    // Short window: the popover must be capped, never clipped.
    const short = { width: 1000, height: 300 };
    const p = placePopover({ top: 150, bottom: 178, left: 40 }, short, OPTS);
    expect(p.maxHeight).toBeLessThan(320);
    expect(p.maxHeight).toBeGreaterThan(0);
    // Below has 300 - 178 - 6 - 8 = 108; above has 150 - 6 - 8 = 136.
    expect(p.placement).toBe('above');
    expect(p.maxHeight).toBe(136);
  });

  it('picks the roomier side when both are too small', () => {
    const short = { width: 1000, height: 300 };
    const p = placePopover({ top: 100, bottom: 128, left: 40 }, short, OPTS);
    // Above has 86, below has 158 — below wins.
    expect(p.placement).toBe('below');
    expect(p.maxHeight).toBe(158);
  });

  it('pulls a popover that would overflow the right edge back inside', () => {
    const p = placePopover({ top: 500, bottom: 528, left: 900 }, VIEWPORT, OPTS);
    expect(p.left).toBe(1000 - 460 - 8);
  });

  it('never pushes the popover off the left edge', () => {
    // A viewport narrower than the popover itself would otherwise produce a
    // negative left and hide the start of every SQL line.
    const narrow = { width: 300, height: 700 };
    const p = placePopover(ROOMY, narrow, OPTS);
    expect(p.left).toBe(8);
  });

  it('leaves a gap between the button and the popover on both sides', () => {
    const above = placePopover(ROOMY, VIEWPORT, OPTS);
    const below = placePopover({ top: 20, bottom: 48, left: 40 }, VIEWPORT, OPTS);
    expect(above.bottom).toBe(VIEWPORT.height - ROOMY.top + 6);
    expect(below.top).toBe(48 + 6);
  });
});
