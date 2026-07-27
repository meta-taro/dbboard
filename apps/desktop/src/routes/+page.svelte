<script lang="ts">
  import { onMount } from 'svelte';
  import { workspace, type MainTab } from '$lib/state/workspace.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import QueryPanel from '$lib/components/QueryPanel.svelte';
  import StructurePanel from '$lib/components/StructurePanel.svelte';

  const tabs: { id: MainTab; label: string }[] = [
    { id: 'query', label: 'Query' },
    { id: 'structure', label: 'Structure' },
  ];

  onMount(() => workspace.init());
</script>

<div class="shell">
  <Sidebar />

  <main class="main">
    <nav class="tabbar" aria-label="View">
      {#each tabs as t (t.id)}
        <button
          type="button"
          class="tab"
          class:active={workspace.activeTab === t.id}
          aria-current={workspace.activeTab === t.id ? 'page' : undefined}
          onclick={() => workspace.setTab(t.id)}
        >
          {t.label}
        </button>
      {/each}

      {#if workspace.connection}
        <span class="conn-pill" title={workspace.connection.id}>
          <span class="dot" aria-hidden="true"></span>
          {workspace.connection.name}
        </span>
      {/if}
    </nav>

    {#if workspace.error}
      <p class="shell-error">{workspace.error}</p>
    {/if}

    <!-- Both panels stay mounted; hiding (not unmounting) preserves the SQL
         you typed and the last result when you flip tabs. -->
    <div class="content">
      <div class="tabpane" hidden={workspace.activeTab !== 'query'}>
        <QueryPanel />
      </div>
      <div class="tabpane" hidden={workspace.activeTab !== 'structure'}>
        <StructurePanel />
      </div>
    </div>
  </main>
</div>

<style>
  .shell {
    display: flex;
    height: 100%;
    min-height: 0;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .tabbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--bg-surface);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  .tab {
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-body);
    font-weight: 500;
    padding: 5px var(--space-3);
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
    background: var(--bg-surface-alt);
  }
  /* Selected tab is an accent-weak fill ringed by the accent, matching the
     mock — a filled chip rather than an underline. */
  .tab.active {
    color: var(--text-accent);
    background: var(--accent-weak);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .tab.active:hover {
    background: var(--accent-weak);
  }

  .conn-pill {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-hint);
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 2px 10px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: var(--radius-pill);
    background: var(--success);
    flex: none;
  }

  .shell-error {
    margin: 0;
    padding: var(--space-2) var(--space-4);
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-bottom: 1px solid var(--border);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }

  .content {
    flex: 1;
    min-height: 0;
    position: relative;
  }

  /* Each pane owns the full content box and its own scroll. */
  .tabpane {
    position: absolute;
    inset: 0;
    overflow: auto;
  }
  .tabpane[hidden] {
    display: none;
  }
</style>
