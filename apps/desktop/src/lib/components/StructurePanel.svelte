<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import {
    describeTable,
    getAnnotations,
    listRelationships,
    tableKey,
    type Relationship,
    type TableSchema,
  } from '$lib/api';

  let schema = $state<TableSchema | null>(null);
  let tableNote = $state<string | null>(null);
  let columnNotes = $state<Map<string, string>>(new Map());
  let relationships = $state<Relationship[]>([]);
  let loading = $state(false);
  let error = $state('');

  // Is this edge endpoint the table currently being viewed?
  function isCurrent(table: { schema: string | null; name: string }): boolean {
    return (
      !!workspace.selectedTable &&
      tableKey(table) === tableKey(workspace.selectedTable)
    );
  }

  // A monotonic token so a response for an older selection can't overwrite a
  // newer one (fast clicking down the table list).
  let seq = 0;

  $effect(() => {
    const table = workspace.selectedTable;
    const connId = workspace.connectionId;

    if (!table || !connId) {
      schema = null;
      tableNote = null;
      columnNotes = new Map();
      relationships = [];
      return;
    }

    const mine = ++seq;
    void (async () => {
      loading = true;
      error = '';
      try {
        // Structure (columns/PK), local notes, and foreign keys are all
        // independent reads; fetch them together. The note/FK filter key
        // matches dbboard-config's `table_key` (schema.name / bare name),
        // which is exactly tableKey().
        const [s, ann, rel] = await Promise.all([
          describeTable(connId, table.name, table.schema),
          getAnnotations(connId, tableKey(table)),
          listRelationships(connId, tableKey(table)),
        ]);
        if (mine !== seq) return; // superseded by a newer selection

        schema = s;
        const ta = ann.tables[0];
        tableNote = ta?.note ?? null;
        const notes = new Map<string, string>();
        for (const c of ta?.columns ?? []) {
          if (c.note) notes.set(c.name, c.note);
        }
        columnNotes = notes;
        relationships = rel.relationships;
      } catch (e) {
        if (mine !== seq) return;
        error = String(e);
        schema = null;
        relationships = [];
      } finally {
        if (mine === seq) loading = false;
      }
    })();
  });
</script>

<div class="panel">
  {#if !workspace.selectedTable}
    <div class="empty">
      <p>Select a table from the sidebar to inspect its structure.</p>
    </div>
  {:else}
    <header class="head">
      <h2 class="title">{tableKey(workspace.selectedTable)}</h2>
      {#if schema && schema.primary_key.length > 0}
        <span class="pk-summary">
          PK: {schema.primary_key.join(', ')}
        </span>
      {/if}
    </header>

    {#if tableNote}
      <p class="table-note">{tableNote}</p>
    {/if}

    {#if error}
      <p class="error">{error}</p>
    {:else if loading && !schema}
      <p class="hint">Loading…</p>
    {:else if schema}
      <div class="grid-wrap">
        <table>
          <thead>
            <tr>
              <th class="num">#</th>
              <th>Column</th>
              <th>Type</th>
              <th>Nullable</th>
              <th>Key</th>
              <th>Default</th>
              <th>Note</th>
            </tr>
          </thead>
          <tbody>
            {#each schema.columns as col (col.name)}
              <tr>
                <td class="num">{col.ordinal || ''}</td>
                <td class="col-name">{col.name}</td>
                <td class="mono muted">{col.declared_type ?? '—'}</td>
                <td class="muted">{col.nullable ? 'NULL' : 'NOT NULL'}</td>
                <td>
                  {#if col.primary_key}<span class="pk">PK</span>{/if}
                </td>
                <td class="mono muted">{col.default_value ?? '—'}</td>
                <td class="note">{columnNotes.get(col.name) ?? ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      {#if relationships.length > 0}
        <section class="rels">
          <h3 class="section-title">Relationships</h3>
          <ul class="rel-list">
            {#each relationships as r, i (r.constraint_name ?? i)}
              {@const outgoing = isCurrent(r.from_table)}
              <li class="rel">
                <span class="dir" class:out={outgoing} class:in={!outgoing}>
                  {outgoing ? 'FK →' : '← ref'}
                </span>
                <span class="mono">
                  <span class="tbl">{tableKey(r.from_table)}</span
                  >(<span class="cols">{r.from_columns.join(', ')}</span>)
                  →
                  <span class="tbl">{tableKey(r.to_table)}</span
                  >(<span class="cols">{r.to_columns.join(', ')}</span>)
                </span>
                {#if r.constraint_name}
                  <span class="constraint">{r.constraint_name}</span>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    {/if}
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

  .head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--text);
  }
  .pk-summary {
    font-size: var(--text-hint);
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .table-note {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    background: var(--bg-surface-alt);
    border-left: 2px solid var(--accent);
    border-radius: var(--radius-widget);
    color: var(--text);
    font-size: var(--text-small);
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
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
    vertical-align: top;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--bg-code);
    color: var(--text-muted);
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }
  tbody tr:nth-child(even) {
    background: var(--bg-surface-alt);
  }
  .num {
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    width: 1%;
  }
  .col-name {
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--text);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .muted {
    color: var(--text-muted);
  }
  .note {
    color: var(--text);
    white-space: normal;
    max-width: 320px;
  }
  .pk {
    font-size: var(--text-hint);
    font-weight: 700;
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: var(--radius-widget);
    padding: 0 6px;
  }

  .rels {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .section-title {
    margin: 0;
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .rel-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .rel {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
    padding: 5px var(--space-2);
    border-radius: var(--radius-widget);
    font-size: var(--text-small);
  }
  .rel:nth-child(even) {
    background: var(--bg-surface-alt);
  }
  .dir {
    flex: none;
    font-size: var(--text-hint);
    font-weight: 700;
    border-radius: var(--radius-widget);
    padding: 0 6px;
  }
  .dir.out {
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .dir.in {
    color: var(--text-muted);
    border: 1px solid var(--border);
  }
  .rel .tbl {
    color: var(--text);
    font-weight: 600;
  }
  .rel .cols {
    color: var(--text-accent);
  }
  .constraint {
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: var(--text-hint);
  }

  .hint,
  .error {
    margin: 0;
    font-size: var(--text-small);
  }
  .hint {
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
    font-family: var(--font-mono);
    white-space: pre-wrap;
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
</style>
