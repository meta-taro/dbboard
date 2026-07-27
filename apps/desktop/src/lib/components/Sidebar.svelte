<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';

  async function onConnectionChange(e: Event) {
    const id = (e.currentTarget as HTMLSelectElement).value;
    await workspace.selectConnection(id);
  }
</script>

<aside class="sidebar">
  <div class="section">
    <label class="field">
      <span class="field-label">Connection</span>
      <select
        value={workspace.connectionId}
        onchange={onConnectionChange}
        disabled={workspace.connections.length === 0}
      >
        {#if workspace.connections.length === 0}
          <option value="">(none configured)</option>
        {/if}
        {#each workspace.connections as c (c.id)}
          <option value={c.id}>{c.name} · {c.kind}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="section tables">
    <div class="heading">
      <span class="heading-label">Tables</span>
      {#if workspace.tables.length > 0}
        <span class="badge">{workspace.tables.length}</span>
      {/if}
    </div>

    <div class="list">
      {#if workspace.loadingTables}
        <p class="hint">Loading…</p>
      {:else if workspace.tables.length === 0}
        <p class="hint">No tables</p>
      {:else}
        {#each workspace.tables as t (workspace.key(t))}
          <button
            type="button"
            class="row"
            class:selected={workspace.selectedTable &&
              workspace.key(workspace.selectedTable) === workspace.key(t)}
            onclick={() => workspace.selectTable(t)}
            title={workspace.key(t)}
          >
            {#if t.schema}<span class="schema">{t.schema}.</span>{/if}<span
              class="name">{t.name}</span
            >
          </button>
        {/each}
      {/if}
    </div>
  </div>
</aside>

<style>
  .sidebar {
    width: 260px;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-surface);
    border-right: 1px solid var(--border);
  }

  .section {
    padding: var(--space-3);
  }

  .section + .section {
    border-top: 1px solid var(--border);
  }

  /* The tables section is the one that grows and scrolls. */
  .tables {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding-bottom: var(--space-2);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .field-label {
    font-size: var(--text-hint);
    color: var(--text-muted);
  }

  select {
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 6px 8px;
    font-size: var(--text-body);
  }
  select:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  select:disabled {
    opacity: 0.6;
  }

  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .heading-label {
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .badge {
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-accent);
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 0 8px;
    min-width: 20px;
    text-align: center;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .row {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--text-body);
    padding: 5px 8px;
    border-radius: var(--radius-widget);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row:hover {
    background: var(--bg-surface-alt);
  }
  .row.selected {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--text-accent);
  }

  .schema {
    color: var(--text-muted);
  }

  .hint {
    margin: var(--space-2) 0 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
</style>
