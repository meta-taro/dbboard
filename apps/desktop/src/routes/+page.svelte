<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { workspace, type MainTab } from '$lib/state/workspace.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import QueryPanel from '$lib/components/QueryPanel.svelte';
  import StructurePanel from '$lib/components/StructurePanel.svelte';
  import BackupDialog from '$lib/components/BackupDialog.svelte';
  import RestoreDialog from '$lib/components/RestoreDialog.svelte';
  import AiPanel from '$lib/components/AiPanel.svelte';
  import UpdateNotice from '$lib/components/UpdateNotice.svelte';
  import {
    updateOptOut,
    checkForUpdate,
    onUiCommand,
    reportUiCommandResult,
  } from '$lib/api';
  import { uiCommands } from '$lib/ui-command/bus';
  import { attachUiCommands, type Detach } from '$lib/ui-command/channel';
  import { updateState } from '$lib/update/state.svelte';
  import {
    SIDEBAR_DEFAULT_WIDTH,
    clampSidebarWidth,
    loadSidebarWidth,
    saveSidebarWidth,
    resetSidebarWidth,
  } from '$lib/layout/splitter';

  const tabs: { id: MainTab; labelKey: MessageKey }[] = [
    { id: 'query', labelKey: 'tab-query' },
    { id: 'structure', labelKey: 'tab-structure' },
  ];

  /** Detaches the `ui:locale` subscription when the shell goes away. */
  let unlistenLocale: UnlistenFn | null = null;
  /** The same, for `ui:command` (ADR-0109). */
  let unlistenCommand: Detach | null = null;
  /** Gives up this component's claim on `open_ai_panel`. */
  let releaseAiPanel: Detach | null = null;

  let backupOpen = $state(false);
  let restoreOpen = $state(false);
  let aiOpen = $state(false);

  // The width the user asked for, kept unclamped: narrowing the window squeezes
  // the sidebar (see `sidebarWidth`) but must not forget the chosen width, so
  // widening the window again restores it.
  let chosenWidth = $state(SIDEBAR_DEFAULT_WIDTH);
  let viewportWidth = $state(Number.POSITIVE_INFINITY);
  let dragging = $state(false);
  let shellEl = $state<HTMLDivElement | null>(null);

  const sidebarWidth = $derived(clampSidebarWidth(chosenWidth, viewportWidth));

  /** How far one arrow-key press moves the divider. */
  const NUDGE = 16;

  onMount(() => {
    i18n.init();
    // Then follow `ui-settings.toml`, which may name a different language and
    // can change while the window is open (ADR-0041). Async, so the UI paints
    // in the locally resolved locale first rather than waiting on IPC.
    void i18n.sync().then((un) => {
      unlistenLocale = un;
    });
    // The shell owns the AI panel, so it is the shell that can open it on
    // request (ADR-0109). The query verbs are claimed by the query panel.
    releaseAiPanel = uiCommands.on('open_ai_panel', async () => {
      if (aiOpen) return 'the AI panel was already open';
      aiOpen = true;
      return 'AI panel opened';
    });
    void attachUiCommands({
      subscribe: onUiCommand,
      dispatch: (command) => uiCommands.dispatch(command),
      report: reportUiCommandResult,
    }).then((un) => {
      unlistenCommand = un;
    });
    workspace.init();
    chosenWidth = loadSidebarWidth();
    viewportWidth = window.innerWidth;
    void maybeCheckForUpdate();
  });

  onDestroy(() => {
    unlistenLocale?.();
    unlistenLocale = null;
    unlistenCommand?.();
    unlistenCommand = null;
    releaseAiPanel?.();
    releaseAiPanel = null;
  });

  function widthAt(clientX: number): number {
    const left = shellEl?.getBoundingClientRect().left ?? 0;
    return clampSidebarWidth(clientX - left, viewportWidth);
  }

  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    dragging = true;
    // Capture keeps the drag alive when the pointer outruns the 7px handle,
    // which it always does.
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onDrag(e: PointerEvent) {
    if (!dragging) return;
    chosenWidth = widthAt(e.clientX);
  }

  function endDrag(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    saveSidebarWidth(chosenWidth);
  }

  /** Double-click puts the divider back where it started. */
  function resetDivider() {
    chosenWidth = resetSidebarWidth();
  }

  function onDividerKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      chosenWidth = clampSidebarWidth(
        sidebarWidth + (e.key === 'ArrowLeft' ? -NUDGE : NUDGE),
        viewportWidth,
      );
      saveSidebarWidth(chosenWidth);
    } else if (e.key === 'Home') {
      resetDivider();
    } else {
      return;
    }
    e.preventDefault();
  }

  // Best-effort startup update check (ADR-0067). Honours the same
  // DBBOARD_NO_UPDATE_CHECK opt-out as the egui client, and swallows every
  // failure: an update check must never be able to break the app's launch.
  async function maybeCheckForUpdate() {
    try {
      if (await updateOptOut()) return;
      updateState.set(await checkForUpdate());
    } catch {
      updateState.set(null);
    }
  }
</script>

<svelte:window onresize={() => (viewportWidth = window.innerWidth)} />

<div class="shell" bind:this={shellEl} style="--sidebar-width: {sidebarWidth}px">
  <Sidebar />

  <!-- A focusable window splitter is the ARIA-sanctioned use of role=separator
       (it takes aria-valuenow and arrow keys); the linter only knows the static
       separator, which is indeed non-interactive. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="divider"
    class:dragging
    role="separator"
    aria-orientation="vertical"
    aria-label={i18n.t('sidebar-resize')}
    aria-valuenow={sidebarWidth}
    title={i18n.t('sidebar-resize')}
    tabindex="0"
    onpointerdown={startDrag}
    onpointermove={onDrag}
    onpointerup={endDrag}
    onpointercancel={endDrag}
    ondblclick={resetDivider}
    onkeydown={onDividerKeydown}
  ></div>

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
          {i18n.t(t.labelKey)}
        </button>
      {/each}

      <!-- Right-side tool group. The AI button is always present so a first
           provider can be added before any connection exists; the backup /
           restore actions and the connection pill only appear with a
           connection. -->
      <div class="tools">
        <button
          type="button"
          class="tool-btn"
          onclick={() => (aiOpen = true)}
          title={i18n.t('ai-button-title')}
        >
          {i18n.t('ai-button')}
        </button>
        {#if workspace.connection}
          <button
            type="button"
            class="tool-btn"
            onclick={() => (backupOpen = true)}
            title={i18n.t('backup-button-title')}
          >
            {i18n.t('backup-button')}
          </button>
          <button
            type="button"
            class="tool-btn"
            onclick={() => (restoreOpen = true)}
            title={i18n.t('restore-button-title')}
          >
            {i18n.t('restore-button')}
          </button>
          <span class="conn-pill" title={workspace.connection.id}>
            <span class="dot" aria-hidden="true"></span>
            {workspace.connection.name}
            <button
              type="button"
              class="pill-reload"
              onclick={() => workspace.reconnect()}
              disabled={workspace.reconnecting}
              title={i18n.t('reconnect-button-title')}
              aria-label={i18n.t('reconnect-button-title')}
            >
              <svg class="reload-icon" viewBox="0 0 16 16" aria-hidden="true">
                <path d="M13.5 8a5.5 5.5 0 1 1-1.61-3.89" />
                <path d="M13.5 2v3.5H10" />
              </svg>
            </button>
          </span>
        {/if}
      </div>
    </nav>

    <!-- A failed load is very often a connection that died out from under us
         (a dropped SSH tunnel, a suspended laptop), so the banner carries the
         one action that fixes it rather than making you hunt for it. -->
    {#if workspace.error}
      <p class="shell-error">
        <span class="shell-error-text">{workspace.error}</span>
        {#if workspace.connectionId}
          <button
            type="button"
            class="shell-error-action"
            onclick={() => workspace.reconnect()}
            disabled={workspace.reconnecting}
          >
            {workspace.reconnecting
              ? i18n.t('reconnect-busy')
              : i18n.t('reconnect-error-action')}
          </button>
        {/if}
      </p>
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

{#if backupOpen && workspace.connection}
  <BackupDialog
    connectionId={workspace.connection.id}
    connectionName={workspace.connection.name}
    onClose={() => (backupOpen = false)}
  />
{/if}

{#if restoreOpen && workspace.connection}
  <RestoreDialog
    connectionId={workspace.connection.id}
    connectionName={workspace.connection.name}
    onClose={() => (restoreOpen = false)}
  />
{/if}

{#if aiOpen}
  <AiPanel
    connectionId={workspace.connection?.id ?? null}
    onClose={() => (aiOpen = false)}
  />
{/if}

{#if updateState.showNotice && updateState.available}
  <UpdateNotice
    update={updateState.available}
    onDismiss={() => updateState.dismiss()}
  />
{/if}

<style>
  .shell {
    display: flex;
    height: 100%;
    min-height: 0;
  }

  /* The grab area is wider than the line it draws: a 1px border is a hard
     target with a mouse, so the handle straddles the sidebar's edge and the
     visible rule stays hairline-thin. */
  .divider {
    flex: none;
    width: 7px;
    margin: 0 -3px 0 -4px;
    z-index: 3;
    cursor: col-resize;
    background: transparent;
    touch-action: none;
  }
  .divider:hover,
  .divider:focus-visible,
  .divider.dragging {
    background: color-mix(in srgb, var(--accent) 45%, transparent);
    outline: none;
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

  /* The right-side tool group hugs the tabbar's right edge; its items sit
     snugly beside each other with a shared gap. */
  .tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  /* Quiet ghost actions in the tool group. */
  .tool-btn {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-hint);
    font-weight: 500;
    padding: 4px 12px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .tool-btn:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }

  .conn-pill {
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

  .pill-reload {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin: 0 -4px 0 2px;
    padding: 2px;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    border-radius: var(--radius-pill);
  }
  .pill-reload:hover:not(:disabled) {
    color: var(--text);
    background: var(--bg-surface-alt);
  }
  .pill-reload:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .reload-icon {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .shell-error {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    margin: 0;
    padding: var(--space-2) var(--space-4);
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    border-bottom: 1px solid var(--border);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }
  .shell-error-text {
    flex: 1;
    min-width: 0;
  }
  .shell-error-action {
    flex: none;
    font: inherit;
    color: inherit;
    background: none;
    border: 1px solid currentColor;
    border-radius: var(--radius-widget);
    padding: 1px 8px;
    cursor: pointer;
  }
  .shell-error-action:hover:not(:disabled) {
    background: var(--danger-weak);
  }
  .shell-error-action:disabled {
    opacity: 0.6;
    cursor: default;
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
