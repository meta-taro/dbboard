<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { aiStatus, aiExplain, aiSuggest, cancelAi, onAiChunk } from '$lib/api';
  import {
    canSend,
    showIncludeDetails,
    emptyStream,
    accumulate,
    hasTokens,
    type AiMode,
    type AiStatus,
    type AiOutcome,
    type StreamState,
  } from '$lib/ai/panel';
  import AiProvidersDialog from './AiProvidersDialog.svelte';

  interface Props {
    // Null when no connection is selected — Explain still works; Suggest is
    // gated (its schema comes from the connection).
    connectionId: string | null;
    onClose: () => void;
  }
  let { connectionId, onClose }: Props = $props();

  type Phase = 'idle' | 'running' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let status = $state<AiStatus | null>(null);
  let mode = $state<AiMode>('explain');
  let input = $state('');
  let includeDetails = $state(false);
  let stream = $state<StreamState>(emptyStream());
  let outcome = $state<AiOutcome | null>(null);
  let errorMsg = $state('');
  let cancelling = $state(false);
  let copied = $state(false);
  let providersOpen = $state(false);

  let unlisten: UnlistenFn | null = null;

  const hasConnection = $derived(connectionId !== null);
  const sendable = $derived(
    !!status?.active && phase !== 'running' && canSend(mode, input, hasConnection),
  );
  // What to render in the body: the live buffer while streaming, the final
  // answer once done (they converge; kept separate so a cancel keeps its text).
  const body = $derived(phase === 'running' ? stream.text : (outcome?.text ?? ''));
  const meterIn = $derived(phase === 'running' ? stream.tokensIn : (outcome?.tokens_in ?? 0));
  const meterOut = $derived(phase === 'running' ? stream.tokensOut : (outcome?.tokens_out ?? 0));
  const showMeter = $derived(hasTokens(meterIn, meterOut));

  onMount(refreshStatus);

  onDestroy(() => {
    unlisten?.();
  });

  async function refreshStatus(): Promise<void> {
    try {
      status = await aiStatus();
    } catch (e) {
      errorMsg = String(e);
    }
  }

  function setMode(m: AiMode): void {
    mode = m;
  }

  async function send(): Promise<void> {
    if (!sendable) return;
    phase = 'running';
    cancelling = false;
    copied = false;
    stream = emptyStream();
    outcome = null;
    errorMsg = '';

    unlisten = await onAiChunk((c) => {
      stream = accumulate(stream, c);
    });

    try {
      outcome =
        mode === 'explain'
          ? await aiExplain(input, connectionId)
          : await aiSuggest(connectionId as string, input, includeDetails);
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
      await cancelAi();
    } catch {
      // A failed cancel just means the run already finished; the outcome
      // resolves normally either way.
    }
  }

  async function copyBody(): Promise<void> {
    try {
      await navigator.clipboard.writeText(body);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // Clipboard can be denied in a locked-down webview; silently ignore.
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && phase !== 'running' && !providersOpen) onClose();
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
  <div class="dialog" role="dialog" aria-modal="true" aria-label={i18n.t('ai-panel-title')}>
    <header class="head">
      <div class="head-text">
        <h2 class="title">{i18n.t('ai-panel-title')}</h2>
        <p class="scope">{i18n.t('ai-scope-hint')}</p>
      </div>
      <button type="button" class="icon-btn" onclick={onClose} title={i18n.t('ai-close')}>✕</button>
    </header>

    {#if status && !status.active}
      <!-- Empty state: no provider bound. Manage-providers is offered only
           when provider storage is available on this host. -->
      <div class="no-provider">
        <p class="np-title">{i18n.t('ai-no-provider-title')}</p>
        <p class="np-body">{i18n.t('ai-no-provider-body')}</p>
        {#if status.can_manage}
          <button type="button" class="primary" onclick={() => (providersOpen = true)}>
            {i18n.t('ai-manage-providers')}
          </button>
        {/if}
      </div>
    {:else if status}
      <div class="subbar">
        {#if status.provider_label}
          <span class="active">{i18n.t('ai-active', { name: status.provider_label })}</span>
        {/if}
        {#if status.can_manage}
          <button type="button" class="link" onclick={() => (providersOpen = true)}>
            {i18n.t('ai-manage-providers')}
          </button>
        {/if}
      </div>

      <div class="modes" role="tablist" aria-label={i18n.t('ai-panel-title')}>
        <button
          type="button"
          class="mode"
          class:active={mode === 'explain'}
          role="tab"
          aria-selected={mode === 'explain'}
          onclick={() => setMode('explain')}
        >
          {i18n.t('ai-mode-explain')}
        </button>
        <button
          type="button"
          class="mode"
          class:active={mode === 'suggest'}
          role="tab"
          aria-selected={mode === 'suggest'}
          onclick={() => setMode('suggest')}
        >
          {i18n.t('ai-mode-suggest')}
        </button>
      </div>

      <label class="field">
        <span class="label">
          {i18n.t(mode === 'explain' ? 'ai-input-explain' : 'ai-input-suggest')}
        </span>
        <textarea
          class="input"
          rows="3"
          bind:value={input}
          spellcheck="false"
          disabled={phase === 'running'}
        ></textarea>
      </label>

      {#if showIncludeDetails(mode)}
        <label class="details">
          <input
            type="checkbox"
            bind:checked={includeDetails}
            disabled={!hasConnection || phase === 'running'}
          />
          {i18n.t('ai-include-details')}
        </label>
      {/if}

      {#if mode === 'suggest' && !hasConnection}
        <p class="hint-line">{i18n.t('ai-suggest-needs-connection')}</p>
      {/if}

      {#if body}
        <div class="response">
          <pre class="body">{body}</pre>
        </div>
      {:else if phase === 'running'}
        <p class="status">{i18n.t('ai-busy')}</p>
      {:else if phase !== 'error'}
        <p class="status empty-body">{i18n.t('ai-empty')}</p>
      {/if}

      {#if outcome?.cancelled}
        <p class="status" role="status">{i18n.t('ai-cancelled')}</p>
      {/if}
      {#if outcome && outcome.prefetch_warnings > 0}
        <p class="warn" role="alert">
          {i18n.t('ai-prefetch-warning', { count: outcome.prefetch_warnings })}
        </p>
      {/if}
      {#if phase === 'error'}
        <p class="error" role="alert">{i18n.t('ai-error')}: {errorMsg}</p>
      {/if}

      <div class="footer">
        <span class="meter">
          {#if showMeter}
            {i18n.t('ai-tokens', { tin: meterIn, tout: meterOut })}
          {/if}
        </span>
        <div class="actions">
          {#if body && phase !== 'running'}
            <button type="button" class="ghost" onclick={copyBody}>
              {copied ? i18n.t('ai-copied') : i18n.t('ai-copy')}
            </button>
          {/if}
          {#if phase === 'running'}
            <button type="button" class="ghost" onclick={requestCancel} disabled={cancelling}>
              {i18n.t('ai-cancel')}
            </button>
          {:else}
            <button type="button" class="primary" onclick={send} disabled={!sendable}>
              {i18n.t('ai-send')}
            </button>
          {/if}
        </div>
      </div>
    {:else if errorMsg}
      <p class="error" role="alert">{i18n.t('ai-error')}: {errorMsg}</p>
    {/if}
  </div>
</div>

{#if providersOpen}
  <AiProvidersDialog
    onClose={() => (providersOpen = false)}
    onChanged={refreshStatus}
  />
{/if}

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
    width: min(560px, 94vw);
    max-height: 88vh;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .head-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    color: var(--text);
  }
  .scope {
    margin: 0;
    font-size: var(--text-hint);
    color: var(--text-muted);
    line-height: 1.5;
  }
  .icon-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-body);
    cursor: pointer;
    padding: 4px 8px;
    border-radius: var(--radius-widget);
    flex: none;
  }
  .icon-btn:hover {
    background: var(--bg-surface-alt);
    color: var(--text);
  }

  .subbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .active {
    font-size: var(--text-hint);
    color: var(--text-accent);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-hint);
    cursor: pointer;
    padding: 2px 4px;
    flex: none;
  }
  .link:hover {
    color: var(--text-accent);
    text-decoration: underline;
  }

  .no-provider {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-2);
  }
  .np-title {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
    color: var(--text);
  }
  .np-body {
    margin: 0;
    font-size: var(--text-small);
    color: var(--text-muted);
    line-height: 1.5;
  }

  .modes {
    display: flex;
    gap: var(--space-2);
  }
  .mode {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-small);
    font-weight: 600;
    padding: 5px var(--space-3);
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .mode:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .mode.active {
    color: var(--text-accent);
    background: var(--accent-weak);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .label {
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-muted);
  }
  .input {
    width: 100%;
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 8px 10px;
    font-family: var(--font-mono);
    font-size: var(--text-small);
    resize: vertical;
  }
  .input:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  .input:disabled {
    opacity: 0.6;
  }

  .details {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-small);
    color: var(--text);
    cursor: pointer;
  }
  .hint-line {
    margin: 0;
    font-size: var(--text-hint);
    color: var(--text-muted);
  }

  .response {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: var(--space-3);
  }
  .body {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-small);
    color: var(--text);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.55;
  }
  .status {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .empty-body {
    padding: var(--space-4) 0;
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

  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .meter {
    font-size: var(--text-hint);
    color: var(--faint);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    display: flex;
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
  .ghost:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .ghost:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
