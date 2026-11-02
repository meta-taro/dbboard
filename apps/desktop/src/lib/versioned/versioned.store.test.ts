// The store decides whether the History tab exists for the connection in
// view. What matters: a slow answer for a connection the user has already
// left must not switch the tab on for the one they are looking at now.
import { describe, expect, it } from 'vitest';
import { VersionedState } from './versioned.svelte';
import type { CommitLogView } from './view';

const view = (id: string, available: boolean): CommitLogView => ({
  connection_id: id,
  available,
  commits: available
    ? [{ hash: 'c1', author: 'A', email: 'a@example.com', date: 'd', message: 'm' }]
    : [],
});

/** A fetch whose answers are released by hand, in any order. */
function deferred() {
  const pending = new Map<string, (v: CommitLogView) => void>();
  const fetch = (id: string) =>
    new Promise<CommitLogView>((resolve) => pending.set(id, resolve));
  return { fetch, release: (id: string, v: CommitLogView) => pending.get(id)?.(v) };
}

describe('VersionedState.load', () => {
  it('is available with its commits for a connection that keeps history', async () => {
    const state = new VersionedState(async (id) => view(id, true));
    await state.load('dolt', null);
    expect(state.available).toBe(true);
    expect(state.commits.map((c) => c.hash)).toEqual(['c1']);
  });

  it('is unavailable, and quiet, for one that does not', async () => {
    const state = new VersionedState(async (id) => view(id, false));
    await state.load('pg', null);
    expect(state.available).toBe(false);
    expect(state.error).toBe('');
  });

  it('treats a failed check as no history rather than an error on screen', async () => {
    // ADR-0165: detection failing means "plain", never a banner. The tab is
    // simply not offered.
    const state = new VersionedState(async () => {
      throw new Error('connection refused');
    });
    await state.load('down', null);
    expect(state.available).toBe(false);
    expect(state.error).toBe('');
  });

  it('ignores a late answer for a connection the user has left', async () => {
    const d = deferred();
    const state = new VersionedState(d.fetch);
    const first = state.load('dolt', null);
    const second = state.load('pg', null);
    d.release('pg', view('pg', false));
    await second;
    d.release('dolt', view('dolt', true));
    await first;
    expect(state.available).toBe(false);
  });

  it('forgets the previous connection as soon as another is asked for', async () => {
    const d = deferred();
    const state = new VersionedState(d.fetch);
    const first = state.load('dolt', null);
    d.release('dolt', view('dolt', true));
    await first;
    void state.load('pg', null);
    // Until pg answers, dolt's commits must not be drawn under pg's name.
    expect(state.available).toBe(false);
    expect(state.commits).toEqual([]);
  });
});
