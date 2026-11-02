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
