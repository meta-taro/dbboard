import { describe, it, expect } from 'vitest';

import { connectionTooltip } from './label';

describe('connectionTooltip', () => {
  it('says the full name, which is the part the row cuts off', () => {
    expect(
      connectionTooltip({ id: 'c_01H8', name: 'analytics replica (eu-west)', kind: 'postgres' }),
    ).toBe('analytics replica (eu-west) — postgres');
  });

  it('never shows the id, which is the one thing nobody asked about', () => {
    const tip = connectionTooltip({ id: 'c_01H8XQ', name: 'reporting', kind: 'mysql' });
    expect(tip).not.toContain('c_01H8XQ');
  });

  it('falls back to the id when there is no name to show', () => {
    // A row can only get here through an import or a hand-edited TOML, but an
    // empty tooltip is indistinguishable from a broken one.
    expect(connectionTooltip({ id: 'c_01H8', name: '', kind: 'sqlite' })).toBe(
      'c_01H8 — sqlite',
    );
  });

  it('treats a name of nothing but spaces as no name', () => {
    expect(connectionTooltip({ id: 'c_01H8', name: '   ', kind: 'sqlite' })).toBe(
      'c_01H8 — sqlite',
    );
  });

  it('does not leave a dangling separator when the kind is missing', () => {
    expect(connectionTooltip({ id: 'c_01H8', name: 'local', kind: '' })).toBe('local');
  });

  it('trims the name rather than reproducing its padding in the tooltip', () => {
    expect(connectionTooltip({ id: 'c_01H8', name: '  staging  ', kind: 'mysql' })).toBe(
      'staging — mysql',
    );
  });
});
