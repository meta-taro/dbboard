import { describe, expect, it } from 'vitest';
import { importSummary } from './import-report';
import type { ImportReport } from '$lib/api';

// A translate stub that echoes the key and its params, so the assertions pin
// *which* message is chosen and *what* it is told — not the English wording,
// which lives in the catalog and is asserted there.
const t = (key: string, params?: Record<string, string | number>): string =>
  params ? `${key}(${Object.entries(params).map(([k, v]) => `${k}=${v}`).join(',')})` : key;

const empty: ImportReport = {
  imported: [],
  overwritten: [],
  skipped_existing: [],
  duplicate_in_bundle: [],
  refused: [],
};

describe('importSummary', () => {
  it('reports only the counts when everything imported cleanly', () => {
    const lines = importSummary({ ...empty, imported: ['a', 'b'] }, t);
    expect(lines).toEqual(['conn-import-ok(imported=2,overwritten=0,skipped=0)']);
  });

  it('names the already-present ids and offers the overwrite way out', () => {
    const lines = importSummary({ ...empty, skipped_existing: ['a', 'b'] }, t);
    expect(lines).toContain('conn-import-skipped-ids(ids=a, b)');
    expect(lines).toContain('conn-import-skipped-hint');
  });

  it('does not call a refusal "already present"', () => {
    // The whole point of ADR-0112: a refused id is *absent* from the store,
    // so the "already present" wording would be factually false for it.
    const lines = importSummary(
      {
        ...empty,
        refused: [{ id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' }],
      },
      t,
    );
    expect(lines.some((l) => l.startsWith('conn-import-skipped-ids'))).toBe(false);
    expect(lines).toContain('conn-import-refused-lead(count=1)');
  });

  it('names both sides of a refusal — the slot and the connection that owns it', () => {
    const lines = importSummary(
      {
        ...empty,
        refused: [{ id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' }],
      },
      t,
    );
    expect(lines).toContain(
      'conn-import-refused-entry(id=beta,ref=dbboard.alpha.token,owner=alpha)',
    );
  });

  it('never offers the overwrite hint for a refusal', () => {
    // Re-importing with overwrite on produces byte-identical output, so the
    // hint would send the operator round the same loop a second time.
    const lines = importSummary(
      {
        ...empty,
        refused: [{ id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' }],
      },
      t,
    );
    expect(lines).not.toContain('conn-import-skipped-hint');
  });

  it('reports a duplicate inside the file as its own reason', () => {
    const lines = importSummary({ ...empty, duplicate_in_bundle: ['dup'] }, t);
    expect(lines).toContain('conn-import-duplicate-ids(ids=dup)');
    // A duplicate is not fixed by overwriting either.
    expect(lines).not.toContain('conn-import-skipped-hint');
  });

  it('counts skips and duplicates together in the headline but keeps refusals out', () => {
    // The headline tallies what the bundle left behind for ordinary reasons.
    // A refusal gets its own count so it cannot hide inside that number.
    const lines = importSummary(
      {
        imported: ['ok'],
        overwritten: ['over'],
        skipped_existing: ['a'],
        duplicate_in_bundle: ['b'],
        refused: [{ id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' }],
      },
      t,
    );
    expect(lines[0]).toBe('conn-import-ok(imported=1,overwritten=1,skipped=2)');
    expect(lines).toContain('conn-import-refused-lead(count=1)');
  });

  it('names each refused entry separately', () => {
    const lines = importSummary(
      {
        ...empty,
        refused: [
          { id: 'b1', key_ref: 'dbboard.alpha.token', owner: 'alpha' },
          { id: 'b2', key_ref: 'dbboard.alpha.url', owner: 'alpha' },
        ],
      },
      t,
    );
    expect(lines).toContain('conn-import-refused-entry(id=b1,ref=dbboard.alpha.token,owner=alpha)');
    expect(lines).toContain('conn-import-refused-entry(id=b2,ref=dbboard.alpha.url,owner=alpha)');
  });

  it('names the overwritten ids last so the destructive outcome is not buried', () => {
    const lines = importSummary({ ...empty, overwritten: ['a'] }, t);
    expect(lines.at(-1)).toBe('conn-import-overwritten-ids(ids=a)');
  });
});
