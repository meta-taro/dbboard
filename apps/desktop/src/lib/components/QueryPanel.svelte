<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import { runReadQuery, type QueryOutput } from '$lib/api';
  import ResultGrid from './ResultGrid.svelte';

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

  // Cmd/Ctrl+Enter runs, the editor convention.
  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      run();
    }
  }
</script>

<div class="panel">
  <div class="editor">
    <textarea
      bind:value={sql}
      spellcheck="false"
      rows="5"
      placeholder="SELECT …"
      onkeydown={onKeydown}
    ></textarea>
    <div class="editor-bar">
      <span class="kbd-hint">⌘/Ctrl + Enter</span>
      <button
        class="run"
        onclick={run}
        disabled={!workspace.connectionId || busy}
      >
        {busy ? 'Running…' : 'Run'}
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

  textarea {
    width: 100%;
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-body);
    line-height: 1.55;
    resize: vertical;
  }
  textarea:focus-visible {
    border-color: var(--accent);
    outline: none;
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
