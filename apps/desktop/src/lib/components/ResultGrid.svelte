<script lang="ts">
  import { save } from '@tauri-apps/plugin-dialog';
  import {
    displayCell,
    saveTextFile,
    updateRow,
    type Cell,
    type Column,
  } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import {
    nextSortKeys,
    sortedIndices,
    toDelimited,
    toDelimitedFile,
    type SortKey,
  } from '$lib/grid/format';
  import {
    buildRowUpdates,
    cellKey,
    needsWideEditor,
    type EditContext,
    type StagedValue,
  } from '$lib/grid/edit';

  interface Props {
    columns: Column[];
    rows: Cell[][];
    rowCount: number;
    truncated: boolean;
    // The row cap that was applied for this run, so a truncated result can say
    // exactly where it stopped ("capped at 1000") instead of a vague suffix.
    limit?: number;
    // Present when the result is an editable table browse: enables inline cell
    // editing keyed on the table's primary key (ADR-0042). Null = read-only.
    edit?: EditContext | null;
    // Called after a save commits, so the parent can re-run the browse and show
    // engine-normalised values.
    onSaved?: () => void;
  }
  let {
    columns,
    rows,
    rowCount,
    truncated,
    limit,
    edit = null,
    onSaved,
  }: Props = $props();

  let sortKeys = $state<SortKey[]>([]);
  // Selection is keyed by ORIGINAL row index so it survives re-sorting.
  let selected = $state<Set<number>>(new Set());
  let anchor = $state<number | null>(null); // last-clicked display position
  let copied = $state('');
  let popup = $state<{ col: string; value: string } | null>(null);

  // Inline editing (ADR-0042). Staged edits are keyed by cellKey(origRow, col)
  // — original row index, so they survive re-sorting — mapping to the new value
  // (text, or null for SQL NULL). `editing` is the one open inline editor.
  let staged = $state<Map<string, StagedValue>>(new Map());
  let editing = $state<{ row: number; col: number; draft: string } | null>(null);
  // The full editor dialog (ADR-0082): the same staging, on a surface big
  // enough for a varchar(500). Mutually exclusive with `editing`.
  let expanded = $state<{ row: number; col: number; draft: string } | null>(null);
  let saving = $state(false);
  let saveError = $state('');

  // Reset transient state when a new result arrives.
  $effect(() => {
    rows; // track
    sortKeys = [];
    selected = new Set();
    anchor = null;
    popup = null;
    staged = new Map();
    editing = null;
    expanded = null;
    saveError = '';
  });

  // A column is editable when the result is an editable browse and the column
  // is NOT part of the primary key (the key identifies the row and drives the
  // WHERE clause, so it is held fixed — parity with the egui editor).
  function columnEditable(ci: number): boolean {
    return !!edit && !edit.pk.includes(columns[ci].name);
  }

  // A blob cell has no sensible text editor, so editing skips it even in an
  // otherwise-editable column.
  function isBlob(cell: Cell): boolean {
    return typeof cell === 'object' && cell !== null && '$blob' in cell;
  }

  function stagedAt(origIdx: number, ci: number): StagedValue | undefined {
    return staged.get(cellKey(origIdx, ci));
  }

  function isDirty(origIdx: number, ci: number): boolean {
    return staged.has(cellKey(origIdx, ci));
  }

  // The text a cell shows: its staged edit (NULL rendered explicitly) if any,
  // otherwise the original value.
  function cellText(origIdx: number, ci: number, cell: Cell): string {
    const s = stagedAt(origIdx, ci);
    if (s !== undefined) return s === null ? 'NULL' : s;
    return displayCell(cell);
  }

  function cellIsNull(origIdx: number, ci: number, cell: Cell): boolean {
    const s = stagedAt(origIdx, ci);
    return s !== undefined ? s === null : cell === null;
  }

  // The text an editor starts from: the staged edit if there is one, else the
  // stored value. A NULL starts as empty text — typing over it means "this is
  // now a value"; the ∅ button is how NULL is chosen deliberately.
  function draftFor(origIdx: number, ci: number, cell: Cell): string {
    const s = stagedAt(origIdx, ci);
    const current = s !== undefined ? s : cell;
    return current === null ? '' : displayCell(current as Cell);
  }

  function beginEdit(origIdx: number, ci: number, cell: Cell) {
    if (!columnEditable(ci) || isBlob(cell)) return;
    editing = { row: origIdx, col: ci, draft: draftFor(origIdx, ci, cell) };
  }

  function openExpanded(origIdx: number, ci: number, cell: Cell) {
    if (!columnEditable(ci) || isBlob(cell)) return;
    editing = null;
    expanded = { row: origIdx, col: ci, draft: draftFor(origIdx, ci, cell) };
  }

  // Hand the inline editor's current text to the dialog, so a value that turned
  // out to need more room is not retyped.
  function expandEditor() {
    if (!editing) return;
    expanded = { ...editing };
    editing = null;
  }

  function commitExpanded() {
    if (!expanded) return;
    setStaged(expanded.row, expanded.col, expanded.draft);
    expanded = null;
  }

  function nullExpanded() {
    if (!expanded) return;
    setStaged(expanded.row, expanded.col, null);
    expanded = null;
  }

  function cancelExpanded() {
    expanded = null;
  }

  // Enter inserts a newline in the dialog's textarea, so committing needs a
  // modifier. Escape is handled by the window listener, like the value popup.
  function onExpandedKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      commitExpanded();
    }
  }

  // Characters, not UTF-16 units: a `varchar(500)` limit counts the same way,
  // so the number shown is the one the column actually constrains.
  function charCount(text: string): number {
    return [...text].length;
  }

  function setStaged(origIdx: number, ci: number, value: StagedValue) {
    const next = new Map(staged);
    next.set(cellKey(origIdx, ci), value);
    staged = next;
  }

  // Commit the open editor's text as the cell's new value.
  function commitEditor() {
    if (!editing) return;
    setStaged(editing.row, editing.col, editing.draft);
    editing = null;
  }

  // Stage an explicit SQL NULL for the cell being edited.
  function nullEditor() {
    if (!editing) return;
    setStaged(editing.row, editing.col, null);
    editing = null;
  }

  function cancelEditor() {
    editing = null;
  }

  function onEditorKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitEditor();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelEditor();
    }
  }

  function onCellDblClick(origIdx: number, ci: number, cell: Cell) {
    if (!columnEditable(ci) || isBlob(cell)) {
      openCell(columns[ci].name, cell);
      return;
    }
    // A value the inline box cannot hold goes straight to the dialog. Opening a
    // 40-character slot onto 500 characters of prose is not an editor.
    if (needsWideEditor(draftFor(origIdx, ci, cell))) {
      openExpanded(origIdx, ci, cell);
    } else {
      beginEdit(origIdx, ci, cell);
    }
  }

  function discardEdits() {
    staged = new Map();
    editing = null;
    expanded = null;
    saveError = '';
  }

  // Save every staged edit as one UPDATE per touched row. Stops at the first
  // failure, keeping the remaining edits staged so the user can fix and retry;
  // on full success clears staging and asks the parent to reload.
  async function saveEdits() {
    if (!edit || saving) return;
    commitEditor(); // flush an open editor into the staging map first
    if (staged.size === 0) return;
    let updates;
    try {
      updates = buildRowUpdates(staged, rows, columns, edit.pk);
    } catch (e) {
      saveError = String(e);
      return;
    }
    saving = true;
    saveError = '';
    try {
      for (const u of updates) {
        await updateRow(edit.connectionId, edit.table, u.key, u.edits);
      }
      staged = new Map();
      flash(i18n.t('edit-saved', { count: updates.length }));
      onSaved?.();
    } catch (e) {
      saveError = String(e);
    } finally {
      saving = false;
    }
  }

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
    // Only worth a popup for text the cell could not show in full — the same
    // display-width test the editor uses, so a truncated Japanese value opens
    // at the same point a truncated Latin one does.
    const value = displayCell(cell);
    if (cell === null || !needsWideEditor(value)) return;
    popup = { col, value };
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    popup = null;
    // Closing the editor dialog discards its draft: the destructive reading of
    // Escape is the one every other dialog in the app uses.
    expanded = null;
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

  {#if edit && (staged.size > 0 || saveError)}
    <div class="edit-bar" role="region" aria-label={i18n.t('edit-bar-label')}>
      <span class="edit-count">{i18n.t('edit-pending', { count: staged.size })}</span>
      {#if saveError}<span class="edit-error">{saveError}</span>{/if}
      <div class="edit-actions">
        <button type="button" onclick={discardEdits} disabled={saving}>
          {i18n.t('edit-discard')}
        </button>
        <button
          type="button"
          class="primary"
          onclick={saveEdits}
          disabled={saving || staged.size === 0}
        >
          {saving ? i18n.t('edit-saving') : i18n.t('edit-save')}
        </button>
      </div>
    </div>
  {/if}

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
              {@const isEditing =
                !!editing && editing.row === origIdx && editing.col === ci}
              <td
                class:null-cell={cellIsNull(origIdx, ci, cell)}
                class:num-cell={typeof cell === 'number' && !isDirty(origIdx, ci)}
                class:dirty={isDirty(origIdx, ci)}
                class:editable={columnEditable(ci) && !isBlob(cell)}
                class:editing={isEditing}
                title={columnEditable(ci) && !isBlob(cell)
                  ? i18n.t('edit-cell-hint')
                  : cellIsNull(origIdx, ci, cell)
                    ? 'NULL'
                    : cellText(origIdx, ci, cell)}
                ondblclick={() => onCellDblClick(origIdx, ci, cell)}
              >
                <!-- The value stays in the flow even while editing (hidden, not
                     removed) so the column keeps its width: an editor that
                     shrinks the column it sits in is why this was unusable. -->
                <span class="cell-value">{cellText(origIdx, ci, cell)}</span>
                {#if isEditing && editing}
                  <div class="cell-editor" role="presentation" onclick={(e) => e.stopPropagation()}>
                    <!-- svelte-ignore a11y_autofocus -->
                    <input
                      class="cell-input"
                      bind:value={editing.draft}
                      autofocus
                      spellcheck="false"
                      onkeydown={onEditorKeydown}
                      onblur={commitEditor}
                      title={i18n.t('edit-cell-editing')}
                    />
                    <button
                      type="button"
                      class="cell-btn"
                      onmousedown={(e) => {
                        // mousedown fires before the input's blur, so the draft
                        // has to move to the dialog before the blur commits it.
                        e.preventDefault();
                        e.stopPropagation();
                        expandEditor();
                      }}
                      title={i18n.t('edit-cell-expand')}>⤢</button
                    >
                    <button
                      type="button"
                      class="cell-btn"
                      onmousedown={(e) => {
                        // Same ordering: NULL must win over the blur's commit.
                        e.preventDefault();
                        e.stopPropagation();
                        nullEditor();
                      }}
                      title={i18n.t('edit-null-title')}>∅</button
                    >
                  </div>
                {/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>

{#if expanded && columns[expanded.col]}
  <div
    class="backdrop"
    onclick={(e) => {
      // Clicking away cancels rather than commits: a dialog opened by accident
      // must not be able to stage an edit on its way out.
      if (e.target === e.currentTarget) cancelExpanded();
    }}
    role="presentation"
  >
    <div
      class="popup editor"
      role="dialog"
      aria-modal="true"
      aria-label={i18n.t('edit-cell-dialog')}
    >
      <div class="popup-head">
        <span class="popup-col">{columns[expanded.col].name}</span>
        <span class="popup-len">
          {i18n.t('edit-cell-chars', { count: charCount(expanded.draft) })}
        </span>
      </div>
      <!-- svelte-ignore a11y_autofocus -->
      <textarea
        class="popup-edit"
        bind:value={expanded.draft}
        autofocus
        spellcheck="false"
        onkeydown={onExpandedKeydown}
      ></textarea>
      <div class="popup-foot">
        <button type="button" class="ghost" onclick={nullExpanded} title={i18n.t('edit-null-title')}>
          ∅ NULL
        </button>
        <span class="popup-hint">{i18n.t('edit-cell-dialog-hint')}</span>
        <button type="button" onclick={cancelExpanded}>{i18n.t('edit-cell-cancel')}</button>
        <button type="button" class="primary" onclick={commitExpanded}>
          {i18n.t('edit-cell-apply')}
        </button>
      </div>
    </div>
  </div>
{/if}

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

  /* Edit action bar: appears only while there are staged edits (or an error). */
  .edit-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 6px 10px;
    background: var(--accent-weak);
    border: 1px solid var(--accent);
    border-radius: var(--radius-widget);
  }
  .edit-count {
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--text-accent);
  }
  .edit-error {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: var(--text-hint);
    color: var(--danger);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .edit-actions {
    margin-left: auto;
    display: flex;
    gap: var(--space-2);
  }
  .edit-actions button {
    border: 1px solid var(--border);
    background: var(--bg-surface);
    color: var(--text);
    border-radius: var(--radius-widget);
    padding: 4px 12px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .edit-actions button:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  .edit-actions .primary {
    background: var(--accent);
    color: var(--on-accent);
    border-color: var(--accent);
  }
  .edit-actions button:disabled {
    opacity: 0.5;
    cursor: default;
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
    /* Above the inline editor's overlay, so scrolling a tall result does not
       slide the editor over the header. */
    z-index: 2;
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
  /* An editable cell hints its affordance on hover; a staged (dirty) cell keeps
     a persistent accent tint until the edit is saved or discarded. */
  td.editable {
    cursor: text;
  }
  td.editable:hover {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  td.dirty {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    font-weight: 600;
  }
  /* The editor floats over the cell instead of replacing its content: laid out
     in flow it would shrink the column to the input's minimum width, which is
     what made a varchar(500) unusable to edit. */
  td.editing {
    position: relative;
    overflow: visible;
  }
  td.editing .cell-value {
    visibility: hidden;
  }
  .cell-editor {
    position: absolute;
    top: 50%;
    left: 0;
    transform: translateY(-50%);
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 4px;
    /* At least as wide as the cell, but never narrower than a usable field. */
    min-width: max(100%, 22rem);
    padding: 3px;
    box-sizing: border-box;
    background: var(--bg-surface);
    border-radius: var(--radius-widget);
    box-shadow: var(--shadow-popover);
  }
  .cell-input {
    flex: 1;
    min-width: 0;
    box-sizing: border-box;
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--accent);
    border-radius: var(--radius-widget);
    padding: 2px 6px;
    font: inherit;
    font-family: var(--font-mono);
  }
  .cell-input:focus-visible {
    outline: none;
  }
  .cell-btn {
    flex: none;
    border: 1px solid var(--border);
    background: var(--bg-surface);
    color: var(--text-muted);
    border-radius: var(--radius-widget);
    padding: 1px 6px;
    font-size: var(--text-hint);
    cursor: pointer;
  }
  .cell-btn:hover {
    color: var(--text);
    border-color: var(--border-strong);
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
  /* The editor dialog: a fixed, generous surface, so the room to type does not
     depend on how wide the column happened to be. */
  .popup.editor {
    width: min(720px, 90vw);
  }
  .popup-len {
    font-size: var(--text-hint);
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .popup-edit {
    margin: 0;
    padding: var(--space-3);
    border: none;
    border-bottom: 1px solid var(--border);
    background: var(--bg-surface);
    color: var(--text);
    resize: vertical;
    min-height: 40vh;
    font-family: var(--font-mono);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .popup-edit:focus-visible {
    outline: none;
  }
  .popup-foot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
  }
  .popup-hint {
    margin-left: auto;
    font-size: var(--text-hint);
    color: var(--text-muted);
  }
  .popup-foot button {
    border: 1px solid var(--border);
    background: var(--bg-surface);
    color: var(--text);
    border-radius: var(--radius-widget);
    padding: 4px 12px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .popup-foot button:hover {
    border-color: var(--border-strong);
  }
  .popup-foot .primary {
    background: var(--accent);
    color: var(--on-accent);
    border-color: var(--accent);
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
