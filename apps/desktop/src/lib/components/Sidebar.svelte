<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  import { searchSchema, type SchemaMatch, type TableInfo } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import ContextMenu, { type MenuItem } from './ContextMenu.svelte';
  import ConnectionManager from './ConnectionManager.svelte';
  import { tableMenuActions } from '$lib/sidebar/menu';
  import { connectionTooltip } from '$lib/connections/label';

  let query = $state('');
  let managerOpen = $state(false);
  let menu = $state<{ x: number; y: number; table: TableInfo } | null>(null);
  // null = not searching (show the full table list); an array = search results.
  let matches = $state<SchemaMatch[] | null>(null);
  let searching = $state(false);
  let searchError = $state('');
  let seq = 0;

  async function selectConnection(id: string) {
    if (id === workspace.connectionId) return;
    query = '';
    matches = null;
    await workspace.selectConnection(id);
  }

  // Debounced schema search. Blank query shows the plain list (the backend
  // rejects a blank pattern anyway); a token drops stale responses.
  $effect(() => {
    const q = query.trim();
    const connId = workspace.connectionId;
    if (!q || !connId) {
      matches = null;
      searchError = '';
      return;
    }
    const mine = ++seq;
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const view = await searchSchema(connId, q);
        if (mine !== seq) return;
        matches = view.matches;
        searchError = '';
      } catch (err) {
        if (mine !== seq) return;
        matches = [];
        searchError = String(err);
      } finally {
        if (mine === seq) searching = false;
      }
    }, 220);
    return () => clearTimeout(timer);
  });

  function isSelected(t: TableInfo): boolean {
    return (
      !!workspace.selectedTable &&
      workspace.key(workspace.selectedTable) === workspace.key(t)
    );
  }

  function openMenu(e: MouseEvent, table: TableInfo) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, table };
  }

  // Read-only actions only: inspect, or generate a bounded SELECT to run.
  // Which actions exist and what SQL each generates lives in $lib/sidebar/menu
  // (unit-tested); this only attaches labels and side effects.
  function menuItems(table: TableInfo): MenuItem[] {
    return tableMenuActions(table, workspace.connection?.kind).map((action) => {
      switch (action.id) {
        case 'open-structure':
          return {
            label: i18n.t('menu-open-structure'),
            onSelect: () => workspace.selectTable(table),
          };
        case 'select-top':
          return {
            label: i18n.t('menu-select-top', { n: action.n }),
            onSelect: () => workspace.browse(table),
          };
        case 'count-rows':
          return {
            label: i18n.t('menu-count-rows'),
            onSelect: () => workspace.runInEditor(action.sql),
          };
        case 'copy-name':
          return {
            label: i18n.t('menu-copy-name'),
            separatorBefore: true,
            onSelect: () => navigator.clipboard.writeText(action.text),
          };
      }
    });
  }
</script>

{#snippet dbIcon()}
  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
    <ellipse cx="8" cy="3.5" rx="5" ry="2" />
    <path d="M3 3.5v9c0 1.1 2.24 2 5 2s5-.9 5-2v-9" />
    <path d="M3 8c0 1.1 2.24 2 5 2s5-.9 5-2" />
  </svg>
{/snippet}

{#snippet tableIcon()}
  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
    <rect x="2" y="3" width="12" height="10" rx="1.5" />
    <line x1="2" y1="6.5" x2="14" y2="6.5" />
    <line x1="6.5" y1="6.5" x2="6.5" y2="13" />
  </svg>
{/snippet}

<aside class="sidebar">
  <div class="section">
    <div class="section-head">
      <div class="eyebrow">{i18n.t('connections-window-title')}</div>
      <button type="button" class="manage" onclick={() => (managerOpen = true)}>
        {i18n.t('conn-manage')}
      </button>
    </div>
    <nav class="nav-list" aria-label={i18n.t('connections-window-title')}>
      {#if workspace.connections.length === 0}
        <p class="hint">{i18n.t('sidebar-connections-empty')}</p>
      {:else}
        {#each workspace.connections as c (c.id)}
          <button
            type="button"
            class="nav-row conn"
            class:active={workspace.connectionId === c.id}
            onclick={() => selectConnection(c.id)}
            title={connectionTooltip(c)}
          >
            {@render dbIcon()}
            <span class="nav-name">{c.name}</span>
            <span class="nav-meta">{c.kind}</span>
          </button>
        {/each}
      {/if}
    </nav>
  </div>

  <div class="section tables">
    <div class="search">
      <input
        type="search"
        placeholder={i18n.t('sidebar-search-placeholder')}
        bind:value={query}
        disabled={!workspace.connectionId}
        spellcheck="false"
      />
    </div>

    {#if matches === null}
      <!-- Normal browse mode: the full table list. -->
      <div class="heading">
        <span class="eyebrow">{i18n.t('tables-heading')}</span>
        {#if workspace.tables.length > 0}
          <span class="badge">{workspace.tables.length}</span>
        {/if}
      </div>
      <div class="list">
        {#if workspace.loadingTables}
          <p class="hint">{i18n.t('sidebar-loading')}</p>
        {:else if workspace.tables.length === 0}
          <p class="hint">{i18n.t('sidebar-tables-empty')}</p>
        {:else}
          {#each workspace.tables as t (workspace.key(t))}
            <button
              type="button"
              class="nav-row"
              class:active={isSelected(t)}
              onclick={() => workspace.selectTable(t)}
              oncontextmenu={(e) => openMenu(e, t)}
              title={workspace.key(t)}
            >
              {@render tableIcon()}
              <span class="nav-name">
                {#if t.schema}<span class="schema">{t.schema}.</span>{/if}{t.name}
              </span>
            </button>
          {/each}
        {/if}
      </div>
    {:else}
      <!-- Search mode. -->
      <div class="heading">
        <span class="eyebrow">{i18n.t('sidebar-matches')}</span>
        {#if !searching}<span class="badge">{matches.length}</span>{/if}
      </div>
      <div class="list">
        {#if searching}
          <p class="hint">{i18n.t('sidebar-searching')}</p>
        {:else if searchError}
          <p class="error">{searchError}</p>
        {:else if matches.length === 0}
          <p class="hint">{i18n.t('sidebar-no-matches')}</p>
        {:else}
          {#each matches as m (workspace.key(m.table))}
            <button
              type="button"
              class="nav-row match"
              class:active={isSelected(m.table)}
              onclick={() => workspace.selectTable(m.table)}
              oncontextmenu={(e) => openMenu(e, m.table)}
              title={workspace.key(m.table)}
            >
              <span class="match-name">
                {@render tableIcon()}
                <span class="nav-name">
                  {#if m.table.schema}<span class="schema"
                      >{m.table.schema}.</span
                    >{/if}{m.table.name}
                </span>
                {#if !m.table_name_matched}<span class="via">col</span>{/if}
              </span>
              {#if m.matched_columns.length > 0}
                <span class="cols">
                  {#each m.matched_columns as c (c.name)}
                    <span class="col-chip">{c.name}</span>
                  {/each}
                </span>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
</aside>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    items={menuItems(menu.table)}
    onClose={() => (menu = null)}
  />
{/if}

{#if managerOpen}
  <ConnectionManager onClose={() => (managerOpen = false)} />
{/if}

<style>
  /* Width is driven by the shell's splitter (ADR-0083); the fallback keeps the
     sidebar usable if this component is ever mounted outside that shell. */
  .sidebar {
    width: var(--sidebar-width, 260px);
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-surface);
    border-right: 1px solid var(--border);
  }

  .section {
    padding: var(--space-3);
  }
  .section + .section {
    border-top: 1px solid var(--border);
  }
  .tables {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding-bottom: var(--space-2);
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  /* Quiet text trigger, aligned with the section eyebrow. */
  .manage {
    background: transparent;
    border: none;
    color: var(--text-accent);
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: var(--radius-widget);
  }
  .manage:hover {
    background: var(--bg-surface-alt);
  }

  /* Uppercase section label — one step quieter than muted body text. */
  .eyebrow {
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--faint);
  }

  input[type='search'] {
    width: 100%;
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 6px 8px;
    font-size: var(--text-body);
  }
  input[type='search']:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  input[type='search']:disabled {
    opacity: 0.6;
  }
  input[type='search']:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  .search {
    margin-bottom: var(--space-3);
  }

  .nav-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin-top: var(--space-2);
  }

  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }
  .badge {
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-accent);
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 0 8px;
    min-width: 20px;
    text-align: center;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  /* Shared navi row: icon + name (+ optional right-aligned meta). Active state
     is an accent-weak fill with a 2px inset accent bar down the left edge. */
  .nav-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--text-body);
    padding: 6px 8px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .nav-row:hover {
    background: var(--bg-surface-alt);
  }
  .nav-row.active {
    background: var(--accent-weak);
    color: var(--text-accent);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .icon {
    flex: none;
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
    stroke-linejoin: round;
    color: var(--faint);
  }
  .nav-row.active .icon {
    color: var(--accent);
  }

  .nav-name {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nav-meta {
    flex: none;
    font-size: var(--text-hint);
    color: var(--faint);
    text-transform: capitalize;
  }
  .schema {
    color: var(--text-muted);
  }

  /* A search hit can wrap onto a second line for matched columns. */
  .match {
    flex-direction: column;
    align-items: stretch;
  }
  .match-name {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .via {
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 0 4px;
  }
  .cols {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 4px 0 0 23px;
  }
  .col-chip {
    font-size: var(--text-hint);
    font-family: var(--font-mono);
    color: var(--text-muted);
    background: var(--bg-surface-alt);
    border-radius: var(--radius-widget);
    padding: 0 6px;
  }

  .hint,
  .error {
    margin: var(--space-2) 0 0;
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
</style>
