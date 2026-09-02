import { describe, expect, it } from 'vitest';

import { dropTarget, gapForPointer } from './reorder';

describe('dropTarget', () => {
  it('converts a gap below the dragged row into the position it lands on', () => {
    // Gaps are counted between rows: gap 3 in a list of 4 is "after the third
    // row". The row being dragged is still in the list while it is dragged, so
    // every gap past it closes up by one once it is lifted out.
    expect(dropTarget(0, 3, 4)).toBe(2);
    expect(dropTarget(0, 4, 4)).toBe(3);
  });

  it('leaves a gap above the dragged row alone', () => {
    // Nothing is removed from in front of it, so the gap is already the
    // position.
    expect(dropTarget(3, 0, 4)).toBe(0);
    expect(dropTarget(3, 1, 4)).toBe(1);
  });

  it('refuses the two gaps that put the row back where it started', () => {
    // Both sides of a row name its own position. Neither is an error, but
    // sending either to the backend rewrites the connections file to say what
    // it already said.
    expect(dropTarget(1, 1, 4)).toBeNull();
    expect(dropTarget(1, 2, 4)).toBeNull();
  });

  it('refuses a gap the list does not have', () => {
    expect(dropTarget(0, -1, 4)).toBeNull();
    expect(dropTarget(0, 5, 4)).toBeNull();
  });

  it('refuses a row index the list does not have', () => {
    // A pointer released after the list was re-read underneath it.
    expect(dropTarget(4, 0, 4)).toBeNull();
    expect(dropTarget(-1, 0, 4)).toBeNull();
  });

  it('has nowhere to drop the only row', () => {
    expect(dropTarget(0, 0, 1)).toBeNull();
    expect(dropTarget(0, 1, 1)).toBeNull();
  });
});

describe('gapForPointer', () => {
  const midpoints = [10, 30, 50, 70];

  it('counts the rows the pointer has passed the middle of', () => {
    // Crossing a row's midpoint is what makes the gap move, not touching its
    // edge — otherwise the list would flicker between two gaps along the
    // boundary between two rows.
    expect(gapForPointer(0, midpoints)).toBe(0);
    expect(gapForPointer(9, midpoints)).toBe(0);
    expect(gapForPointer(11, midpoints)).toBe(1);
    expect(gapForPointer(31, midpoints)).toBe(2);
  });

  it('names the gap past the last row', () => {
    expect(gapForPointer(999, midpoints)).toBe(4);
  });

  it('clamps a pointer dragged above the list', () => {
    expect(gapForPointer(-500, midpoints)).toBe(0);
  });

  it('has one gap in an empty list', () => {
    expect(gapForPointer(42, [])).toBe(0);
  });
});
