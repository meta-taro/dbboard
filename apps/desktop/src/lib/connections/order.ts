/**
 * Where a row lands when the operator clicks ▲ or ▼, or `null` when it has
 * nowhere to go (issue #192, criterion 1).
 *
 * The stored order *is* the order of `[[connections]]` in the connections
 * file, so a move is expressed as the position the entry ends up at — the
 * same index the backend's `move_to` takes. Keeping the arithmetic here
 * rather than inline in the dialog is what lets the end-of-list cases be
 * tested at all: the buttons disable themselves at the ends, and this is
 * the check behind that.
 *
 * @param index - the row's current position in the rendered list
 * @param delta - -1 for ▲, +1 for ▼
 * @param length - how many rows the list holds
 */
export function moveTarget(index: number, delta: number, length: number): number | null {
  if (index < 0 || index >= length) return null;
  const target = index + delta;
  if (target < 0 || target >= length) return null;
  return target;
}
