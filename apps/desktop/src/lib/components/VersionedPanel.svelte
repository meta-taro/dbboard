<script lang="ts">
  // The Commits tab: the history a versioned database keeps (ADR-0165).
  //
  // Two readings of one table. "What changed" lists the commits and shows,
  // for the one picked, the rows of the table it added, modified or removed,
  // with each changed cell beside its old value. "Last changed by" shows the
  // commit that last touched each row. Either opens one row's full history.
  //
  // Nothing here writes: no commit, checkout or merge (ADR-0165).
  import {
    blame,
    commitDiff,
    describeTable,
    tableKey,
    type TableInfo,
  } from '$lib/api';
  import type { KeyColumn } from '$lib/grid/edit';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { workspace } from '$lib/state/workspace.svelte';
  import { versioned } from '$lib/versioned/versioned.svelte';
  import {
    blameRows,
    diffRows,
    keepsNoHistory,
    keyFor,
    parentOf,
    shortHash,
    type BlameRow,
    type CommitDiff,
  } from '$lib/versioned/view';
  import RowHistoryDialog from './RowHistoryDialog.svelte';

  type Mode = 'diff' | 'blame';

  let mode = $state<Mode>('diff');
  let tableId = $state('');
  let selectedHash = $state('');

  /** Null until the table's schema has answered; empty means it has none. */
  let primaryKey = $state<string[] | null>(null);
  let diff = $state<CommitDiff | null>(null);
  let blamed = $state<BlameRow[] | null>(null);
  let loading = $state(false);
  let error = $state('');
  let opened = $state<KeyColumn[] | null>(null);

  // The tables of whichever database the tree is pointing at, the same list
  // the sidebar draws.
  const tables = $derived<TableInfo[]>(
    workspace.database
      ? (workspace.databaseTables[workspace.database] ?? [])
      : workspace.tables,
  );
  const table = $derived(tables.find((t) => tableKey(t) === tableId) ?? null);
  const from = $derived(parentOf(versioned.commits, selectedHash));
  const rows = $derived(diff ? diffRows(diff) : []);

  // Follow the sidebar: a table picked there is the one asked about here.
  $effect(() => {
    const picked = workspace.selectedTable;
    if (picked) tableId = tableKey(picked);
  });

  // Start on the newest commit, and again whenever the log is replaced.
  $effect(() => {
    const commits = versioned.commits;
    if (!commits.some((c) => c.hash === selectedHash)) {
      selectedHash = commits[0]?.hash ?? '';
    }
  });

  // The primary key decides whether a row's history can be asked for.
  $effect(() => {
    const t = table;
    primaryKey = null;
    if (!t) return;
    const conn = workspace.connectionId;
    describeTable(conn, t.name, t.schema, workspace.database)
      .then((schema) => {
        if (table === t) primaryKey = schema.primary_key;
      })
      .catch(() => {
        // The diff still reads without it; only the per-row history is lost.
        if (table === t) primaryKey = [];
      });
  });

  let seq = 0;
  $effect(() => {
    const t = table;
    const m = mode;
    const to = selectedHash;
    const base = from;
    // Read only in blame mode, so that the key arriving does not fetch the
    // same diff a second time.
    const key = m === 'blame' ? primaryKey : null;
    void load(t, m, to, base, key);
  });

  async function load(
    t: TableInfo | null,
    m: Mode,
    to: string,
    base: string | null,
    key: string[] | null,
  ) {
    const mine = ++seq;
    diff = null;
    blamed = null;
    error = '';
    if (!t) return;
    if (m === 'diff' && (!to || !base)) return;
    // Dolt has no blame for a table without a primary key.
    if (m === 'blame' && (key === null || key.length === 0)) return;
    loading = true;
    try {
      if (m === 'diff') {
        const d = await commitDiff(workspace.connectionId, t, base!, to, workspace.database);
        if (mine === seq) diff = d;
      } else {
        const b = await blame(workspace.connectionId, t, workspace.database);
        if (mine === seq) blamed = blameRows(key!, b);
      }
    } catch (e) {
      if (mine === seq) error = String(e);
    } finally {
      if (mine === seq) loading = false;
    }
  }

  function reload() {
    void versioned.refresh(workspace.connectionId, workspace.database);
  }

  const kindMark = { added: '+', modified: '~', removed: '−' } as const;
</script>

<div class="panel">
  <div class="bar">
    <label class="field">
      <span class="label">{i18n.t('versioned-table')}</span>
      <select bind:value={tableId}>
        <option value="" disabled>{i18n.t('versioned-pick-table')}</option>
        {#each tables as t (tableKey(t))}
          <option value={tableKey(t)}>{tableKey(t)}</option>
        {/each}
      </select>
    </label>

    <div class="segmented" role="group">
      <button type="button" class:on={mode === 'diff'} aria-pressed={mode === 'diff'} onclick={() => (mode = 'diff')}>
        {i18n.t('versioned-mode-diff')}
      </button>
      <button type="button" class:on={mode === 'blame'} aria-pressed={mode === 'blame'} onclick={() => (mode = 'blame')}>
        {i18n.t('versioned-mode-blame')}
      </button>
    </div>

    <button type="button" class="ghost" onclick={reload} disabled={versioned.loading}>
      {i18n.t('versioned-refresh')}
    </button>
    <span class="note">{i18n.t('versioned-read-only')}</span>
  </div>

  {#if versioned.error}
    <p class="error">{versioned.error}</p>
  {/if}

  <div class="body">
    {#if mode === 'diff'}
      <ul class="commits" aria-label={i18n.t('tab-versioned')}>
        {#each versioned.commits as c (c.hash)}
          <li>
            <button
              type="button"
              class="commit"
              class:selected={c.hash === selectedHash}
              aria-current={c.hash === selectedHash ? 'true' : undefined}
              onclick={() => (selectedHash = c.hash)}
            >
              <span class="message">{c.message}</span>
              <span class="meta"><span class="mono hash">{shortHash(c.hash)}</span> · {c.author} · {c.date}</span>
            </button>
          </li>
        {:else}
          <li class="muted">{i18n.t('versioned-no-commits')}</li>
        {/each}
      </ul>
    {/if}

    <section class="detail">
      {#if !table}
        <p class="muted">{i18n.t('versioned-no-table')}</p>
      {:else if keepsNoHistory(error)}
        <p class="muted">{i18n.t('versioned-latest-only', { table: tableKey(table) })}</p>
      {:else if error}
        <p class="error">{error}</p>
      {:else if mode === 'diff' && selectedHash && !from}
        <p class="muted">{i18n.t('versioned-first-commit')}</p>
      {:else if mode === 'blame' && primaryKey?.length === 0}
        <p class="muted">{i18n.t('versioned-no-key', { table: tableKey(table) })}</p>
      {:else if loading}
        <p class="muted">{i18n.t('versioned-loading')}</p>
      {:else if mode === 'diff' && diff}
        {#if rows.length === 0}
          <p class="muted">{i18n.t('versioned-no-changes', { table: tableKey(table) })}</p>
        {:else}
          <p class="summary">
            <span class="mono">{shortHash(from ?? '')} → {shortHash(selectedHash)}</span>
            · {i18n.t('versioned-change-count', { count: rows.length, table: tableKey(table) })}
          </p>
          <div class="scroll">
            <table class="grid">
              <thead>
                <tr>
                  <th class="kind-col"></th>
                  {#each diff.columns as col (col.name)}<th>{col.name}</th>{/each}
                  <th class="action-col"></th>
                </tr>
              </thead>
              <tbody>
                {#each rows as row, r (r)}
                  {@const key = keyFor(diff.columns, primaryKey ?? [], row.values)}
                  <tr class="row-{row.kind}">
                    <td class="kind-col">
                      <span class="badge {row.kind}" title={i18n.t(`versioned-kind-${row.kind}`)}>
                        <span aria-hidden="true">{kindMark[row.kind]}</span>
                        {i18n.t(`versioned-kind-${row.kind}`)}
                      </span>
                    </td>
                    {#each row.cells as cell, i (i)}
                      <td
                        class="mono"
                        class:changed={cell.changed}
                        title={cell.before !== null ? i18n.t('versioned-was', { value: cell.before }) : undefined}
                      >
                        {#if cell.before !== null}
                          <span class="before">{cell.before}</span><span class="arrow" aria-hidden="true">→</span>
                        {/if}{cell.text}
                      </td>
                    {/each}
                    <td class="action-col">
                      {#if key}
                        <button type="button" class="link" onclick={() => (opened = key)}>
                          {i18n.t('versioned-row-history')}
                        </button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      {:else if mode === 'blame' && blamed}
        {#if blamed.length === 0}
          <p class="muted">{i18n.t('versioned-no-rows', { table: tableKey(table) })}</p>
        {:else}
          <div class="scroll">
            <table class="grid">
              <thead>
                <tr>
                  {#each primaryKey ?? [] as col (col)}<th>{col}</th>{/each}
                  <th>{i18n.t('versioned-col-message')}</th>
                  <th>{i18n.t('versioned-col-author')}</th>
                  <th>{i18n.t('versioned-col-date')}</th>
                  <th>{i18n.t('versioned-col-commit')}</th>
                  <th class="action-col"></th>
                </tr>
              </thead>
              <tbody>
                {#each blamed as row, r (r)}
                  <tr>
                    {#each row.cells as cell, i (i)}<td class="mono">{cell}</td>{/each}
                    <td>{row.commit.message}</td>
                    <td>{row.commit.author}</td>
                    <td class="mono">{row.commit.date}</td>
                    <td class="mono hash">{shortHash(row.commit.hash)}</td>
                    <td class="action-col">
                      <button type="button" class="link" onclick={() => (opened = row.key)}>
                        {i18n.t('versioned-row-history')}
                      </button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      {/if}
    </section>
  </div>
</div>

{#if opened && table}
  <RowHistoryDialog
    connectionId={workspace.connectionId}
    database={workspace.database}
    {table}
    rowKey={opened}
    onClose={() => (opened = null)}
  />
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .field {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .label {
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  select {
    font: inherit;
    font-size: var(--text-small);
    min-width: 12rem;
    padding: 3px 6px;
    color: var(--text);
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .segmented {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    overflow: hidden;
  }
  .segmented button {
    border: 0;
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-small);
    padding: 4px 10px;
    cursor: pointer;
  }
  .segmented button + button {
    border-left: 1px solid var(--border);
  }
  .segmented button.on {
    color: var(--text-accent);
    background: var(--accent-weak);
  }
  .ghost {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-small);
    padding: 3px 10px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .ghost:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .note {
    margin-left: auto;
    color: var(--text-muted);
    font-size: var(--text-hint);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .commits {
    flex: none;
    width: 300px;
    margin: 0;
    padding: var(--space-2);
    list-style: none;
    overflow: auto;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .commit {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    padding: 6px 8px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .commit:hover {
    background: var(--bg-surface-alt);
  }
  .commit.selected {
    background: var(--accent-weak);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .message {
    font-size: var(--text-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    color: var(--text-muted);
    font-size: var(--text-hint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hash {
    color: var(--text-accent);
  }
  .mono {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .detail {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: var(--space-3);
    gap: var(--space-2);
  }
  .summary {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .muted {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
    padding: var(--space-2);
  }
  .error {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
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
    white-space: nowrap;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--bg-surface-alt);
    color: var(--text-muted);
    font-weight: 600;
  }
  td.changed {
    background: var(--warning-weak);
  }
  .before {
    color: var(--text-muted);
    text-decoration: line-through;
  }
  .arrow {
    color: var(--text-muted);
    margin: 0 3px;
  }
  .kind-col {
    width: 1%;
  }
  .action-col {
    width: 1%;
    text-align: right;
  }
  /* The kind is spelled out beside its symbol: colour is never the only
     mark (DESIGN.md). */
  .badge {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    font-size: var(--text-hint);
    padding: 0 6px;
    border-radius: var(--radius-pill);
    border: 1px solid currentColor;
  }
  .badge.added {
    color: var(--success);
    background: var(--success-weak);
  }
  .badge.modified {
    color: var(--warning);
    background: var(--warning-weak);
  }
  .badge.removed {
    color: var(--danger);
    background: var(--danger-weak);
  }
  .row-removed td.mono {
    color: var(--text-muted);
    text-decoration: line-through;
  }
  .link {
    border: 0;
    background: none;
    color: var(--text-accent);
    font-size: var(--text-hint);
    cursor: pointer;
    padding: 0;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
