// Pure placement maths for popovers anchored to a button (ADR-0083).
//
// A popover positioned with `position: absolute` inside a scrolling pane is
// clipped by that pane: the query history opened upward from the editor bar and
// its top — the newest entries — was cut off by the tab pane's edge. Placing it
// with `position: fixed` escapes the clip, but then nothing keeps it on screen,
// so the geometry has to be worked out explicitly. That is this module; it is
// deliberately DOM-free so the flipping and clamping can be unit-tested.

/** The anchor button's viewport-relative box (a `DOMRect` subset). */
export interface AnchorRect {
  top: number;
  bottom: number;
  left: number;
}

export interface Viewport {
  width: number;
  height: number;
}

export interface PopoverOptions {
  /** Rendered width of the popover, used to keep it inside the right edge. */
  width: number;
  /** Height the popover would like; the result caps it to what actually fits. */
  preferredHeight: number;
}

/** Ready-to-apply `position: fixed` coordinates. Exactly one of `top`/`bottom`
 *  is set: the popover is pinned by the edge that touches the button, so a
 *  short list hugs the anchor instead of floating away from it. */
export interface PopoverPlacement {
  placement: 'above' | 'below';
  top: number | null;
  bottom: number | null;
  left: number;
  maxHeight: number;
}

/** Breathing room between the button and the popover. */
const GAP = 6;

/** Minimum distance kept from the window edges. */
const MARGIN = 8;

/**
 * Place a popover above its anchor, flipping below when there is not enough
 * room, and cap its height to the space actually available on the chosen side.
 *
 * Upward is preferred because the anchor buttons live on a toolbar at the
 * bottom of their pane; downward is the fallback, not the default.
 */
export function placePopover(
  anchor: AnchorRect,
  viewport: Viewport,
  opts: PopoverOptions,
): PopoverPlacement {
  const spaceAbove = anchor.top - GAP - MARGIN;
  const spaceBelow = viewport.height - anchor.bottom - GAP - MARGIN;

  const above = spaceAbove >= opts.preferredHeight || spaceAbove >= spaceBelow;
  const available = above ? spaceAbove : spaceBelow;

  // `left` is clamped from the right first, then from the left, so a viewport
  // narrower than the popover pins it to the left edge rather than off it.
  const left = Math.max(MARGIN, Math.min(anchor.left, viewport.width - opts.width - MARGIN));

  return {
    placement: above ? 'above' : 'below',
    top: above ? null : anchor.bottom + GAP,
    bottom: above ? viewport.height - anchor.top + GAP : null,
    left,
    maxHeight: Math.max(0, Math.min(opts.preferredHeight, available)),
  };
}
