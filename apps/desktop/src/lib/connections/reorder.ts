/**
 * Where a dragged row lands, or `null` when the drop is not a move.
 *
 * ▲▼ (`./order.ts`) asks a different question — it steps one place and needs
 * to know whether there is a place to step to. A drag names a *gap*: the
 * space between two rows that the pointer was released over, counted 0..length
 * so that both ends of the list can be named. Converting a gap into the
 * position the backend's `move_to` wants is where the off-by-one lives, and it
 * is why this is a function rather than two lines in the dialog.
 *
 * The dragged row is still in the list while it is being dragged. Lifting it
 * out closes up every gap past it, so a gap below the row is one too high and
 * a gap above it is already correct.
 *
 * @param from - the dragged row's current position
 * @param gap - the gap the pointer was released over, 0..length inclusive
 * @param length - how many rows the list holds
 */
export function dropTarget(from: number, gap: number, length: number): number | null {
  if (from < 0 || from >= length) return null;
  if (gap < 0 || gap > length) return null;
  const target = gap > from ? gap - 1 : gap;
  // The gaps on either side of a row both name the row's own position. A drop
  // there is not an error; it is just not a move, and the connections file
  // should not be rewritten to say what it already says.
  if (target === from) return null;
  return target;
}

/**
 * Which gap a pointer at `y` is over, given each row's vertical midpoint.
 *
 * Midpoints rather than edges: a gap that changed when the pointer touched the
 * boundary between two rows would flip back and forth along that boundary,
 * because inserting the placeholder moves the rows under the pointer. Halfway
 * through a row is far enough from either edge that the answer is stable.
 *
 * @param y - the pointer's position, in the same coordinate space as `midpoints`
 * @param midpoints - each row's vertical middle, in rendered order
 */
export function gapForPointer(y: number, midpoints: readonly number[]): number {
  let gap = 0;
  while (gap < midpoints.length && midpoints[gap] < y) gap += 1;
  return gap;
}
