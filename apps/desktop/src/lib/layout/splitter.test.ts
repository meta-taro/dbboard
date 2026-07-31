import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MIN_WIDTH,
  SIDEBAR_MAX_WIDTH,
  clampSidebarWidth,
  loadSidebarWidth,
  saveSidebarWidth,
  resetSidebarWidth,
} from './splitter';

describe('clampSidebarWidth (ADR-0083)', () => {
  it('keeps a width inside the range untouched', () => {
    expect(clampSidebarWidth(300, 1400)).toBe(300);
  });

  it('refuses to shrink the sidebar past its minimum', () => {
    // Dragging the divider all the way to the left edge must leave a list that
    // still shows a table name, not a sliver.
    expect(clampSidebarWidth(0, 1400)).toBe(SIDEBAR_MIN_WIDTH);
    expect(clampSidebarWidth(-500, 1400)).toBe(SIDEBAR_MIN_WIDTH);
  });

  it('refuses to grow the sidebar past its maximum', () => {
    expect(clampSidebarWidth(99_999, 4000)).toBe(SIDEBAR_MAX_WIDTH);
  });

  it('never lets the sidebar take more than half a narrow window', () => {
    // The result grid is the point of the app; on a 900px window the sidebar
    // stops at 450 even though the absolute maximum is larger.
    expect(clampSidebarWidth(800, 900)).toBe(450);
  });

  it('honours the minimum even when half the window is smaller', () => {
    // A window narrower than 2x the minimum would otherwise clamp the sidebar
    // below the minimum and produce an unusable list.
    expect(clampSidebarWidth(200, 200)).toBe(SIDEBAR_MIN_WIDTH);
  });

  it('falls back to the default for a non-finite width', () => {
    expect(clampSidebarWidth(Number.NaN, 1400)).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it('rounds to whole pixels', () => {
    expect(clampSidebarWidth(300.6, 1400)).toBe(301);
  });
});

describe('sidebar width persistence (ADR-0083)', () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });

  it('returns the default when nothing has been saved', () => {
    expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it('round-trips a saved width', () => {
    saveSidebarWidth(340);
    expect(loadSidebarWidth()).toBe(340);
  });

  it('clamps what it reads back, not just what it writes', () => {
    // A hand-edited or stale value must not be able to hide the result grid.
    localStorage.setItem('dbboard.sidebarWidth', '99999');
    expect(loadSidebarWidth()).toBe(SIDEBAR_MAX_WIDTH);
  });

  it('falls back to the default for a garbage value', () => {
    localStorage.setItem('dbboard.sidebarWidth', 'wide-please');
    expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it('forgets the saved width on reset and reports the default', () => {
    // Double-clicking the divider must not just move it back — a later launch
    // has to start at the default too.
    saveSidebarWidth(500);
    expect(resetSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  });

  it('survives a webview that refuses storage', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
      removeItem: () => {
        throw new Error('denied');
      },
    });
    expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
    expect(() => saveSidebarWidth(300)).not.toThrow();
    expect(resetSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  });
});
