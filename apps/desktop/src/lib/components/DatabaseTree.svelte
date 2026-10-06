<!-- The database level above the tables (ADR-0162, docs/every-database.md).
     Shown only for a connection saved without a database whose engine can see
     several; every other connection keeps the table list it always had.

     A database's tables load the first time it is opened, because opening is
     what makes the backend connect to it. Inside, the tables are grouped by
     schema exactly as a single-database connection's are: this component adds
     one level and reuses TableTree for the rest. -->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { TableInfo } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { workspace } from '$lib/state/workspace.svelte';
  import TableTree from './TableTree.svelte';

  interface Props {
    /** The sidebar's table row; see TableTree. */
    row: Snippet<[TableInfo, boolean, string | null]>;
  }

  let { row }: Props = $props();

  // Closed by default, unlike schema groups: an open database is a live
  // connection, and a server can list dozens.
  let open = $state(new Set<string>());

  function toggle(database: string) {
    const next = new Set(open);
    if (next.has(database)) {
      next.delete(database);
    } else {
      next.add(database);
      void workspace.openDatabase(database);
    }
    open = next;
  }
</script>

{#each workspace.databases as database (database)}
  {@const isOpen = open.has(database)}
  {@const tables = workspace.databaseTables[database]}
  <button
    type="button"
    class="database"
    class:current={workspace.database === database}
    aria-expanded={isOpen}
    onclick={() => toggle(database)}
    title={database}
  >
    <span class="caret" class:open={isOpen} aria-hidden="true">▸</span>
    <span class="database-name">{database}</span>
    {#if tables}<span class="count">{tables.length}</span>{/if}
  </button>
  {#if isOpen}
    <div class="members">
      {#if workspace.loadingDatabases[database]}
        <p class="hint">{i18n.t('sidebar-loading')}</p>
      {:else if tables && tables.length === 0}
        <p class="hint">{i18n.t('sidebar-tables-empty')}</p>
      {:else if tables}
        <TableTree {tables} {row} {database} />
      {/if}
    </div>
  {/if}
{/each}

<style>
  .database {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 5px 8px;
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--text-body);
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-widget);
  }
  .database:hover {
    background: var(--bg-surface-alt);
  }
  .database.current .database-name {
    color: var(--text-accent);
  }
  .caret {
    display: inline-block;
    width: 10px;
    color: var(--text-muted);
    transition: transform 0.12s ease;
  }
  .caret.open {
    transform: rotate(90deg);
  }
  .database-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .count {
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .members {
    padding-left: 12px;
  }
  .hint {
    margin: 4px 8px;
    color: var(--text-hint);
    font-size: var(--text-small);
  }
</style>
