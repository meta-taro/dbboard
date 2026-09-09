<script lang="ts">
  // The Compare tab (ADR-0149): pick two connections of the same engine, read
  // what their schemas disagree about. The comparison and the cross-engine
  // refusal are the backend's (ADR-0148); this panel picks the pair, draws the
  // report, and answers the sidebar's jumps.
  import {
    diffSchemas,
    tableKey,
    type ColumnField,
    type ColumnInfo,
    type ForeignKeyField,
    type ForeignKeyRef,
    type TableDiff,
  } from '$lib/api';
  import { compare } from '$lib/compare/compare.svelte';
  import {
    comparableWith,
    sectionId,
    tableDifferenceCount,
    totalDifferingTables,
  } from '$lib/compare/diff';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { workspace } from '$lib/state/workspace.svelte';

  let leftId = $state('');
  let rightId = $state('');
  let busy = $state(false);
  let error = $state('');
  let report = $state<HTMLDivElement | null>(null);

  // Rule 1 — opening the tab seeds the left side from the connection in the
  // sidebar, because that is the one the operator is already thinking about.
  // It is a default, not a binding: they can pick any other.
  $effect(() => {
    if (leftId === '' && workspace.connectionId) leftId = workspace.connectionId;
  });

  // Rule 2 is the absence of an effect on `workspace.connectionId`: once a
  // report is on screen, switching connection in the sidebar leaves it alone.
  // A report that swapped itself out while being read would be its own bug
  // report, and the picker above says which two sides it came from.

  const targets = $derived(comparableWith(workspace.connections, leftId));
  const leftName = $derived(
    workspace.connections.find((c) => c.id === leftId)?.name ?? '',
  );
  const rightName = $derived(
    workspace.connections.find((c) => c.id === rightId)?.name ?? '',
  );
  const leftMark = $derived(workspace.marks[leftId]?.color ?? null);
  const rightMark = $derived(workspace.marks[rightId]?.color ?? null);
  const kind = $derived(
    workspace.connections.find((c) => c.id === leftId)?.kind ?? '',
  );
  const canRun = $derived(leftId !== '' && rightId !== '' && !busy);

  // A right-hand pick stops making sense when the left side moves to another
  // engine; drop it rather than run a pair the backend will refuse.
  $effect(() => {
    if (rightId && !targets.some((c) => c.id === rightId)) rightId = '';
  });

  // Rule 3 — the sidebar asks for a table, the report scrolls to it. Not a
  // filter: the whole report stays, only the viewport moves.
  $effect(() => {
    const jump = compare.jump;
    if (!jump || !report) return;
    const target = report.querySelector(`#${CSS.escape(jump.id)}`);
    target?.scrollIntoView({ block: 'start' });
  });

  async function run() {
    if (!canRun) return;
    busy = true;
    error = '';
    try {
      const diff = await diffSchemas(leftId, rightId);
      compare.hold(diff, leftId, rightId);
    } catch (e) {
      compare.clear();
      error = String(e);
    } finally {
      busy = false;
    }
  }

  /** The attributes a changed column disagrees about, as a readable list.
   *  Mapped explicitly rather than by building a key from the value: the
   *  catalogue's keys are a checked union, and a computed key would let a
   *  renamed field ship as a missing translation. */
  const FIELD_LABEL = {
    DeclaredType: 'compare-field-declaredtype',
    Nullable: 'compare-field-nullable',
    Default: 'compare-field-default',
    PrimaryKey: 'compare-field-primarykey',
  } as const;

  function fieldLabels(fields: ColumnField[]): string {
    return fields.map((f) => i18n.t(FIELD_LABEL[f])).join(' · ');
  }

  const FK_FIELD_LABEL = {
    ReferencedTable: 'compare-fk-table',
    ReferencedColumns: 'compare-fk-columns',
    Name: 'compare-fk-name',
  } as const;

  function fkFieldLabels(fields: ForeignKeyField[]): string {
    return fields.map((f) => i18n.t(FK_FIELD_LABEL[f])).join(' · ');
  }

  /** A foreign key in the report's shorthand: what it constrains, and where. */
  function describeKey(k: ForeignKeyRef): string {
    const target = tableKey(k.referenced_table);
    const name = k.constraint_name ? ` · ${k.constraint_name}` : '';
    return `(${k.columns.join(', ')}) → ${target}(${k.referenced_columns.join(', ')})${name}`;
  }

  /** One side's spelling of a column, in the report's shorthand. */
  function describe(c: ColumnInfo): string {
    const parts = [c.declared_type ?? '—', c.nullable ? 'NULL' : 'NOT NULL'];
    if (c.default_value) parts.push(`default ${c.default_value}`);
    if (c.primary_key) parts.push('PK');
    return parts.join(' · ');
  }

  function count(t: TableDiff): number {
    return tableDifferenceCount(t);
  }
</script>

<div class="panel">
  <div class="picker">
    <label class="side">
      <span class="side-label">{i18n.t('compare-left')}</span>
      <select bind:value={leftId} disabled={busy}>
        {#each workspace.connections as c (c.id)}
          <option value={c.id}>{c.name}</option>
        {/each}
      </select>
    </label>

    <span class="between" aria-hidden="true">↔</span>

    <label class="side">
      <span class="side-label">{i18n.t('compare-right')}</span>
      <select bind:value={rightId} disabled={busy || targets.length === 0}>
        <option value="">{i18n.t('compare-pick-target')}</option>
        {#each targets as c (c.id)}
          <option value={c.id}>{c.name}</option>
        {/each}
      </select>
    </label>

    {#if targets.length === 0 && leftId}
      <span class="no-target">{i18n.t('compare-no-target', { kind })}</span>
    {/if}

    <div class="spacer"></div>

    {#if compare.active}
      <span class="summary">
        {i18n.t('compare-summary', {
          count: totalDifferingTables(compare.diff!),
        })}
      </span>
    {/if}
    <button type="button" class="run" onclick={run} disabled={!canRun}>
      {busy ? i18n.t('compare-running') : i18n.t('compare-run')}
    </button>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if compare.active && compare.diff}
    {@const diff = compare.diff}
    <div class="report" bind:this={report}>
      <div class="head">
        <div class="head-side">
          {#if leftMark}<span class="dot" style="background: var(--conn-{leftMark})"></span>{/if}
          <span class="mono">{leftName}</span>
        </div>
        <div class="head-side">
          {#if rightMark}<span class="dot" style="background: var(--conn-{rightMark})"></span>{/if}
          <span class="mono">{rightName}</span>
        </div>
      </div>

      {#if totalDifferingTables(diff) === 0}
        <div class="empty" role="note">
          <span class="empty-title">{i18n.t('compare-no-differences')}</span>
          <span class="empty-body">
            {diff.foreign_keys_compared
              ? i18n.t('compare-scope-note-fk')
              : i18n.t('compare-scope-note')}
          </span>
        </div>
      {/if}

      {#each diff.tables_changed as t (tableKey(t.table))}
        <div class="table-head" id={sectionId(t.table)}>
          <span class="mono table-name">{tableKey(t.table)}</span>
          <span class="table-count">{i18n.t('compare-table-count', { count: count(t) })}</span>
        </div>

        {#each t.columns_changed as c (c.name)}
          <div class="row">
            <div class="cell">
              <span class="mono col-name">{c.name}</span>
              <span class="mono value">{describe(c.left)}</span>
            </div>
            <div class="cell differs">
              <span class="mono col-name">{c.name}</span>
              <span class="mono value">{describe(c.right)}</span>
              <span class="fields">{fieldLabels(c.fields)}</span>
            </div>
          </div>
        {/each}

        {#each t.columns_only_in_left as c (c.name)}
          <div class="row">
            <div class="cell">
              <span class="mono col-name">{c.name}</span>
              <span class="mono value">{describe(c)}</span>
              <span class="only left">{i18n.t('compare-only-in', { name: leftName })}</span>
            </div>
            <div class="cell absent">—</div>
          </div>
        {/each}

        {#each t.columns_only_in_right as c (c.name)}
          <div class="row">
            <div class="cell absent">—</div>
            <div class="cell">
              <span class="mono col-name">{c.name}</span>
              <span class="mono value">{describe(c)}</span>
              <span class="only right">{i18n.t('compare-only-in', { name: rightName })}</span>
            </div>
          </div>
        {/each}

        {#if t.primary_key}
          <div class="row">
            <div class="cell">
              <span class="pk">PK</span>
              <span class="mono value">({t.primary_key[0].join(', ')})</span>
            </div>
            <div class="cell differs">
              <span class="pk">PK</span>
              <span class="mono value">({t.primary_key[1].join(', ')})</span>
            </div>
          </div>
        {/if}

        {#each t.foreign_keys_changed as k (k.columns.join(','))}
          <div class="row">
            <div class="cell">
              <span class="pk fk">FK</span>
              <span class="mono value">{describeKey(k.left)}</span>
            </div>
            <div class="cell differs">
              <span class="pk fk">FK</span>
              <span class="mono value">{describeKey(k.right)}</span>
              <span class="fields">{fkFieldLabels(k.fields)}</span>
            </div>
          </div>
        {/each}

        {#each t.foreign_keys_only_in_left as k (k.columns.join(','))}
          <div class="row">
            <div class="cell">
              <span class="pk fk">FK</span>
              <span class="mono value">{describeKey(k)}</span>
              <span class="only left">{i18n.t('compare-only-in', { name: leftName })}</span>
            </div>
            <div class="cell absent">—</div>
          </div>
        {/each}

        {#each t.foreign_keys_only_in_right as k (k.columns.join(','))}
          <div class="row">
            <div class="cell absent">—</div>
            <div class="cell">
              <span class="pk fk">FK</span>
              <span class="mono value">{describeKey(k)}</span>
              <span class="only right">{i18n.t('compare-only-in', { name: rightName })}</span>
            </div>
          </div>
        {/each}
      {/each}

      {#each diff.tables_only_in_left as t (tableKey(t))}
        <div class="row" id={sectionId(t)}>
          <div class="cell">
            <span class="mono table-name">{tableKey(t)}</span>
            <span class="only left">{i18n.t('compare-only-in', { name: leftName })}</span>
          </div>
          <div class="cell absent">—</div>
        </div>
      {/each}

      {#each diff.tables_only_in_right as t (tableKey(t))}
        <div class="row" id={sectionId(t)}>
          <div class="cell absent">—</div>
          <div class="cell">
            <span class="mono table-name">{tableKey(t)}</span>
            <span class="only right">{i18n.t('compare-only-in', { name: rightName })}</span>
          </div>
        </div>
      {/each}
    </div>
  {:else if !error}
    <div class="intro" role="note">
      <p class="intro-title">{i18n.t('compare-intro-title')}</p>
      <p class="intro-body">{i18n.t('compare-intro-body')}</p>
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
    height: 100%;
  }

  .picker {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .side-label {
    font-size: var(--text-hint);
    color: var(--faint);
  }
  .between {
    color: var(--faint);
    padding-bottom: 6px;
  }
  .no-target {
    font-size: var(--text-small);
    color: var(--text-muted);
    padding-bottom: 6px;
  }
  .spacer {
    flex-grow: 1;
  }
  .summary {
    font-size: var(--text-small);
    color: var(--text-muted);
    padding-bottom: 6px;
  }
  select {
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 5px 8px;
    font-size: var(--text-body);
  }
  .run {
    background: var(--accent);
    color: var(--on-accent);
    border: none;
    border-radius: var(--radius-widget);
    padding: 6px 14px;
    font-size: var(--text-body);
    font-weight: 600;
    cursor: pointer;
  }
  .run:disabled {
    background: var(--bg-surface-alt);
    color: var(--faint);
    cursor: default;
  }

  .error {
    margin: 0;
    border-left: 2px solid var(--danger);
    background: var(--danger-weak);
    border-radius: var(--radius-widget);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-small);
    color: var(--text);
  }

  .intro {
    border: 1px dashed var(--border);
    border-radius: var(--radius-window);
    padding: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .intro-title {
    margin: 0;
    font-size: var(--text-body);
    color: var(--text);
  }
  .intro-body {
    margin: 0;
    font-size: var(--text-small);
    color: var(--text-muted);
    line-height: var(--line-body);
  }

  .report {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
  }
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    display: grid;
    grid-template-columns: 1fr 1fr;
    background: var(--bg-code);
    border-bottom: 1px solid var(--border);
  }
  .head-side {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 7px var(--space-3);
    font-size: var(--text-small);
    color: var(--text);
  }
  .head-side + .head-side {
    border-left: 1px solid var(--border);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-pill);
    flex: none;
  }

  .table-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    padding: 7px var(--space-3);
    background: var(--bg-surface-alt);
    border-bottom: 1px solid var(--border);
    scroll-margin-top: 32px;
  }
  .table-name {
    font-weight: 600;
    color: var(--text);
    font-size: var(--text-body);
  }
  .table-count {
    font-size: var(--text-hint);
    color: var(--text-muted);
  }

  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    border-bottom: 1px solid var(--border);
    scroll-margin-top: 32px;
  }
  .cell {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    padding: 6px var(--space-3);
    font-size: var(--text-body);
    min-width: 0;
  }
  .cell + .cell {
    border-left: 1px solid var(--border);
  }
  /* Differences are marked with the caution colour, never the danger one:
     two databases differing is not a fault, it is the answer to the
     question that was asked. */
  .differs {
    background: var(--warning-weak);
  }
  .absent {
    color: var(--faint);
    font-size: var(--text-small);
  }
  .col-name {
    font-weight: 600;
    color: var(--text);
  }
  .value {
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fields {
    font-size: var(--text-hint);
    color: var(--text-muted);
  }
  /* The side a row exists on is said in that connection's identity colour —
     the axis that answers "which server is this" (issue #192), which is
     exactly what the label means here. */
  .only {
    font-size: var(--text-hint);
    font-weight: 700;
    border-radius: var(--radius-widget);
    padding: 0 5px;
    white-space: nowrap;
  }
  .only.left {
    color: var(--text-accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .only.right {
    color: var(--text-accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .pk {
    font-size: var(--text-hint);
    font-weight: 700;
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: var(--radius-widget);
    padding: 0 5px;
  }

  /* The same badge as PK, in the muted ink: a foreign key is a constraint the
     reader scans past unless it differs, and it should not shout louder than
     the key the table is identified by. */
  .fk {
    color: var(--text-muted);
    border-color: var(--border-strong);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6);
  }
  .empty-title {
    font-size: var(--text-body);
    color: var(--text);
  }
  .empty-body {
    font-size: var(--text-hint);
    color: var(--text-muted);
    text-align: center;
    line-height: var(--line-body);
  }

  .mono {
    font-family: var(--font-mono);
  }
</style>
