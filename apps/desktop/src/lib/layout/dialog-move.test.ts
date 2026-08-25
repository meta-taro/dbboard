import { describe, it, expect } from 'vitest';
import {
  DIALOG_MIN_VISIBLE_X,
  DIALOG_HEADER_VISIBLE,
  dismissAction,
  clampDialogOffset,
  centredOffset,
  type DialogGeometry,
} from "./dialog-move";

// A roomy window with the connection dialog at its usual size.
const ROOMY: DialogGeometry = {
  viewportWidth: 1440,
  viewportHeight: 900,
  dialogWidth: 560,
  dialogHeight: 600,
};

describe('dismissAction — a half-typed connection is not thrown away by accident', () => {
  it('closes the manager when the backdrop is clicked from the list', () => {
    // Nothing is being typed, so the backdrop keeps its usual meaning.
    expect(dismissAction('backdrop', 'list', false)).toBe('close');
  });

  it("ignores a backdrop click while a form is open, dirty or not", () => {
    // The reported loop: reaching past the dialog to see what is behind it
    // used to close it and take the half-typed connection with it.
    expect(dismissAction('backdrop', 'form', true)).toBe('ignore');
    expect(dismissAction('backdrop', 'form', false)).toBe('ignore');
  });

  it('ignores a backdrop click on the other panels too', () => {
    // Import has a chosen file, repair has a half-answered prompt. Neither is
    // cheaper to lose than a typed form.
    expect(dismissAction('backdrop', 'import', false)).toBe('ignore');
    expect(dismissAction('backdrop', 'export', false)).toBe('ignore');
    expect(dismissAction('backdrop', 'duplicate', false)).toBe('ignore');
    expect(dismissAction('backdrop', 'repair', false)).toBe('ignore');
  });

  it('closes the manager on Escape from the list', () => {
    expect(dismissAction('escape', 'list', false)).toBe('close');
  });

  it('steps back to the list on Escape from an untouched form', () => {
    // Opened the form, typed nothing: Escape is the fastest way back and
    // costs nothing.
    expect(dismissAction('escape', 'form', false)).toBe('back');
  });

  it('ignores Escape once the form has been typed into', () => {
    // Escape sits next to the keys being typed. Losing a connection to a
    // mis-hit is exactly the complaint; leaving is still one click on Cancel.
    expect(dismissAction('escape', 'form', true)).toBe('ignore');
  });

  it('steps back to the list on Escape from the other panels', () => {
    expect(dismissAction('escape', 'import', false)).toBe('back');
    expect(dismissAction('escape', 'repair', false)).toBe('back');
  });
});

describe("clampDialogOffset — the dialog can be moved, but never out of reach", () => {
  it('leaves the centred position alone', () => {
    expect(clampDialogOffset(centredOffset(), ROOMY)).toEqual({ dx: 0, dy: 0 });
  });

  it('passes through a modest nudge untouched', () => {
    expect(clampDialogOffset({ dx: 120, dy: -80 }, ROOMY)).toEqual({
      dx: 120,
      dy: -80,
    });
  });

  it('keeps a grabbable strip on screen when shoved off the right edge', () => {
    const { dx } = clampDialogOffset({ dx: 99999, dy: 0 }, ROOMY);
    const left = (ROOMY.viewportWidth - ROOMY.dialogWidth) / 2 + dx;
    expect(left).toBe(ROOMY.viewportWidth - DIALOG_MIN_VISIBLE_X);
  });

  it('keeps a grabbable strip on screen when shoved off the left edge', () => {
    const { dx } = clampDialogOffset({ dx: -99999, dy: 0 }, ROOMY);
    const left = (ROOMY.viewportWidth - ROOMY.dialogWidth) / 2 + dx;
    expect(left).toBe(DIALOG_MIN_VISIBLE_X - ROOMY.dialogWidth);
  });

  it('never lets the header slide above the top edge', () => {
    // Above the top edge the header is unreachable, and the dialog can never
    // be dragged back — the one move that strands it for good.
    const { dy } = clampDialogOffset({ dx: 0, dy: -99999 }, ROOMY);
    const top = (ROOMY.viewportHeight - ROOMY.dialogHeight) / 2 + dy;
    expect(top).toBe(0);
  });

  it('keeps the header above the bottom edge', () => {
    const { dy } = clampDialogOffset({ dx: 0, dy: 99999 }, ROOMY);
    const top = (ROOMY.viewportHeight - ROOMY.dialogHeight) / 2 + dy;
    expect(top).toBe(ROOMY.viewportHeight - DIALOG_HEADER_VISIBLE);
  });

  it('still yields a reachable header on a window shorter than the dialog', () => {
    // The narrow-screen case from the report: a laptop window so short that
    // the dialog is taller than it. Moving must still be possible.
    const cramped: DialogGeometry = {
      viewportWidth: 900,
      viewportHeight: 420,
      dialogWidth: 560,
      dialogHeight: 600,
    };
    const up = clampDialogOffset({ dx: 0, dy: -99999 }, cramped);
    const top = (cramped.viewportHeight - cramped.dialogHeight) / 2 + up.dy;
    expect(top).toBe(0);
    expect(up.dy).toBeGreaterThan(0); // centring had pushed it above the top
  });

  it('falls back to centred when the geometry has not been measured yet', () => {
    // First paint: the component asks before the dialog has a box.
    const unmeasured: DialogGeometry = {
      viewportWidth: Number.NaN,
      viewportHeight: Number.NaN,
      dialogWidth: 0,
      dialogHeight: 0,
    };
    expect(clampDialogOffset({ dx: 40, dy: 40 }, unmeasured)).toEqual({
      dx: 0,
      dy: 0,
    });
  });

  it('rounds to whole pixels so a dragged dialog does not blur', () => {
    expect(clampDialogOffset({ dx: 10.4, dy: -10.6 }, ROOMY)).toEqual({
      dx: 10,
      dy: -11,
    });
  });
});
