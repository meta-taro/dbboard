// Clicking a table in the sidebar decides which tab the user lands on.
import { describe, expect, it } from 'vitest';
import { workspace } from './workspace.svelte';

const orders = { schema: null, name: 'orders' };

describe('workspace.selectTable', () => {
  it('opens the Structure tab from anywhere else', () => {
    workspace.setTab('query');
    workspace.selectTable(orders);
    expect(workspace.activeTab).toBe('structure');
    expect(workspace.selectedTable).toEqual(orders);
  });

  it('stays on the Commits tab, which reads the picked table there', () => {
    workspace.setTab('versioned');
    workspace.selectTable(orders);
    expect(workspace.activeTab).toBe('versioned');
    expect(workspace.selectedTable).toEqual(orders);
  });
});

describe('workspace.selectTable on a Redis-compatible connection', () => {
  it('opens the prefix as a bounded SCAN, since a prefix has no structure', () => {
    workspace.connections = [{ id: 'cache', name: 'cache', kind: 'redis' }];
    workspace.connectionId = 'cache';
    workspace.setTab('query');
    workspace.selectTable({ schema: null, name: 'user:*' });
    expect(workspace.activeTab).toBe('query');
    expect(workspace.queryRequest?.sql).toBe('SCAN 0 MATCH user:* COUNT 100');
    // Read-only: no table identity, so the grid offers no editing.
    expect(workspace.queryRequest?.table).toBeUndefined();
    workspace.connections = [];
    workspace.connectionId = '';
  });
});
