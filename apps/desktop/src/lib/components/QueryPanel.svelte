<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import { runReadQuery, type QueryOutput } from '$lib/api';
  import ResultGrid from './ResultGrid.svelte';
  import SqlEditor from './SqlEditor.svelte';

  // Local to the panel: the SQL you typed and the last result persist across
  // tab switches because the shell keeps this panel mounted (display:none),
  // never unmounts it.
  let sql = $state('SELECT 1 AS hello;');
  let result = $state<QueryOutput | null>(null);
  let error = $state('');
  let busy = $state(false);

  async function run() {
    if (!workspace.connectionId || busy) return;
    busy = true;
    error = '';
    try {
      result = await runReadQuery(workspace.connectionId, sql);
    } catch (e) {
      result = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // Consume a query the sidebar context menu pushed in ("Select top 100"):
  // load it into the editor and run it, exactly once per request.
  let lastSeq = 0;
  $effect(() => {
    const req = workspace.queryRequest;
    if (req && req.seq !== lastSeq) {
      lastSeq = req.seq;
      sql = req.sql;
      run();
    }
  });
</script>

<div class="panel">
  <div class="editor">
    <SqlEditor bind:value={sql} onRun={run} />
    <div class="editor-bar">
      <span class="kbd-hint">⌘/Ctrl + Enter</span>
      <button
        class="run"
        onclick={run}
        disabled={!workspace.connectionId || busy}
      >
        {busy ? 'Running…' : '▸ Run'}
      </button>
    </div>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if result}
    <div class="result">
      <ResultGrid
        columns={result.columns}
        rows={result.rows}
        rowCount={result.row_count}
        truncated={result.truncated}
      />
    </div>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    min-height: 0;
  }

  .editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .editor-bar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-3);
  }

  .kbd-hint {
    font-size: var(--text-hint);
    color: var(--text-muted);
  }

  .run {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
    border: none;
    border-radius: var(--radius-widget);
    padding: 7px 22px;
    cursor: pointer;
  }
  .run:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }

  /* The result grid owns its own scroll/height; give it room to grow. */
  .result {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>
