<script lang="ts">
  import { onMount } from 'svelte';
  import {
    listConnections,
    listTables,
    runReadQuery,
    displayCell,
    type ConnectionView,
    type QueryOutput,
    type TableInfo,
  } from '$lib/api';

  // The spike's whole state: which connections exist, which is selected, the
  // SQL in the box, the last result, and any error surfaced from the core.
  let connections = $state<ConnectionView[]>([]);
  let selectedId = $state('');
  let tables = $state<TableInfo[]>([]);
  let sql = $state('SELECT 1 AS hello;');
  let result = $state<QueryOutput | null>(null);
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    try {
      connections = await listConnections();
      if (connections.length > 0) {
        selectedId = connections[0].id;
        await refreshTables();
      }
    } catch (e) {
      error = String(e);
    }
  });

  async function refreshTables() {
    tables = [];
    if (!selectedId) return;
    try {
      tables = await listTables(selectedId);
    } catch (e) {
      error = String(e);
    }
  }

  async function onConnectionChange() {
    result = null;
    error = '';
    await refreshTables();
  }

  async function run() {
    if (!selectedId || busy) return;
    busy = true;
    error = '';
    try {
      result = await runReadQuery(selectedId, sql);
    } catch (e) {
      result = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<main>
  <p class="sub">
    Same egui-free core (McpService), new webview shell. Read-only, engine-enforced.
  </p>

  <section class="bar">
    <label>
      Connection
      <select bind:value={selectedId} onchange={onConnectionChange}>
        {#if connections.length === 0}
          <option value="">(none configured)</option>
        {/if}
        {#each connections as c (c.id)}
          <option value={c.id}>{c.name} · {c.kind}</option>
        {/each}
      </select>
    </label>
    {#if tables.length > 0}
      <span class="badge">{tables.length} tables</span>
    {/if}
  </section>

  <section class="editor">
    <textarea bind:value={sql} spellcheck="false" rows="4"></textarea>
    <button class="run" onclick={run} disabled={!selectedId || busy}>
      {busy ? 'Running…' : 'Run'}
    </button>
  </section>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if result}
    <section class="result">
      <p class="meta">
        {result.row_count} rows{result.truncated ? ' (truncated)' : ''}
      </p>
      <div class="grid-wrap">
        <table>
          <thead>
            <tr>
              {#each result.columns as col (col.name)}
                <th>{col.name}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each result.rows as row, i (i)}
              <tr>
                {#each row as cell, j (j)}
                  <td>{displayCell(cell)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}
</main>

<style>
  /* Palette/spacing come from the token layer (src/lib/styles/tokens.css); this
     view only references tokens so it flips with the Auto/Light/Dark theme. */
  main {
    max-width: 1000px;
    margin: 0 auto;
    padding: var(--space-6);
  }
  .sub {
    margin: 0 0 var(--space-5);
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-hint);
    color: var(--text-muted);
  }
  select,
  textarea {
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: var(--space-2);
    font-size: var(--text-body);
  }
  select:hover,
  textarea:hover {
    border-color: var(--border-strong);
  }
  textarea {
    width: 100%;
    font-family: var(--font-mono);
    resize: vertical;
  }
  .badge {
    font-size: var(--text-hint);
    color: var(--text-accent);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 2px 8px;
    align-self: end;
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .run {
    align-self: flex-start;
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
    border: none;
    border-radius: var(--radius-widget);
    padding: var(--space-2) var(--space-5);
    cursor: pointer;
  }
  .run:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .error {
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }
  .meta {
    color: var(--text-muted);
    font-size: var(--text-small);
    margin: 0 0 var(--space-2);
  }
  .grid-wrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: var(--text-body);
  }
  th,
  td {
    text-align: left;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  th {
    background: var(--bg-code);
    color: var(--text-accent);
    font-weight: 600;
  }
  tbody tr:nth-child(even) {
    background: var(--bg-surface-alt);
  }
</style>
