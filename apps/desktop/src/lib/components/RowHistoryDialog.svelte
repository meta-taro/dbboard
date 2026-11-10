<script lang="ts">
  // One row's value at each commit that changed it, newest first (ADR-0165).
  // Each version marks the cells that differ from the version below it, so
  // reading down the list reads the row's story.
  import { onMount } from 'svelte';
  import { displayCell, rowHistory, tableKey, type TableInfo } from '$lib/api';
  import type { KeyColumn } from '$lib/grid/edit';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { keepsNoHistory, shortHash, versionRows, type RowHistory } from '$lib/versioned/view';

  interface Props {
    connectionId: string;
    database: string | null;
    table: TableInfo;
    rowKey: KeyColumn[];
    onClose: () => void;
  }
  let { connectionId, database, table, rowKey, onClose }: Props = $props();

  let history = $state<RowHistory | null>(null);
  let error = $state('');
  const rows = $derived(history ? versionRows(history) : []);

  onMount(async () => {
    try {
      history = await rowHistory(connectionId, table, rowKey, database);
    } catch (e) {
      error = String(e);
    }
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
  role="presentation"
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={i18n.t('versioned-row-history-title', { table: tableKey(table) })}
  >
    <header class="head">
      <h2 class="title">{i18n.t('versioned-row-history-title', { table: tableKey(table) })}</h2>
      <p class="key mono">
        {rowKey.map((k) => `${k.column} = ${displayCell(k.value)}`).join(', ')}
      </p>
    </header>

    <div class="body">
      {#if keepsNoHistory(error)}
        <p class="muted">{i18n.t('versioned-latest-only', { table: tableKey(table) })}</p>
      {:else if error}
        <p class="error">{error}</p>
      {:else if !history}
        <p class="muted">{i18n.t('versioned-loading')}</p>
      {:else if rows.length === 0}
        <p class="muted">{i18n.t('versioned-row-history-empty')}</p>
      {:else}
        <table class="grid">
          <thead>
            <tr>
              <th>{i18n.t('versioned-col-commit')}</th>
              {#each history.columns as col (col.name)}
                <th>{col.name}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each rows as row (row.commit.hash)}
              <tr>
                <td class="commit">
                  <span class="mono hash">{shortHash(row.commit.hash)}</span>
                  <span class="message">{row.commit.message}</span>
                  <span class="meta">{row.commit.author} · {row.commit.date}</span>
                </td>
                {#each row.cells as cell, i (i)}
                  <td
                    class="mono cell"
                    class:changed={cell.changed}
                    title={cell.before !== null
                      ? i18n.t('versioned-was', { value: cell.before })
                      : undefined}
                  >
                    {#if cell.before !== null}
                      <span class="before">{cell.before}</span>
                      <span class="arrow" aria-hidden="true">→</span>
                    {/if}
                    {cell.text}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>

    <footer class="foot">
      <button type="button" class="btn" onclick={onClose}>{i18n.t('versioned-close')}</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: 50;
  }
  .dialog {
    width: min(960px, 94vw);
    max-height: 86vh;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
  }
  .key {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .body {
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .muted,
  .error {
    margin: 0;
    padding: var(--space-3);
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
  }
  .btn {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    padding: 6px 10px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .btn:hover {
    border-color: var(--border-strong);
  }

  .grid {
    border-collapse: collapse;
    width: 100%;
    font-size: var(--text-small);
  }
  th,
  td {
    text-align: left;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
    white-space: nowrap;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--bg-surface-alt);
    font-weight: 600;
    color: var(--text-muted);
  }
  .mono {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .commit {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 14rem;
    white-space: normal;
  }
  .hash {
    color: var(--text-accent);
  }
  .message {
    font-size: var(--text-small);
  }
  .meta {
    color: var(--text-muted);
    font-size: var(--text-hint);
  }
  .cell.changed {
    background: var(--warning-weak);
  }
  .before {
    color: var(--text-muted);
    text-decoration: line-through;
  }
  .arrow {
    color: var(--text-muted);
    margin: 0 2px;
  }
</style>
