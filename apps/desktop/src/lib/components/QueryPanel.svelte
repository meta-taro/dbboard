<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import { runReadQuery, type QueryOutput } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { queryHistory } from '$lib/history/history.svelte';
  import {
    ROW_LIMIT_OPTIONS,
    loadRowLimit,
    saveRowLimit,
    clampLimit,
  } from '$lib/query/limits';
  import ResultGrid from './ResultGrid.svelte';
  import SqlEditor from './SqlEditor.svelte';

  // Local to the panel: the SQL you typed and the last result persist across
  // tab switches because the shell keeps this panel mounted (display:none),
  // never unmounts it.
  let sql = $state('SELECT 1 AS hello;');
  let result = $state<QueryOutput | null>(null);
  let error = $state('');
  let busy = $state(false);
  // The row cap actually applied to the *displayed* result, so the grid can
  // report "capped at N" accurately even after the selector later changes.
  let resultLimit = $state<number | undefined>(undefined);

  let rowLimit = $state(loadRowLimit());
  let historyOpen = $state(false);

  // Recent queries for the active connection, most-recent-first (reactive).
  const history = $derived(
    workspace.connectionId ? queryHistory.for(workspace.connectionId) : [],
  );

  function onLimitChange(e: Event) {
    const next = clampLimit(Number((e.currentTarget as HTMLSelectElement).value));
    rowLimit = next;
    saveRowLimit(next);
  }

  async function run() {
    const connId = workspace.connectionId;
    if (!connId || busy) return;
    busy = true;
    error = '';
    const limit = rowLimit;
    try {
      result = await runReadQuery(connId, sql, limit);
      resultLimit = limit;
      queryHistory.record(connId, sql);
    } catch (e) {
      result = null;
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function loadFromHistory(entry: string) {
    sql = entry;
    historyOpen = false;
  }

  function clearHistory() {
    if (!workspace.connectionId) return;
    if (!confirm(i18n.t('history-clear-confirm'))) return;
    queryHistory.clear(workspace.connectionId);
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
      <div class="left-tools">
        <div class="history">
          <button
            type="button"
            class="chip"
            onclick={() => (historyOpen = !historyOpen)}
            disabled={!workspace.connectionId}
            aria-expanded={historyOpen}
            title={i18n.t('history-heading')}
          >
            🕘 {i18n.t('history-title', { count: history.length })}
          </button>

          {#if historyOpen}
            <div class="history-pop" role="menu">
              <div class="history-head">
                <span class="history-title">{i18n.t('history-heading')}</span>
                {#if history.length > 0}
                  <button type="button" class="linkish" onclick={clearHistory}>
                    {i18n.t('history-clear')}
                  </button>
                {/if}
              </div>
              {#if history.length === 0}
                <p class="history-empty">{i18n.t('history-empty')}</p>
              {:else}
                <ul class="history-list">
                  {#each history as entry (entry.at + entry.sql)}
                    <li>
                      <button
                        type="button"
                        class="history-item"
                        onclick={() => loadFromHistory(entry.sql)}
                        title={entry.sql}
                      >
                        {entry.sql}
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        </div>
      </div>

      <div class="right-tools">
        <label class="limit">
          <span class="limit-label">{i18n.t('result-row-limit')}</span>
          <select value={rowLimit} onchange={onLimitChange}>
            {#each ROW_LIMIT_OPTIONS as opt (opt)}
              <option value={opt}>{opt}</option>
            {/each}
          </select>
        </label>
        <span class="kbd-hint">⌘/Ctrl + Enter</span>
        <button
          class="run"
          onclick={run}
          disabled={!workspace.connectionId || busy}
        >
          {busy ? i18n.t('query-run-busy') : `▸ ${i18n.t('sql-run-button')}`}
        </button>
      </div>
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
        limit={resultLimit}
      />
    </div>
  {/if}
</div>

<!-- Click-away closes the history popover. -->
<svelte:window
  onclick={(e) => {
    if (historyOpen && !(e.target as HTMLElement).closest('.history')) {
      historyOpen = false;
    }
  }}
/>

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
    justify-content: space-between;
    gap: var(--space-3);
  }
  .right-tools {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .history {
    position: relative;
  }
  .chip {
    background: var(--bg-surface);
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 5px 12px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .chip:hover:not(:disabled) {
    border-color: var(--border-strong);
    color: var(--text);
  }
  .chip:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* Popover floats above the editor-bar, opening upward from the button. */
  .history-pop {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    width: min(460px, 60vw);
    max-height: 320px;
    display: flex;
    flex-direction: column;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
    z-index: 20;
  }
  .history-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .history-title {
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .linkish {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
    padding: 0;
  }
  .linkish:hover {
    color: var(--danger);
  }
  .history-empty {
    margin: 0;
    padding: var(--space-3);
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .history-list {
    list-style: none;
    margin: 0;
    padding: 4px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .history-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    padding: 6px 8px;
    border-radius: var(--radius-widget);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .history-item:hover {
    background: var(--bg-surface-alt);
  }

  .limit {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .limit-label {
    font-size: var(--text-hint);
    color: var(--text-muted);
  }
  .limit select {
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 4px 8px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .limit select:hover {
    border-color: var(--border-strong);
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
