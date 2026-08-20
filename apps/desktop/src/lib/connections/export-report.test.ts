import { describe, expect, it } from 'vitest';
import { exportSummary } from './export-report';
import type { ExportReport } from '$lib/api';

// A translate stub that echoes the key and its params, so the assertions pin
// *which* message is chosen and *what* it is told — not the English wording,
// which lives in the catalog and is asserted there.
const t = (key: string, params?: Record<string, string | number>): string =>
  params ? `${key}(${Object.entries(params).map(([k, v]) => `${k}=${v}`).join(',')})` : key;

describe('exportSummary', () => {
  it('reports the count alone for a clean store', () => {
    const lines = exportSummary({ exported: 3, foreign_refs: [] }, t);
    expect(lines).toEqual(['conn-export-ok(count=3)']);
  });

  it('still leads with the success when a slot is foreign', () => {
    // Export warns, it does not refuse. If the warning displaced the success
    // line, an operator who most needs the backup would read this as "the
    // export failed" and go looking for a file that is already on disk.
    const lines = exportSummary(
      {
        exported: 2,
        foreign_refs: [{ id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' }],
      },
      t,
    );
    expect(lines[0]).toBe('conn-export-ok(count=2)');
  });

  it('names both sides — the entry, the slot, and the connection that owns it', () => {
    const lines = exportSummary(
      {
        exported: 2,
        foreign_refs: [{ id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' }],
      },
      t,
    );
    expect(lines).toContain('conn-export-foreign-lead(count=1)');
    expect(lines).toContain(
      'conn-export-foreign-entry(id=beta,ref=dbboard.alpha.token,owner=alpha)',
    );
  });

  it('gives every offending entry its own line', () => {
    const lines = exportSummary(
      {
        exported: 3,
        foreign_refs: [
          { id: 'beta', key_ref: 'dbboard.alpha.token', owner: 'alpha' },
          { id: 'gamma', key_ref: 'dbboard.alpha.url', owner: 'alpha' },
        ],
      },
      t,
    );
    expect(lines).toHaveLength(4);
    expect(lines.filter((l) => l.startsWith('conn-export-foreign-entry'))).toHaveLength(2);
  });
});
