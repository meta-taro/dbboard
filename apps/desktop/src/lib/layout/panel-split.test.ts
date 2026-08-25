import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  CONNECTION_ROW_HEIGHT,
  CONNECTIONS_MIN_HEIGHT,
  CONNECTIONS_MAX_HEIGHT,
  defaultConnectionsHeight,
  clampConnectionsHeight,
  resolveConnectionsHeight,
  loadConnectionsHeight,
  saveConnectionsHeight,
  resetConnectionsHeight,
} from './panel-split';

// A tall sidebar, so the viewport share is not what is under test.
const TALL = 1200;

describe('defaultConnectionsHeight (ADR-0131)', () => {
  it('gives a short list exactly the room its rows need', () => {
    // Three connections must not leave a gap of empty list above the tables,
    // and must not need scrolling either.
    expect(defaultConnectionsHeight(3, TALL)).toBe(3 * CONNECTION_ROW_HEIGHT);
  });

  it('holds the minimum open for an empty or nearly empty list', () => {
    // Zero connections still shows the "add one" hint, and one connection
    // must not produce a pane too short to drop a second row into.
    expect(defaultConnectionsHeight(0, TALL)).toBe(CONNECTIONS_MIN_HEIGHT);
    expect(defaultConnectionsHeight(1, TALL)).toBe(CONNECTIONS_MIN_HEIGHT);
  });

  it('stops growing long before a long list eats the tables', () => {
    expect(defaultConnectionsHeight(200, TALL)).toBe(CONNECTIONS_MAX_HEIGHT);
  });

  it('takes at most half a short sidebar, whatever the count', () => {
    // On a laptop-height window the table list is the one that has to stay
    // usable: it is the list being scrolled all day.
    expect(defaultConnectionsHeight(40, 400)).toBe(200);
  });

  it('honours the minimum even when half the sidebar is smaller', () => {
    expect(defaultConnectionsHeight(40, 80)).toBe(CONNECTIONS_MIN_HEIGHT);
  });
});

describe('clampConnectionsHeight (ADR-0131)', () => {
  it('keeps a height inside the range untouched', () => {
    expect(clampConnectionsHeight(180, TALL)).toBe(180);
  });

  it('refuses to collapse the pane to nothing', () => {
    expect(clampConnectionsHeight(0, TALL)).toBe(CONNECTIONS_MIN_HEIGHT);
    expect(clampConnectionsHeight(-400, TALL)).toBe(CONNECTIONS_MIN_HEIGHT);
  });

  it('refuses to grow past the absolute maximum', () => {
    expect(clampConnectionsHeight(99_999, 4000)).toBe(CONNECTIONS_MAX_HEIGHT);
  });

  it('falls back to the minimum for a non-finite height', () => {
    expect(clampConnectionsHeight(Number.NaN, TALL)).toBe(CONNECTIONS_MIN_HEIGHT);
  });

  it('rounds to whole pixels', () => {
    expect(clampConnectionsHeight(180.6, TALL)).toBe(181);
  });
});

describe('resolveConnectionsHeight (ADR-0131)', () => {
  it('follows the connection count while the divider is untouched', () => {
    // The point of storing null rather than the default: adding a fourth
    // connection to an untouched sidebar makes room for it by itself.
    expect(resolveConnectionsHeight(null, 3, TALL)).toBe(3 * CONNECTION_ROW_HEIGHT);
    expect(resolveConnectionsHeight(null, 4, TALL)).toBe(4 * CONNECTION_ROW_HEIGHT);
  });

  it('stops following the count once the divider has been dragged', () => {
    expect(resolveConnectionsHeight(180, 3, TALL)).toBe(180);
    expect(resolveConnectionsHeight(180, 40, TALL)).toBe(180);
  });

  it('clamps a chosen height against the sidebar it is being shown in', () => {
    // Shrinking the window must squeeze a remembered height, not overflow it.
    expect(resolveConnectionsHeight(380, 3, 400)).toBe(200);
  });
});

describe('connections height persistence (ADR-0131)', () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });

  it('reports nothing saved as null, not as a height', () => {
    // null is what lets the pane keep following the connection count.
    expect(loadConnectionsHeight()).toBeNull();
  });

  it('round-trips a saved height', () => {
    saveConnectionsHeight(180);
    expect(loadConnectionsHeight()).toBe(180);
  });

  it('clamps what it reads back against the absolute range', () => {
    localStorage.setItem('dbboard.connectionsHeight', '99999');
    expect(loadConnectionsHeight()).toBe(CONNECTIONS_MAX_HEIGHT);
  });

  it('treats a garbage value as nothing saved', () => {
    localStorage.setItem('dbboard.connectionsHeight', 'tall-please');
    expect(loadConnectionsHeight()).toBeNull();
  });

  it('forgets the saved height on reset', () => {
    // Double-clicking the divider hands the pane back to the connection
    // count, and a later launch has to start there too.
    saveConnectionsHeight(300);
    resetConnectionsHeight();
    expect(loadConnectionsHeight()).toBeNull();
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
    expect(loadConnectionsHeight()).toBeNull();
    expect(() => saveConnectionsHeight(180)).not.toThrow();
    expect(() => resetConnectionsHeight()).not.toThrow();
  });
});
