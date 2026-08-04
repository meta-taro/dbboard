<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { save } from '@tauri-apps/plugin-dialog';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { planDump, runDump, cancelDump, onDumpProgress } from '$lib/api';
  import {
    loadWarnThreshold,
    exceedsThreshold,
    progressPercent,
    defaultDumpFileName,
    type DumpPlan,
    type DumpOutcome,
    type DumpProgress,
  } from '$lib/backup/plan';

  interface Props {
    connectionId: string;
    connectionName: string;
    onClose: () => void;
  }
  let { connectionId, connectionName, onClose }: Props = $props();

  type Phase = 'planning' | 'ready' | 'running' | 'done' | 'error';
  let phase = $state<Phase>('planning');
  let plan = $state<DumpPlan | null>(null);
  let progress = $state<DumpProgress | null>(null);
  let outcome = $state<DumpOutcome | null>(null);
  let errorMsg = $state('');
  let cancelling = $state(false);

  const threshold = loadWarnThreshold();
  let unlisten: UnlistenFn | null = null;

  const percent = $derived(progress ? progressPercent(progress) : 0);
  const isLarge = $derived(!!plan && exceedsThreshold(plan, threshold));

  onMount(async () => {
    try {
      plan = await planDump(connectionId);
      phase = 'ready';
    } catch (e) {
      errorMsg = String(e);
      phase = 'error';
    }
  });

  onDestroy(() => {
    // Detach the progress listener if the dialog closes mid-run; the backend
    // run itself is unaffected (it finishes into the void, as designed).
    unlisten?.();
  });

  async function runBackup(): Promise<void> {
    const path = await save({
      defaultPath: defaultDumpFileName(connectionName),
      filters: [{ name: 'SQL', extensions: ['sql'] }],
    });
    if (!path) return; // user cancelled the save dialog

    phase = 'running';
    cancelling = false;
    progress = null;
    errorMsg = '';
    unlisten = await onDumpProgress((p) => {
      progress = p;
    });

    try {
      outcome = await runDump(connectionId, path);
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
      await cancelDump();
    } catch {
      // A failed cancel just means the run already finished; the outcome
      // resolves normally either way.
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    // Esc closes only when a dump is not actively running, so a stray keypress
    // can't orphan an in-flight backup.
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
  <div class="dialog" role="dialog" aria-modal="true" aria-label={i18n.t('backup-title')}>
    <header class="head">
      <h2 class="title">{i18n.t('backup-title')}</h2>
      <span class="conn">{connectionName}</span>
    </header>

    {#if phase === 'planning'}
      <p class="status">{i18n.t('backup-planning')}</p>
    {:else if phase === 'ready' && plan}
      <p class="summary">
        {i18n.t('backup-summary', { tables: plan.tables.length, rows: plan.total_rows })}
      </p>
      {#if plan.is_empty_data}
        <p class="note">{i18n.t('backup-empty-data')}</p>
      {/if}
      {#if isLarge}
        <p class="warn" role="alert">
          {i18n.t('backup-warn-large', { rows: plan.total_rows, threshold })}
        </p>
      {/if}
      <p class="note">{i18n.t('backup-note')}</p>
    {:else if phase === 'running'}
      <div class="progress" role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
        <div class="bar" style="width: {percent}%"></div>
      </div>
      <p class="status mono">
        {#if progress}
          {i18n.t('backup-progress', {
            done: progress.rows_done,
            total: progress.rows_total,
            table: progress.current_table ?? '—',
          })}
        {:else}
          {i18n.t('backup-running')}
        {/if}
      </p>
    {:else if phase === 'done' && outcome}
      {#if outcome.cancelled}
        <p class="status" role="status">
          {i18n.t('backup-cancelled', { rows: outcome.rows_written })}
        </p>
      {:else}
        <p class="status ok" role="status">
          {i18n.t('backup-done', { tables: outcome.tables_dumped, rows: outcome.rows_written })}
        </p>
      {/if}
      {#if outcome.failures.length > 0}
        <p class="warn">{i18n.t('backup-failures', { count: outcome.failures.length })}</p>
      {/if}
      {#if outcome.truncations.length > 0}
        <p class="warn">{i18n.t('backup-truncations', { count: outcome.truncations.length })}</p>
      {/if}
    {:else if phase === 'error'}
      <p class="error" role="alert">{i18n.t('backup-failed')}: {errorMsg}</p>
    {/if}

    <div class="actions">
      {#if phase === 'ready'}
        <button type="button" class="ghost" onclick={onClose}>{i18n.t('backup-close')}</button>
        <button type="button" class="primary" onclick={runBackup}>{i18n.t('backup-run')}</button>
      {:else if phase === 'running'}
        <button type="button" class="ghost" onclick={requestCancel} disabled={cancelling}>
          {cancelling ? i18n.t('backup-cancelling') : i18n.t('backup-cancel')}
        </button>
      {:else}
        <button type="button" class="primary" onclick={onClose}>{i18n.t('backup-close')}</button>
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
