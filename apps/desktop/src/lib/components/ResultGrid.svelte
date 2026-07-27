<script lang="ts">
  import { save } from '@tauri-apps/plugin-dialog';
  import { displayCell, saveTextFile, type Cell, type Column } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import {
    nextSortKeys,
    sortedIndices,
    toDelimited,
    toDelimitedFile,
    type SortKey,
  } from '$lib/grid/format';

  interface Props {
    columns: Column[];
    rows: Cell[][];
    rowCount: number;
    truncated: boolean;
    // The row cap that was applied for this run, so a truncated result can say
    // exactly where it stopped ("capped at 1000") instead of a vague suffix.
    limit?: number;
  }
  let { columns, rows, rowCount, truncated, limit }: Props = $props();

  let sortKeys = $state<SortKey[]>([]);
  // Selection is keyed by ORIGINAL row index so it survives re-sorting.
  let selected = $state<Set<number>>(new Set());
  let anchor = $state<number | null>(null); // last-clicked display position
  let copied = $state('');
  let popup = $state<{ col: string; value: string } | null>(null);

  // Reset transient state when a new result arrives.
  $effect(() => {
    rows; // track
    sortKeys = [];
    selected = new Set();
    anchor = null;
    popup = null;
  });

  const displayOrder = $derived(sortedIndices(rows, sortKeys));

  function sortIndicator(col: number): string {
    const pos = sortKeys.findIndex((k) => k.col === col);
    if (pos === -1) return '';
    const arrow = sortKeys[pos].dir === 'asc' ? '↑' : '↓';
    // Show the priority number only when more than one key is active.
    return sortKeys.length > 1 ? `${arrow}${pos + 1}` : arrow;
  }

  function onHeaderClick(col: number, e: MouseEvent) {
    sortKeys = nextSortKeys(sortKeys, col, e.shiftKey);
  }

  function onRowClick(displayPos: number, e: MouseEvent) {
    const origIdx = displayOrder[displayPos];
    const next = new Set(selected);

    if (e.shiftKey && anchor !== null) {
      const [lo, hi] = [anchor, displayPos].sort((a, b) => a - b);
      for (let p = lo; p <= hi; p++) next.add(displayOrder[p]);
    } else if (e.ctrlKey || e.metaKey) {
      next.has(origIdx) ? next.delete(origIdx) : next.add(origIdx);
      anchor = displayPos;
    } else {
      next.clear();
      next.add(origIdx);
      anchor = displayPos;
    }
    selected = next;
  }

  // Rows to export: the selection (in display order) if any, else everything.
  function rowsForExport(): Cell[][] {
    const idxs =
      selected.size > 0
        ? displayOrder.filter((i) => selected.has(i))
        : displayOrder;
    return idxs.map((i) => rows[i]);
  }

  async function copy(sep: ',' | '\t') {
    const text = toDelimited(columns, rowsForExport(), sep);
    try {
      await navigator.clipboard.writeText(text);
      flash(i18n.t('result-copied', { fmt: sep === ',' ? 'CSV' : 'TSV' }));
    } catch {
      flash(i18n.t('result-copy-failed'));
    }
  }

  // Save the export (selection or all rows) to a file the user names in the
  // native "Save As" dialog. The chosen extension picks the delimiter (.tsv →
  // tab, anything else → comma); the file always carries a UTF-8 BOM so Excel
  // on a non-UTF-8 code page opens it without mojibake (ADR-0035). Cancelling
  // the dialog is a silent no-op.
  async function saveFile() {
    const path = await save({
      defaultPath: 'dbboard-result.csv',
      filters: [
        { name: 'CSV', extensions: ['csv'] },
        { name: 'TSV', extensions: ['tsv'] },
      ],
    });
    if (!path) return;
    const sep = path.toLowerCase().endsWith('.tsv') ? '\t' : ',';
    try {
      await saveTextFile(path, toDelimitedFile(columns, rowsForExport(), sep));
      flash(i18n.t('result-saved', { name: baseName(path) }));
    } catch {
      flash(i18n.t('result-save-failed'));
    }
  }

  // Last path segment for the success toast — the dialog returns an absolute
  // path, and only the file name is worth surfacing.
  function baseName(path: string): string {
    return path.split(/[\\/]/).pop() ?? path;
  }

  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  function flash(msg: string) {
    copied = msg;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (copied = ''), 1600);
  }

  function openCell(col: string, cell: Cell) {
    // Only worth a popup for real, non-trivial text.
    const value = displayCell(cell);
    if (cell === null || value.length < 40) return;
    popup = { col, value };
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') popup = null;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="wrap">
  <div class="toolbar">
    <span class="count">
      {#if selected.size > 0}
        {i18n.t('result-selected', { sel: selected.size, total: rowCount })}
      {:else}
        {i18n.t('result-rows', { count: rowCount })}{truncated
          ? ` (${i18n.t('result-truncated-suffix', { max: limit ?? rowCount })})`
          : ''}
      {/if}
    </span>

    <div class="tools">
      {#if copied}<span class="flash">{copied}</span>{/if}
      <button type="button" onclick={() => copy('\t')} title={i18n.t('result-copy-tsv-title')}>
        {i18n.t('result-copy-tsv')}
      </button>
      <button type="button" onclick={() => copy(',')} title={i18n.t('result-copy-csv-title')}>
        {i18n.t('result-copy-csv')}
      </button>
      <button type="button" class="ghost" onclick={saveFile} title={i18n.t('result-save-title')}>
        {i18n.t('result-save')}
      </button>
    </div>
  </div>

  <div class="grid-wrap">
    <table>
      <thead>
        <tr>
          {#each columns as col, ci (col.name)}
            <th
              class:sorted={sortKeys.some((k) => k.col === ci)}
              onclick={(e) => onHeaderClick(ci, e)}
              title={i18n.t('result-sort-hint')}
            >
              <span class="th-inner">
                <span class="th-name">{col.name}</span>
                <span class="th-sort">{sortIndicator(ci)}</span>
              </span>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each displayOrder as origIdx, displayPos (origIdx)}
          <tr
            class:selected={selected.has(origIdx)}
            onclick={(e) => onRowClick(displayPos, e)}
          >
            {#each rows[origIdx] as cell, ci (ci)}
              <td
                class:null-cell={cell === null}
                class:num-cell={typeof cell === 'number'}
                title={cell === null ? 'NULL' : displayCell(cell)}
                ondblclick={() => openCell(columns[ci].name, cell)}
              >
                {displayCell(cell)}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>

{#if popup}
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) popup = null;
    }}
    role="presentation"
  >
    <div class="popup" role="dialog" aria-modal="true" aria-label={i18n.t('result-cell-dialog')} tabindex="-1">
      <div class="popup-head">
        <span class="popup-col">{popup.col}</span>
        <button
          type="button"
          class="ghost"
          onclick={() => navigator.clipboard.writeText(popup?.value ?? '')}
        >
          {i18n.t('cell-copy')}
        </button>
      </div>
      <pre class="popup-body">{popup.value}</pre>
    </div>
  </div>
{/if}

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    min-height: 0;
    gap: var(--space-2);
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .count {
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .tools button {
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 4px 10px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .tools button:hover {
    border-color: var(--border-strong);
    background: var(--bg-surface-alt);
  }
  .tools .ghost {
    color: var(--text-muted);
  }
  .flash {
    font-size: var(--text-hint);
    color: var(--success);
  }

  .grid-wrap {
    overflow: auto;
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
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--bg-code);
    color: var(--text-accent);
    font-weight: 600;
    cursor: pointer;
    user-select: none;
  }
  th:hover {
    background: var(--bg-surface-alt);
  }
  th.sorted {
    color: var(--text-accent);
  }
  .th-inner {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .th-sort {
    font-size: var(--text-hint);
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  tbody tr {
    cursor: default;
  }
  tbody tr:nth-child(even) {
    background: var(--bg-surface-alt);
  }
  tbody tr:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  tbody tr.selected {
    background: var(--accent-weak);
  }
  /* Inset accent bar on the leading cell marks the selected row — reliable
     under border-collapse, where an inset box-shadow on <tr> is not. */
  tbody tr.selected td:first-child {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .null-cell {
    color: var(--text-muted);
    font-style: italic;
  }
  /* Numeric columns right-align with figure-aligned digits so the ones, tens
     and hundreds stack — detected from the JSON scalar, never fabricated. */
  .num-cell {
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: 10;
  }
  .popup {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    max-width: min(720px, 90vw);
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .popup-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .popup-col {
    font-family: var(--font-mono);
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--text-accent);
  }
  .popup .ghost {
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 3px 10px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .popup .ghost:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .popup-body {
    margin: 0;
    padding: var(--space-3);
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--text-small);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text);
  }
</style>
