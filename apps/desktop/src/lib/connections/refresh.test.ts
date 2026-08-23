import { describe, expect, it, vi } from 'vitest';
import { refreshConnectionList } from './refresh';

// What a mutating action has to do afterwards, and the reason it is a named
// function with a test rather than two lines inlined at five call sites.
//
// It was inlined at two, then factored out to a local helper when duplicate
// and repair added three more. The `workspace.` prefix was lost in the move,
// which left `refreshAll` calling `refreshAll` — a loop with no base case,
// awaited before anything else, so it never overflowed the stack and never
// returned either. Every save, delete, duplicate, repair and import spun the
// microtask queue until the webview stopped answering, with `busy` still set
// and the `finally` that clears it unreachable.
//
// Nothing caught it: the recursion typechecks, and a component this size has
// no render test. So the coordination lives here, where it can be called.

describe('refreshConnectionList', () => {
  it('refreshes the connections and then their badges', async () => {
    const order: string[] = [];
    const connections = vi.fn(async () => void order.push('connections'));
    const foreignRefs = vi.fn(async () => void order.push('foreignRefs'));

    await refreshConnectionList({ connections, foreignRefs });

    expect(connections).toHaveBeenCalledTimes(1);
    expect(foreignRefs).toHaveBeenCalledTimes(1);
    // Connections first: the badges annotate rows, so refreshing them against
    // a list that is about to be replaced would show the previous answer.
    expect(order).toEqual(['connections', 'foreignRefs']);
  });

  it('returns, rather than calling itself', async () => {
    // The regression, stated as the thing it actually broke: a caller that
    // awaits this must get control back. A self-call would fail here by
    // timing out instead of by asserting, which is why the timeout is short —
    // the failure should be quick to see, not a hung suite.
    const done = await Promise.race([
      refreshConnectionList({
        connections: async () => {},
        foreignRefs: async () => {},
      }).then(() => 'returned'),
      new Promise((r) => setTimeout(() => r('still going'), 300)),
    ]);

    expect(done).toBe('returned');
  }, 2000);

  it('does not refresh the badges when the list itself failed', async () => {
    // Badges read from a separate command. Running it against a list that did
    // not load would decorate stale rows, and the error the caller is about to
    // show would be contradicted by a panel that looks freshly updated.
    const foreignRefs = vi.fn(async () => {});

    await expect(
      refreshConnectionList({
        connections: async () => {
          throw new Error('listing failed');
        },
        foreignRefs,
      }),
    ).rejects.toThrow('listing failed');

    expect(foreignRefs).not.toHaveBeenCalled();
  });
});
