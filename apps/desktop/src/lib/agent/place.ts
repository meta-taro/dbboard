// Where the speech bubble goes (ADR-0177): beside what the cursor points at,
// always at least MARGIN inside the window, with its tail toward the target.
// Pure arithmetic so the corners can be tested without a browser.

export const MARGIN = 8;
/** Gap between the target and the bubble, room for the tail. */
const GAP = 12;
/**
 * Below the target the cursor's own "Agent" tag hangs down from its centre,
 * so a bubble there keeps clear of it rather than covering who is speaking.
 */
const BELOW_GAP = 30;

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export type Side = 'top' | 'bottom' | 'left' | 'right';

export interface Placement {
  left: number;
  top: number;
  /** The bubble's side the tail sits on, facing the target. */
  tail: Side;
}

/**
 * Below the target if it fits, else above, else to the right, else to the
 * left; then pushed back inside the window on both axes.
 */
export function placeBubble(
  target: Rect | null,
  bubble: { width: number; height: number },
  view: { width: number; height: number },
): Placement {
  const maxLeft = Math.max(MARGIN, view.width - bubble.width - MARGIN);
  const maxTop = Math.max(MARGIN, view.height - bubble.height - MARGIN);
  const clampX = (x: number) => Math.min(Math.max(x, MARGIN), maxLeft);
  const clampY = (y: number) => Math.min(Math.max(y, MARGIN), maxTop);
  if (!target) {
    // A `say` with nothing to point at sits at the bottom centre.
    return { left: clampX((view.width - bubble.width) / 2), top: maxTop, tail: 'bottom' };
  }
  const centreX = target.left + target.width / 2 - bubble.width / 2;
  const centreY = target.top + target.height / 2 - bubble.height / 2;
  const below = target.top + target.height + BELOW_GAP;
  if (below + bubble.height <= view.height - MARGIN) {
    return { left: clampX(centreX), top: below, tail: 'top' };
  }
  const above = target.top - GAP - bubble.height;
  if (above >= MARGIN) {
    return { left: clampX(centreX), top: above, tail: 'bottom' };
  }
  const right = target.left + target.width + GAP;
  if (right + bubble.width <= view.width - MARGIN) {
    return { left: right, top: clampY(centreY), tail: 'left' };
  }
  return { left: clampX(target.left - GAP - bubble.width), top: clampY(centreY), tail: 'right' };
}

/** How long a message stays: its length decides unless the agent said. */
export function showMs(message: string | undefined, seconds?: number): number {
  if (seconds) return seconds * 1000;
  if (!message) return 2600;
  return Math.min(20_000, 5000 + [...message].length * 100);
}
