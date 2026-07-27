<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import { searchSchema, type SchemaMatch, type TableInfo } from '$lib/api';

  let query = $state('');
  // null = not searching (show the full table list); an array = search results.
  let matches = $state<SchemaMatch[] | null>(null);
  let searching = $state(false);
  let searchError = $state('');
  let seq = 0;

  async function onConnectionChange(e: Event) {
    const id = (e.currentTarget as HTMLSelectElement).value;
    query = '';
    matches = null;
    await workspace.selectConnection(id);
  }

  // Debounced schema search. Blank query shows the plain list (the backend
  // rejects a blank pattern anyway); a token drops stale responses.
  $effect(() => {
    const q = query.trim();
    const connId = workspace.connectionId;
    if (!q || !connId) {
      matches = null;
      searchError = '';
      return;
    }
    const mine = ++seq;
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const view = await searchSchema(connId, q);
        if (mine !== seq) return;
        matches = view.matches;
        searchError = '';
      } catch (err) {
        if (mine !== seq) return;
        matches = [];
        searchError = String(err);
      } finally {
        if (mine === seq) searching = false;
      }
    }, 220);
    return () => clearTimeout(timer);
  });

  function isSelected(t: TableInfo): boolean {
    return (
      !!workspace.selectedTable &&
      workspace.key(workspace.selectedTable) === workspace.key(t)
    );
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
    <div class="search">
      <input
        type="search"
        placeholder="Search tables &amp; columns…"
        bind:value={query}
        disabled={!workspace.connectionId}
        spellcheck="false"
      />
    </div>

    {#if matches === null}
      <!-- Normal browse mode: the full table list. -->
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
              class:selected={isSelected(t)}
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
    {:else}
      <!-- Search mode. -->
      <div class="heading">
        <span class="heading-label">Matches</span>
        {#if !searching}<span class="badge">{matches.length}</span>{/if}
      </div>
      <div class="list">
        {#if searching}
          <p class="hint">Searching…</p>
        {:else if searchError}
          <p class="error">{searchError}</p>
        {:else if matches.length === 0}
          <p class="hint">No matches</p>
        {:else}
          {#each matches as m (workspace.key(m.table))}
            <button
              type="button"
              class="row match"
              class:selected={isSelected(m.table)}
              onclick={() => workspace.selectTable(m.table)}
              title={workspace.key(m.table)}
            >
              <span class="match-name">
                {#if m.table.schema}<span class="schema"
                    >{m.table.schema}.</span
                  >{/if}{m.table.name}
                {#if !m.table_name_matched}<span class="via">col</span>{/if}
              </span>
              {#if m.matched_columns.length > 0}
                <span class="cols">
                  {#each m.matched_columns as c (c.name)}
                    <span class="col-chip">{c.name}</span>
                  {/each}
                </span>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    {/if}
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

  select,
  input[type='search'] {
    width: 100%;
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 6px 8px;
    font-size: var(--text-body);
  }
  select:hover:not(:disabled),
  input[type='search']:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  select:disabled,
  input[type='search']:disabled {
    opacity: 0.6;
  }
  input[type='search']:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  .search {
    margin-bottom: var(--space-3);
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

  .match {
    white-space: normal;
  }
  .match-name {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .via {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 0 4px;
  }
  .cols {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 4px;
  }
  .col-chip {
    font-size: var(--text-hint);
    font-family: var(--font-mono);
    color: var(--text-muted);
    background: var(--bg-surface-alt);
    border-radius: var(--radius-widget);
    padding: 0 6px;
  }

  .hint,
  .error {
    margin: var(--space-2) 0 0;
    font-size: var(--text-small);
  }
  .hint {
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
    font-family: var(--font-mono);
    white-space: pre-wrap;
  }
</style>
