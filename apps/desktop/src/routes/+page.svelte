<script lang="ts">
  import { onMount } from 'svelte';
  import {
    listConnections,
    listTables,
    runReadQuery,
    displayCell,
    type ConnectionView,
    type QueryOutput,
  } from '$lib/api';

  // The spike's whole state: which connections exist, which is selected, the
  // SQL in the box, the last result, and any error surfaced from the core.
  let connections = $state<ConnectionView[]>([]);
  let selectedId = $state('');
  let tables = $state<string[]>([]);
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
  <header>
    <h1>dbboard <span class="tag">Tauri + SvelteKit spike</span></h1>
    <p class="sub">
      Same egui-free core (McpService), new webview shell. Read-only, engine-enforced.
    </p>
  </header>

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
  /* dbboard dark palette (DESIGN.md / ADR-0056) so the spike reads as dbboard,
     not default-Svelte. A full theme system is out of scope for the spike. */
  :global(body) {
    margin: 0;
    background: #0c0e14;
    color: #e6e8f0;
    font: 14px/1.5 system-ui, sans-serif;
  }
  main {
    max-width: 1000px;
    margin: 0 auto;
    padding: 24px;
  }
  h1 {
    font-size: 20px;
    margin: 0 0 4px;
  }
  .tag {
    font-size: 11px;
    font-weight: 600;
    color: #a5b4fc;
    border: 1px solid #282c39;
    border-radius: 6px;
    padding: 2px 8px;
    vertical-align: middle;
  }
  .sub {
    margin: 0 0 20px;
    color: #8b90a3;
    font-size: 12px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    color: #8b90a3;
  }
  select,
  textarea {
    background: #171922;
    color: #e6e8f0;
    border: 1px solid #282c39;
    border-radius: 6px;
    padding: 8px;
    font-size: 13px;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    font-family: ui-monospace, "Cascadia Code", monospace;
    resize: vertical;
  }
  .badge {
    font-size: 11px;
    color: #a5b4fc;
    border: 1px solid #282c39;
    border-radius: 6px;
    padding: 2px 8px;
    align-self: end;
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
  }
  .run {
    align-self: flex-start;
    background: #6366f1;
    color: #fafbff;
    font-weight: 600;
    border: none;
    border-radius: 6px;
    padding: 8px 20px;
    cursor: pointer;
  }
  .run:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .error {
    color: #f87171;
    font-family: ui-monospace, monospace;
    font-size: 12px;
    white-space: pre-wrap;
  }
  .meta {
    color: #8b90a3;
    font-size: 12px;
    margin: 0 0 8px;
  }
  .grid-wrap {
    overflow-x: auto;
    border: 1px solid #282c39;
    border-radius: 8px;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 13px;
  }
  th,
  td {
    text-align: left;
    padding: 6px 10px;
    border-bottom: 1px solid #1e2130;
    white-space: nowrap;
  }
  th {
    background: #12141c;
    color: #a5b4fc;
    font-weight: 600;
  }
  tbody tr:nth-child(even) {
    background: #12141c;
  }
</style>
