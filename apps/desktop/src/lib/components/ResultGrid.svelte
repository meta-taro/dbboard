<script lang="ts">
  import { save } from '@tauri-apps/plugin-dialog';
  import { timestampedFileName } from '$lib/export/filename';
  import {
    displayCell,
    isDocument,
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
  import { enumOptions } from '$lib/grid/enum';
  import CellViewer from './CellViewer.svelte';
  import ExpandedCellEditor from './ExpandedCellEditor.svelte';
  import '$lib/styles/result-grid.css';

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
    // Members of every ENUM column of the browsed table, by column name. An
    // enum edited as free text is a spelling test; with this the editor offers
    // the declared members instead (ADR-0102). Empty for a non-enum table, and
    // for any result that is not an editable browse.
    enums?: Record<string, string[]>;
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
    enums = {},
    onSaved,
  }: Props = $props();

  let sortKeys = $state<SortKey[]>([]);
  // Selection is keyed by ORIGINAL row index so it survives re-sorting.
  let selected = $state<Set<number>>(new Set());
  let anchor = $state<number | null>(null); // last-clicked display position
  let copied = $state('');
  // The read-only value dialog. `doc` is set only for a document cell, and is
  // wrapped so that a document whose value is literally `null` stays
  // distinguishable from "this is not a document" (ADR-0100).
  let popup = $state<{ col: string; value: string; doc: { value: unknown } | null } | null>(
    null,
  );
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

  // Neither a blob nor a document has a sensible single-line text editor — one
  // is bytes, the other a tree that a free-text edit could leave unparseable —
  // so editing skips both even in an otherwise-editable column.
  function isUneditable(cell: Cell): boolean {
    return (
      (typeof cell === 'object' && cell !== null && '$blob' in cell) ||
      isDocument(cell)
    );
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

  // The declared members of column `ci`, or null when it is not an enum — in
  // which case every editor falls back to free text.
  function variantsFor(ci: number): string[] | null {
    return enums[columns[ci].name] ?? null;
  }

  function beginEdit(origIdx: number, ci: number, cell: Cell) {
    if (!columnEditable(ci) || isUneditable(cell)) return;
    editing = { row: origIdx, col: ci, draft: draftFor(origIdx, ci, cell) };
  }

  function openExpanded(origIdx: number, ci: number, cell: Cell) {
    if (!columnEditable(ci) || isUneditable(cell)) return;
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
    if (!columnEditable(ci) || isUneditable(cell)) {
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
      defaultPath: timestampedFileName('dbboard-result', 'csv'),
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
    // A document always opens, however short its serialisation: the row shows
    // it as one line of JSON, and one line is exactly what a tree is not.
    if (isDocument(cell)) {
      popup = { col, value: JSON.stringify(cell.$json, null, 2), doc: { value: cell.$json } };
      return;
    }
    // Otherwise only worth a popup for text the cell could not show in full —
    // the same display-width test the editor uses, so a truncated Japanese
    // value opens at the same point a truncated Latin one does.
    const value = displayCell(cell);
    if (cell === null || !needsWideEditor(value)) return;
    popup = { col, value, doc: null };
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

<div class="wrap result-grid">
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
                class:editable={columnEditable(ci) && !isUneditable(cell)}
                class:editing={isEditing}
                title={columnEditable(ci) && !isUneditable(cell)
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
                  {@const variants = variantsFor(ci)}
                  <div class="cell-editor" role="presentation" onclick={(e) => e.stopPropagation()}>
                    {#if variants}
                      <!-- svelte-ignore a11y_autofocus -->
                      <select
                        class="cell-input"
                        bind:value={editing.draft}
                        autofocus
                        onchange={commitEditor}
                        onkeydown={onEditorKeydown}
                        onblur={commitEditor}
                        title={i18n.t('edit-enum-editing')}
                      >
                        {#each enumOptions(variants, editing.draft) as v (v)}
                          <option value={v}>{v === '' ? i18n.t('edit-enum-blank') : v}</option>
                        {/each}
                      </select>
                    {:else}
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
                      <!-- No wide editor for an enum: the choices are the whole
                           value space, and none of them needs more room. -->
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
                    {/if}
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
  <ExpandedCellEditor
    col={columns[expanded.col].name}
    bind:draft={expanded.draft}
    variants={variantsFor(expanded.col)}
    onCommit={commitExpanded}
    onNull={nullExpanded}
    onCancel={cancelExpanded}
  />
{/if}

{#if popup}
  <CellViewer col={popup.col} value={popup.value} doc={popup.doc} onClose={() => (popup = null)} />
{/if}
