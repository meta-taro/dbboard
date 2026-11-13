import { describe, expect, it } from 'vitest';
import { MARGIN, placeBubble, showMs } from './place';

const view = { width: 800, height: 600 };
const bubble = { width: 240, height: 80 };
const inside = (p: { left: number; top: number }) =>
  p.left >= MARGIN &&
  p.top >= MARGIN &&
  p.left + bubble.width <= view.width - MARGIN &&
  p.top + bubble.height <= view.height - MARGIN;

describe('placeBubble', () => {
  it('sits below a target with room, its tail pointing up at it', () => {
    const p = placeBubble({ left: 300, top: 100, width: 80, height: 24 }, bubble, view);
    expect(p.tail).toBe('top');
    expect(p.top).toBeGreaterThan(124);
    expect(inside(p)).toBe(true);
  });

  // The bar for done (ADR-0177): near every corner and edge, the bubble
  // stays inside the window and its tail faces the target.
  it.each([
    ['top-left', { left: 0, top: 0, width: 40, height: 20 }],
    ['top-right', { left: 760, top: 0, width: 40, height: 20 }],
    ['bottom-left', { left: 0, top: 580, width: 40, height: 20 }],
    ['bottom-right', { left: 760, top: 580, width: 40, height: 20 }],
    ['right edge', { left: 790, top: 300, width: 10, height: 20 }],
  ])('stays inside the window at the %s', (_where, target) => {
    expect(inside(placeBubble(target, bubble, view))).toBe(true);
  });

  it('goes above a target at the bottom, tail pointing down', () => {
    expect(placeBubble({ left: 300, top: 560, width: 80, height: 24 }, bubble, view).tail).toBe(
      'bottom',
    );
  });

  it('puts a message with no target at the bottom centre', () => {
    const p = placeBubble(null, bubble, view);
    expect(p.left).toBe((800 - 240) / 2);
    expect(inside(p)).toBe(true);
  });
});

describe('showMs', () => {
  it('uses the seconds the agent gave', () => expect(showMs('hi', 7)).toBe(7000));
  it('grows with the message, up to 20 s', () => {
    expect(showMs('a'.repeat(10))).toBe(6000);
    expect(showMs('a'.repeat(1000))).toBe(20_000);
  });
  it('keeps an outline alone for 2.6 s', () => expect(showMs(undefined)).toBe(2600));
});
