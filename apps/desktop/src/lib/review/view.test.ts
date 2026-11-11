import { describe, expect, it } from 'vitest';
import { changedKeys, planFor, tablePattern, type PendingChange } from './view';

describe('planFor', () => {
  it('reloads the diff when the agent wrote more since the person looked', () => {
    expect(planFor({ kind: 'moved' })).toEqual({
      messageKey: 'review-refused-moved',
      followUp: 'reload-diff',
    });
  });

  it('reloads the list when someone else decided, or nothing is waiting', () => {
    expect(planFor({ kind: 'decided' }).followUp).toBe('reload-list');
    expect(planFor({ kind: 'not_waiting' }).followUp).toBe('reload-list');
  });

  // Nothing was written, so the screen stays where it is and says why.
  it('stays put on a conflict or a token problem', () => {
    expect(planFor({ kind: 'conflict', keys: ['player/9/gems'], more: 0 }).followUp).toBe('stay');
    expect(planFor({ kind: 'no_permission', message: 'x' }).followUp).toBe('stay');
    expect(planFor({ kind: 'no_auth' }).followUp).toBe('stay');
  });

  it('gives every refusal its own message', () => {
    const keys = [
      planFor({ kind: 'moved' }),
      planFor({ kind: 'decided' }),
      planFor({ kind: 'not_waiting' }),
      planFor({ kind: 'conflict', keys: [], more: 0 }),
      planFor({ kind: 'no_permission', message: '' }),
      planFor({ kind: 'no_auth' }),
    ].map((p) => p.messageKey);
    expect(new Set(keys).size).toBe(keys.length);
  });
});

describe('helpers', () => {
  const change: PendingChange = {
    branch: 'agent-a',
    head: 'h',
    base: 'b',
    authors: ['agent:support'],
    why: 'refund',
    first_at: '2026-11-12T00:00:00Z',
    last_at: '2026-11-12T00:00:00Z',
    tables: [
      { table: 'player', changed: 2 },
      { table: 'pos', changed: 3 },
    ],
    main_moved: false,
    conflicts: false,
  };

  it('counts every key the change touches', () => {
    expect(changedKeys(change)).toBe(5);
  });

  it('reads a table by the same pattern the sidebar uses', () => {
    expect(tablePattern('player')).toBe('player/*');
  });
});
