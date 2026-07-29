<script lang="ts">
  // Auto-update notice (ADR-0067). A non-modal card in the bottom corner: it
  // surfaces a newer signed release, then downloads + installs it in place and
  // relaunches — going one step past the egui client, which only informs
  // (ADR-0040). Kept out of the way (never a modal) so it can't block work.
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { installUpdate, relaunchApp } from '$lib/api';
  import {
    emptyDownload,
    foldDownload,
    downloadPercent,
    type AvailableUpdate,
    type DownloadState,
  } from '$lib/update/notice';

  interface Props {
    update: AvailableUpdate;
    onDismiss: () => void;
  }
  let { update, onDismiss }: Props = $props();

  type Phase = 'available' | 'downloading' | 'installing' | 'restarting' | 'failed';
  let phase = $state<Phase>('available');
  let download = $state<DownloadState>(emptyDownload());

  const percent = $derived(downloadPercent(download));

  async function install() {
    phase = 'downloading';
    download = emptyDownload();
    try {
      await installUpdate((event) => {
        download = foldDownload(download, event);
        if (event.event === 'Finished') phase = 'installing';
      });
      // The bundle is installed; hand over to the fresh copy.
      phase = 'restarting';
      await relaunchApp();
    } catch {
      // Any transport/verify/install failure is non-fatal: the user stays on
      // the running version and can retry later. The reason is deliberately not
      // surfaced (nothing actionable), only that it didn't complete.
      phase = 'failed';
    }
  }
</script>

<div class="notice" role="dialog" aria-label={i18n.t('update-available-title')}>
  <header class="head">
    <span class="dot" aria-hidden="true"></span>
    <h2 class="title">{i18n.t('update-available-title')}</h2>
  </header>

  <p class="body">
    {i18n.t('update-available-body', {
      version: update.version,
      current: update.currentVersion,
    })}
  </p>

  {#if update.notes.trim().length > 0}
    <section class="notes">
      <h3 class="notes-heading">{i18n.t('update-notes-heading')}</h3>
      <div class="notes-body">{update.notes}</div>
    </section>
  {/if}

  {#if phase === 'downloading'}
    {#if percent === null}
      <div class="progress indeterminate" aria-label={i18n.t('update-downloading-wait')}>
        <span class="bar"></span>
      </div>
      <p class="status">{i18n.t('update-downloading-wait')}</p>
    {:else}
      <div
        class="progress"
        role="progressbar"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={percent}
      >
        <span class="bar" style="width: {percent}%"></span>
      </div>
      <p class="status">{i18n.t('update-downloading', { percent })}</p>
    {/if}
  {:else if phase === 'installing'}
    <p class="status">{i18n.t('update-installing')}</p>
  {:else if phase === 'restarting'}
    <p class="status">{i18n.t('update-restarting')}</p>
  {:else if phase === 'failed'}
    <p class="status failed">{i18n.t('update-failed')}</p>
  {/if}

  {#if phase === 'available' || phase === 'failed'}
    <div class="actions">
      <button type="button" class="ghost" onclick={onDismiss}>
        {i18n.t('update-later')}
      </button>
      <button type="button" class="primary" onclick={install}>
        {i18n.t('update-install')}
      </button>
    </div>
  {/if}
</div>

<style>
  .notice {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    width: min(360px, calc(100vw - 2 * var(--space-4)));
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    z-index: 40;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--accent);
    flex: none;
  }
  .title {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
    color: var(--text);
  }
  .body {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  .notes {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--space-3);
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .notes-heading {
    margin: 0;
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .notes-body {
    margin: 0;
    max-height: 160px;
    overflow-y: auto;
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .progress {
    height: 6px;
    border-radius: 999px;
    background: var(--bg-surface-alt);
    overflow: hidden;
  }
  .progress .bar {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
    transition: width 120ms ease-out;
  }
  .progress.indeterminate .bar {
    width: 40%;
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    0% {
      margin-left: -40%;
    }
    100% {
      margin-left: 100%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .progress.indeterminate .bar {
      animation: none;
      width: 100%;
      opacity: 0.6;
    }
    .progress .bar {
      transition: none;
    }
  }

  .status {
    margin: 0;
    font-size: var(--text-hint);
    color: var(--text-muted);
  }
  .status.failed {
    color: var(--danger, #dc2626);
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
    padding: 7px 16px;
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
</style>
