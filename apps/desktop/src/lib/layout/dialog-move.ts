// Pure rules for the connection dialog: when a dismissal gesture is allowed to
// throw work away, and where the dialog may be dragged to (ADR-0132). Same
// division of labour as `splitter.ts` and `panel-split.ts` — the component
// owns the pointer plumbing, this module owns what is worth testing.

/** Which panel of the manager is on screen. Mirrors `Mode` in
 *  ConnectionManager.svelte; kept structural so this module stays pure. */
export type DialogMode = 'list' | 'form' | 'export' | 'import' | 'duplicate' | 'repair';

/** The two gestures that used to close the dialog without being aimed at it. */
export type DismissSource = 'backdrop' | 'escape';

/** `close` dismisses the manager, `back` returns to the connection list,
 *  `ignore` does nothing at all. */
export type DismissResult = 'close' | 'back' | 'ignore';

/**
 * What a stray dismissal should do.
 *
 * The rule is that neither gesture may destroy something the user is part-way
 * through. Leaving is still one deliberate click away — the ✕ in the header,
 * or Cancel in the form — so nothing becomes unreachable, only un-accidental.
 */
export function dismissAction(
  source: DismissSource,
  mode: DialogMode,
  formDirty: boolean,
): DismissResult {
  if (mode === 'list') return 'close';
  // Any panel other than the list holds something the user assembled: typed
  // fields, a chosen import file, a half-answered repair.
  if (source === 'backdrop') return 'ignore';
  // Escape shares a keyboard with the fields being typed into, so a dirty form
  // outranks it. An untouched one costs nothing to leave.
  if (mode === 'form' && formDirty) return 'ignore';
  return 'back';
}

/** How much of the dialog's width stays on screen however far it is shoved
 *  sideways. Enough to be seen and grabbed, not enough to be in the way. */
export const DIALOG_MIN_VISIBLE_X = 120;

/** The header's own height. Below the bottom edge it could not be grabbed, and
 *  a dialog that cannot be grabbed cannot be brought back. */
export const DIALOG_HEADER_VISIBLE = 36;

/** A displacement from the dialog's centred resting place, in CSS pixels. */
export interface DialogOffset {
  dx: number;
  dy: number;
}

/** Everything needed to decide where a dialog may sit. */
export interface DialogGeometry {
  viewportWidth: number;
  viewportHeight: number;
  dialogWidth: number;
  dialogHeight: number;
}

/** Where the dialog sits before anyone drags it, and where a double-click on
 *  the header puts it back. */
export function centredOffset(): DialogOffset {
  return { dx: 0, dy: 0 };
}

function measured(geometry: DialogGeometry): boolean {
  return (
    Number.isFinite(geometry.viewportWidth) &&
    Number.isFinite(geometry.viewportHeight) &&
    geometry.viewportWidth > 0 &&
    geometry.viewportHeight > 0 &&
    geometry.dialogWidth > 0 &&
    geometry.dialogHeight > 0
  );
}

/**
 * Clamp a dragged offset so the dialog stays reachable.
 *
 * Sideways it may leave all but a grabbable strip. Vertically the header is
 * the constraint in both directions: above the top edge there is no way to
 * drag it back, and below the bottom edge there is nothing left to grab.
 */
export function clampDialogOffset(offset: DialogOffset, geometry: DialogGeometry): DialogOffset {
  if (!measured(geometry)) return centredOffset();
  if (!Number.isFinite(offset.dx) || !Number.isFinite(offset.dy)) return centredOffset();

  const { viewportWidth, viewportHeight, dialogWidth, dialogHeight } = geometry;
  const centreLeft = (viewportWidth - dialogWidth) / 2;
  const centreTop = (viewportHeight - dialogHeight) / 2;

  const minLeft = DIALOG_MIN_VISIBLE_X - dialogWidth;
  const maxLeft = Math.max(minLeft, viewportWidth - DIALOG_MIN_VISIBLE_X);
  const maxTop = Math.max(0, viewportHeight - DIALOG_HEADER_VISIBLE);

  const left = Math.min(Math.max(centreLeft + offset.dx, minLeft), maxLeft);
  const top = Math.min(Math.max(centreTop + offset.dy, 0), maxTop);

  return { dx: Math.round(left - centreLeft), dy: Math.round(top - centreTop) };
}
