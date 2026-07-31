<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import {
    runReadQuery,
    describeTable,
    configPath,
    type QueryOutput,
    type TableInfo,
  } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { queryHistory } from '$lib/history/history.svelte';
  import {
    ROW_LIMIT_OPTIONS,
    loadRowLimit,
    saveRowLimit,
    clampLimit,
  } from '$lib/query/limits';
  import { placePopover, type PopoverPlacement } from '$lib/layout/popover';
  import ResultGrid from './ResultGrid.svelte';
  import SqlEditor from './SqlEditor.svelte';

  // Local to the panel: the SQL you typed and the last result persist across
  // tab switches because the shell keeps this panel mounted (display:none),
  // never unmounts it.
  let sql = $state('SELECT 1 AS hello;');
  let editor = $state<SqlEditor | undefined>(undefined);

  // Every write to `sql` that did not come from the keyboard has to reach the
  // editor explicitly; the editor only reads `value` when it builds its view.
  function setSql(next: string) {
    sql = next;
    editor?.setDoc(next);
  }

  let result = $state<QueryOutput | null>(null);
  let error = $state('');
  let busy = $state(false);
  // The row cap actually applied to the *displayed* result, so the grid can
  // report "capped at N" accurately even after the selector later changes.
  let resultLimit = $state<number | undefined>(undefined);

  let rowLimit = $state(loadRowLimit());
  let historyOpen = $state(false);

  // The history popover is positioned with `position: fixed` and explicit
  // coordinates (ADR-0083): the tab pane scrolls, and an absolutely-positioned
  // popover was being clipped by it — upward from a toolbar near the top of the
  // window, the newest entries fell off the screen entirely.
  const HISTORY_POP_HEIGHT = 320;
  const HISTORY_POP_WIDTH = 460;

  let historyBtn = $state<HTMLButtonElement | null>(null);
  let historyPop = $state<PopoverPlacement | null>(null);
  let historyWidth = $state(HISTORY_POP_WIDTH);

  function placeHistory() {
    if (!historyBtn) return;
    historyWidth = Math.min(HISTORY_POP_WIDTH, window.innerWidth * 0.6);
    historyPop = placePopover(
      historyBtn.getBoundingClientRect(),
      { width: window.innerWidth, height: window.innerHeight },
      { width: historyWidth, preferredHeight: HISTORY_POP_HEIGHT },
    );
  }

  function toggleHistory() {
    historyOpen = !historyOpen;
    if (historyOpen) placeHistory();
  }

  const historyStyle = $derived(
    historyPop === null
      ? ''
      : `left:${historyPop.left}px;` +
          (historyPop.top === null
            ? `bottom:${historyPop.bottom}px;`
            : `top:${historyPop.top}px;`) +
          `width:${historyWidth}px;max-height:${historyPop.maxHeight}px`,
  );

  // Editable-browse context. When the current result came from a sidebar
  // "Select top 100" (a known table, not an arbitrary query), we load its
  // primary key so the grid can offer inline cell editing (ADR-0042). A
  // manual Run or a history replay clears this — only a browse is editable.
  let editTable = $state<TableInfo | null>(null);
  let editPk = $state<string[]>([]);
  // A browsed table that has NO declared primary key: editable-intent but not
  // safely keyable, so we show a read-only reason instead of edit affordances.
  const noPk = $derived(editTable !== null && editPk.length === 0);
  const editContext = $derived(
    workspace.connectionId && editTable && editPk.length > 0
      ? { connectionId: workspace.connectionId, table: editTable, pk: editPk }
      : null,
  );

  // First-run guidance: with no connection configured the Run button can only
  // stay disabled, so we explain *why* and *where* to register one. The path
  // is resolved lazily — only fetched while the empty state is actually shown.
  let configFilePath = $state('');
  $effect(() => {
    if (workspace.connections.length === 0 && !configFilePath) {
      configPath()
        .then((p) => (configFilePath = p))
        .catch(() => {
          // Outside a Tauri runtime the command is unavailable; the empty
          // state still explains the situation without the concrete path.
        });
    }
  });

  // Recent queries for the active connection, most-recent-first (reactive).
  const history = $derived(
    workspace.connectionId ? queryHistory.for(workspace.connectionId) : [],
  );

  function onLimitChange(e: Event) {
    const next = clampLimit(Number((e.currentTarget as HTMLSelectElement).value));
    rowLimit = next;
    saveRowLimit(next);
  }

  // Run the current SQL. `table` marks an editable browse of that table (from
  // the sidebar); `null` is an arbitrary query, which is never editable. On a
  // browse we also load the table's primary key so the grid can key its edits.
  async function execute(table: TableInfo | null) {
    const connId = workspace.connectionId;
    if (!connId || busy) return;
    busy = true;
    error = '';
    editTable = table;
    editPk = [];
    const limit = rowLimit;
    try {
      result = await runReadQuery(connId, sql, limit);
      resultLimit = limit;
      queryHistory.record(connId, sql);
      if (table) {
        // A failed schema read just leaves the table read-only (empty PK); the
        // rows still show. Never let it mask a successful query.
        try {
          const schema = await describeTable(connId, table.name, table.schema);
          editPk = schema.primary_key;
        } catch {
          editPk = [];
        }
      }
    } catch (e) {
      result = null;
      error = String(e);
      editTable = null;
    } finally {
      busy = false;
    }
  }

  // The Run button and history replay run arbitrary SQL — never editable.
  function run() {
    execute(null);
  }

  // Re-run the same browse after a committed save so the grid reflects what
  // actually landed (a typed/triggered column may differ from the text sent).
  function reloadAfterSave() {
    execute(editTable);
  }

  function loadFromHistory(entry: string) {
    setSql(entry);
    historyOpen = false;
  }

  function clearHistory() {
    if (!workspace.connectionId) return;
    if (!confirm(i18n.t('history-clear-confirm'))) return;
    queryHistory.clear(workspace.connectionId);
  }

  // Consume a query the sidebar context menu pushed in ("Select top 100"):
  // load it into the editor and run it, exactly once per request. A request
  // carrying a `table` is an editable browse; otherwise it is arbitrary SQL.
  let lastSeq = 0;
  $effect(() => {
    const req = workspace.queryRequest;
    if (req && req.seq !== lastSeq) {
      lastSeq = req.seq;
      setSql(req.sql);
      execute(req.table ?? null);
    }
  });
</script>

<div class="panel">
  {#if workspace.connections.length === 0}
    <div class="empty" role="note">
      <h2 class="empty-title">{i18n.t('empty-no-connection-title')}</h2>
      <p class="empty-body">{i18n.t('empty-no-connection-body')}</p>
      <div class="empty-path">
        <span class="empty-path-label">{i18n.t('empty-config-path-label')}</span>
        <code class="empty-path-value"
          >{configFilePath || i18n.t('empty-config-path-loading')}</code
        >
      </div>
    </div>
  {/if}

  <div class="editor">
    <SqlEditor bind:this={editor} bind:value={sql} onRun={run} />
    <div class="editor-bar">
      <div class="left-tools">
        <div class="history">
          <button
            type="button"
            class="chip"
            bind:this={historyBtn}
            onclick={toggleHistory}
            disabled={!workspace.connectionId}
            aria-expanded={historyOpen}
            title={i18n.t('history-heading')}
          >
            🕘 {i18n.t('history-title', { count: history.length })}
          </button>

          {#if historyOpen}
            <div class="history-pop" role="menu" style={historyStyle}>
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
      {#if noPk}
        <p class="readonly-note" role="note">{i18n.t('edit-readonly-no-pk')}</p>
      {/if}
      <ResultGrid
        columns={result.columns}
        rows={result.rows}
        rowCount={result.row_count}
        truncated={result.truncated}
        limit={resultLimit}
        edit={editContext}
        onSaved={reloadAfterSave}
      />
    </div>
  {/if}
</div>

<!-- Click-away closes the history popover; a resize re-places it, since its
     fixed coordinates were measured against the old window. -->
<svelte:window
  onclick={(e) => {
    if (historyOpen && !(e.target as HTMLElement).closest('.history')) {
      historyOpen = false;
    }
  }}
  onresize={() => historyOpen && placeHistory()}
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

  /* First-run empty state: quiet, explanatory card above the (inert) editor. */
  .empty {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
  }
  .empty-title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    color: var(--text);
  }
  .empty-body {
    margin: 0;
    max-width: 68ch;
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.6;
  }
  .empty-path {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: var(--space-1);
  }
  .empty-path-label {
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .empty-path-value {
    font-family: var(--font-mono);
    font-size: var(--text-small);
    color: var(--text-accent);
    user-select: all;
    word-break: break-all;
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

  /* Fixed, not absolute: the tab pane scrolls and would otherwise clip the
     popover. Position, width and height all come from `placePopover`
     (ADR-0083), which flips it below the button when there is no room above. */
  .history-pop {
    position: fixed;
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
    gap: var(--space-2);
    min-height: 0;
  }

  /* Why a browsed table isn't editable — a table with no declared primary key
     has no safe row key, so it stays read-only (parity with the egui editor). */
  .readonly-note {
    margin: 0;
    padding: 6px 10px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    color: var(--text-muted);
    font-size: var(--text-hint);
  }
</style>
