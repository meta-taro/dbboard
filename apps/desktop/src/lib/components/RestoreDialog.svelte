<script lang="ts">
  import { onDestroy } from 'svelte';
  import { open } from '@tauri-apps/plugin-dialog';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import {
    planRestore,
    runRestore,
    cancelRestore,
    onRestoreProgress,
  } from '$lib/api';
  import {
    restoreProgressPercent,
    needsConfirmation,
    hasUnparsed,
    restoreFileFilters,
    type RestorePlan,
    type RestoreOutcome,
    type RestoreProgress,
    type OnError,
  } from '$lib/restore/plan';

  interface Props {
    connectionId: string;
    connectionName: string;
    onClose: () => void;
  }
  let { connectionId, connectionName, onClose }: Props = $props();

  // 'idle' waits for a file; a restore has no source until the user picks one,
  // so unlike the dump we cannot preflight on mount.
  type Phase = 'idle' | 'planning' | 'ready' | 'running' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let path = $state('');
  let plan = $state<RestorePlan | null>(null);
  let progress = $state<RestoreProgress | null>(null);
  let outcome = $state<RestoreOutcome | null>(null);
  let errorMsg = $state('');
  let cancelling = $state(false);
  let onError = $state<OnError>('stop');
  // Gate for a non-empty target: the run stays disabled until this is checked.
  let confirmed = $state(false);

  let unlisten: UnlistenFn | null = null;

  const percent = $derived(progress ? restoreProgressPercent(progress) : 0);
  const mustConfirm = $derived(!!plan && needsConfirmation(plan));
  const canRun = $derived(!!plan && (!mustConfirm || confirmed));

  onDestroy(() => {
    // Detach the progress listener if the dialog closes mid-run; the backend
    // run itself is unaffected (it finishes into the void, as designed).
    unlisten?.();
  });

  async function chooseFile(): Promise<void> {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: restoreFileFilters(),
    });
    if (typeof picked !== 'string') return; // cancelled (or multi, never here)

    path = picked;
    phase = 'planning';
    plan = null;
    confirmed = false;
    errorMsg = '';
    try {
      plan = await planRestore(connectionId, picked);
      phase = 'ready';
    } catch (e) {
      errorMsg = String(e);
      phase = 'error';
    }
  }

  async function runImport(): Promise<void> {
    if (!canRun) return;

    phase = 'running';
    cancelling = false;
    progress = null;
    outcome = null;
    errorMsg = '';
    unlisten = await onRestoreProgress((p) => {
      progress = p;
    });

    try {
      outcome = await runRestore(connectionId, path, confirmed, onError);
      phase = 'done';
    } catch (e) {
      errorMsg = String(e);
      phase = 'error';
    } finally {
      unlisten?.();
      unlisten = null;
    }
  }

  async function requestCancel(): Promise<void> {
    cancelling = true;
    try {
      await cancelRestore();
    } catch {
      // A failed cancel just means the run already finished; the outcome
      // resolves normally either way.
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    // Esc closes only when a restore is not actively running, so a stray
    // keypress can't orphan an in-flight import.
    if (e.key === 'Escape' && phase !== 'running') onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget && phase !== 'running') onClose();
  }}
  role="presentation"
>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={i18n.t('restore-title')}>
    <header class="head">
      <h2 class="title">{i18n.t('restore-title')}</h2>
      <span class="conn">{connectionName}</span>
    </header>

    {#if phase === 'idle'}
      <p class="note">{i18n.t('restore-note')}</p>
    {:else if phase === 'planning'}
      <p class="status">{i18n.t('restore-planning')}</p>
    {:else if phase === 'ready' && plan}
      <p class="summary">
        {i18n.t('restore-summary', {
          statements: plan.statements_total,
          ddl: plan.ddl_count,
          data: plan.data_count,
        })}
      </p>
      {#if hasUnparsed(plan)}
        <p class="warn" role="alert">
          {i18n.t('restore-unparsed-warn', { count: plan.unparsed_count })}
        </p>
      {/if}
      {#if mustConfirm}
        <p class="warn" role="alert">
          {i18n.t('restore-nonempty-warn', { tables: plan.existing_tables.length })}
        </p>
        <label class="confirm">
          <input type="checkbox" bind:checked={confirmed} />
          {i18n.t('restore-confirm-label')}
        </label>
      {/if}
      <label class="onerror">
        <span class="onerror-label">{i18n.t('restore-onerror-label')}</span>
        <select bind:value={onError}>
          <option value="stop">{i18n.t('restore-onerror-stop')}</option>
          <option value="continue">{i18n.t('restore-onerror-continue')}</option>
        </select>
      </label>
      <p class="note">{i18n.t('restore-note')}</p>
    {:else if phase === 'running'}
      <div
        class="progress"
        role="progressbar"
        aria-valuenow={percent}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div class="bar" style="width: {percent}%"></div>
      </div>
      <p class="status mono">
        {#if progress}
          {i18n.t('restore-progress', {
            done: progress.statements_done,
            total: progress.statements_total,
          })}
        {:else}
          {i18n.t('restore-running')}
        {/if}
      </p>
    {:else if phase === 'done' && outcome}
      {#if outcome.cancelled}
        <p class="status" role="status">
          {i18n.t('restore-cancelled', { statements: outcome.statements_run })}
        </p>
      {:else}
        <p class="status ok" role="status">
          {i18n.t(outcome.atomic ? 'restore-done-atomic' : 'restore-done', {
            statements: outcome.statements_run,
          })}
        </p>
      {/if}
      {#if outcome.failures.length > 0}
        <p class="warn">{i18n.t('restore-failures', { count: outcome.failures.length })}</p>
      {/if}
    {:else if phase === 'error'}
      <p class="error" role="alert">{i18n.t('restore-failed')}: {errorMsg}</p>
    {/if}

    <div class="actions">
      {#if phase === 'idle'}
        <button type="button" class="ghost" onclick={onClose}>{i18n.t('restore-close')}</button>
        <button type="button" class="primary" onclick={chooseFile}>{i18n.t('restore-choose')}</button>
      {:else if phase === 'ready'}
        <button type="button" class="ghost" onclick={onClose}>{i18n.t('restore-close')}</button>
        <button type="button" class="primary" onclick={runImport} disabled={!canRun}>
          {i18n.t('restore-run')}
        </button>
      {:else if phase === 'running'}
        <button type="button" class="ghost" onclick={requestCancel} disabled={cancelling}>
          {cancelling ? i18n.t('restore-cancelling') : i18n.t('restore-cancel')}
        </button>
      {:else}
        <button type="button" class="primary" onclick={onClose}>{i18n.t('restore-close')}</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: 50;
  }
  .dialog {
    width: min(460px, 92vw);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    color: var(--text);
  }
  .conn {
    font-size: var(--text-hint);
    color: var(--text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary {
    margin: 0;
    color: var(--text);
    font-size: var(--text-body);
  }
  .status {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .status.ok {
    color: var(--success);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .note {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-hint);
    line-height: 1.5;
  }
  .warn {
    margin: 0;
    color: var(--warning, #b7791f);
    background: color-mix(in srgb, var(--warning, #b7791f) 10%, transparent);
    border-radius: var(--radius-widget);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .confirm {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    color: var(--text);
    font-size: var(--text-small);
    line-height: 1.4;
    cursor: pointer;
  }
  .confirm input {
    margin-top: 2px;
    flex: none;
  }

  .onerror {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .onerror-label {
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .onerror select {
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 4px 8px;
    font-size: var(--text-small);
  }

  .progress {
    height: 8px;
    background: var(--bg-surface-alt);
    border-radius: var(--radius-pill);
    overflow: hidden;
  }
  .bar {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s ease;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .primary {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
    border: none;
    border-radius: var(--radius-widget);
    padding: 7px 20px;
    cursor: pointer;
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ghost {
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 7px 16px;
    cursor: pointer;
  }
  .ghost:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .ghost:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
