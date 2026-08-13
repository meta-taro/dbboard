<script lang="ts">
  // The window's bottom edge. It carries only what is not already on screen:
  // how long the last statement took (measured nowhere else), the running
  // version (otherwise buried in the About dialog), and a standing chip when a
  // newer release is waiting — dismissing the update card hides it for good,
  // and without this there would be no way back to it (ADR-0101).
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { runStatus } from '$lib/status/status.svelte';
  import { formatElapsed } from '$lib/status/summary';
  import { updateState } from '$lib/update/state.svelte';

  let version = $state('');

  onMount(async () => {
    try {
      version = await getVersion();
    } catch {
      // Outside a Tauri runtime (a plain browser preview) there is no app
      // version to read; the bar simply omits it.
    }
  });
</script>

<footer class="statusbar">
  <div class="left">
    {#if runStatus.running}
      <span class="run running">{i18n.t('status-query-running')}</span>
    {:else if runStatus.last}
      <span class="run" class:failed={runStatus.last.failed}>
        <span class="run-label"
          >{runStatus.last.failed
            ? i18n.t('status-query-failed')
            : i18n.t('status-query-done')}</span
        >
        <span class="elapsed">{formatElapsed(runStatus.last.elapsedMs)}</span>
      </span>
    {:else}
      <span class="run idle">{i18n.t('status-query-idle')}</span>
    {/if}
  </div>

  <div class="right">
    {#if updateState.available && updateState.dismissed}
      <button
        type="button"
        class="update-chip"
        onclick={() => updateState.reopen()}
        title={i18n.t('status-update-reopen')}
      >
        ↑ {i18n.t('status-update-available', {
          version: updateState.available.version,
        })}
      </button>
    {/if}
    {#if version}
      <span class="version">v{version}</span>
    {/if}
  </div>
</footer>

<style>
  .statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    height: 24px;
    padding: 0 var(--space-3);
    background: var(--bg-canvas);
    border-top: 1px solid var(--border);
    font-size: var(--text-hint);
    color: var(--text-muted);
    user-select: none;
    flex: none;
  }

  .left,
  .right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .run {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .run.idle,
  .run.running {
    color: var(--faint);
  }
  .run.failed .run-label {
    color: var(--danger);
  }

  .elapsed {
    font-family: var(--font-mono);
    color: var(--text);
  }

  /* Quiet by default: an available update is information, not an alarm. */
  .update-chip {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    color: var(--text-accent);
    font-size: var(--text-hint);
    font-weight: 600;
    line-height: 1;
    padding: 3px 10px;
    cursor: pointer;
  }
  .update-chip:hover {
    border-color: var(--accent);
  }

  .version {
    font-family: var(--font-mono);
    color: var(--faint);
  }
</style>
