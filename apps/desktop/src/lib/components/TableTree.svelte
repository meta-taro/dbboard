<!-- The table list, grouped by the database (MySQL) or schema (Postgres) each
     table lives in (docs/every-database.md). One group is drawn flat, exactly as the list
     looked before: a parent with a single child is a click with nothing
     behind it.

     The row itself is the sidebar's, passed in as a snippet, so its look and
     its handlers stay in one place and this file only adds the levels. -->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { TableInfo } from '$lib/api';
  import { groupTables } from '$lib/sidebar/tree';

  interface Props {
    tables: TableInfo[];
    /** One table row. `qualified` is whether the row should name its own
     *  database or schema, which it must when there is no group header to. */
    row: Snippet<[TableInfo, boolean, string | null]>;
    /** The database these tables belong to (ADR-0162), passed back to `row`
     *  so a click knows where to point table-level calls. Null: the
     *  connection's own. */
    database?: string | null;
  }

  let { tables, row, database = null }: Props = $props();

  const groups = $derived(groupTables(tables));

  // Open by default: a Postgres connection with two schemas used to show
  // every table, and hiding them behind a click would be a step back.
  let collapsed = $state(new Set<string>());

  function toggle(name: string) {
    const next = new Set(collapsed);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    collapsed = next;
  }
</script>

{#if groups.length <= 1}
  {#each tables as t (`${t.schema ?? ''}.${t.name}`)}
    {@render row(t, true, database)}
  {/each}
{:else}
  {#each groups as g (g.name ?? '')}
    {@const key = g.name ?? ''}
    {@const open = !collapsed.has(key)}
    <button
      type="button"
      class="group"
      aria-expanded={open}
      onclick={() => toggle(key)}
      title={g.name ?? ''}
    >
      <span class="caret" class:open aria-hidden="true">▸</span>
      <span class="group-name">{g.name ?? '—'}</span>
      <span class="count">{g.tables.length}</span>
    </button>
    {#if open}
      <div class="members">
        {#each g.tables as t (`${key}.${t.name}`)}
          {@render row(t, false, database)}
        {/each}
      </div>
    {/if}
  {/each}
{/if}

<style>
  .group {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 8px;
    border: none;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: var(--text-small);
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-widget);
  }
  .group:hover {
    background: var(--bg-surface-alt);
  }
  .caret {
    display: inline-block;
    width: 10px;
    transition: transform 0.12s ease;
  }
  .caret.open {
    transform: rotate(90deg);
  }
  .group-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .count {
    font-variant-numeric: tabular-nums;
    opacity: 0.7;
  }
  .members {
    padding-left: 12px;
  }
</style>
