// What matters here: a decision is sent for the head the person looked at,
// and each refusal leaves the screen where ADR-0176 says it should.
import { describe, expect, it } from 'vitest';
import { ReviewState, type ReviewBackend } from './review.svelte';
import type { PendingChange, ReviewAnswer } from './view';

const change = (branch: string, head: string, base: string | null = 'b0'): PendingChange => ({
  branch,
  head,
  base,
  authors: ['agent:support'],
  why: 'refund',
  first_at: '2026-11-12T00:00:00Z',
  last_at: '2026-11-12T00:00:00Z',
  tables: [{ table: 'player', changed: 2 }],
  main_moved: false,
  conflicts: false,
});

function backend(lists: PendingChange[][], answer: ReviewAnswer) {
  const sent: string[] = [];
  const diffs: string[] = [];
  let reads = 0;
  const b: ReviewBackend = {
    pending: async (id) => ({
      connection_id: id,
      available: true,
      changes: lists[Math.min(reads++, lists.length - 1)],
    }),
    diff: async (_id, table, from, to) => {
      diffs.push(`${table}:${from}..${to}`);
      return { from, to, columns: [], changes: [] } as never;
    },
    approve: async (_id, branch, head, reason) => {
      sent.push(`approve ${branch} ${head} ${reason}`);
      return answer;
    },
    reject: async (_id, branch, head, reason) => {
      sent.push(`reject ${branch} ${head} ${reason}`);
      return answer;
    },
  };
  return { b, sent, diffs };
}

describe('ReviewState', () => {
  it("reads a change's diff from its base to its head, table by table", async () => {
    const { b, diffs } = backend([[change('agent-a', 'h1')]], { outcome: 'rejected' });
    const s = new ReviewState(b);
    await s.load('umbx');
    await s.select(s.changes[0]);
    expect(diffs).toEqual(['player:b0..h1']);
  });

  // A branch sharing nothing with main: all of it is its change.
  it('diffs a change without a base from the empty state', async () => {
    const { b, diffs } = backend([[change('agent-a', 'h1', null)]], { outcome: 'rejected' });
    const s = new ReviewState(b);
    await s.load('umbx');
    await s.select(s.changes[0]);
    expect(diffs).toEqual(['player:-..h1']);
  });

  it('sends the head the person looked at, and clears it once approved', async () => {
    const { b, sent } = backend([[change('agent-a', 'h1')], []], {
      outcome: 'approved',
      merge: 'm1',
    });
    const s = new ReviewState(b);
    await s.load('umbx');
    await s.select(s.changes[0]);
    await s.decide('approve', 'looks right');
    expect(sent).toEqual(['approve agent-a h1 looks right']);
    expect(s.selected).toBeNull();
    expect(s.changes).toEqual([]);
    expect(s.outcome).toEqual({ kind: 'approved', merge: 'm1' });
  });

  it('shows the newer diff when the agent wrote more since', async () => {
    const { b, diffs } = backend([[change('agent-a', 'h1')], [change('agent-a', 'h2')]], {
      outcome: 'refused',
      refusal: { kind: 'moved' },
    });
    const s = new ReviewState(b);
    await s.load('umbx');
    await s.select(s.changes[0]);
    await s.decide('approve', 'ok');
    expect(s.selected?.head).toBe('h2');
    expect(diffs).toEqual(['player:b0..h1', 'player:b0..h2']);
    expect(s.outcome?.kind).toBe('refused');
  });

  it('keeps everything in place on a conflict, since nothing was written', async () => {
    const { b } = backend([[change('agent-a', 'h1')]], {
      outcome: 'refused',
      refusal: { kind: 'conflict', keys: ['player/9/gems'], more: 0 },
    });
    const s = new ReviewState(b);
    await s.load('umbx');
    await s.select(s.changes[0]);
    await s.decide('approve', 'ok');
    expect(s.selected?.head).toBe('h1');
    expect(s.outcome).toMatchObject({ kind: 'refused', refusal: { kind: 'conflict' } });
  });

  it('drops a change someone else already decided', async () => {
    const { b } = backend([[change('agent-a', 'h1')], []], {
      outcome: 'refused',
      refusal: { kind: 'decided' },
    });
    const s = new ReviewState(b);
    await s.load('umbx');
    await s.select(s.changes[0]);
    await s.decide('reject', 'no');
    expect(s.selected).toBeNull();
    expect(s.changes).toEqual([]);
  });
});
