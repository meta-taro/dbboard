<script lang="ts">
  import { onMount } from 'svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import {
    listAiProviders,
    addAiProvider,
    updateAiProvider,
    deleteAiProvider,
    setActiveAiProvider,
  } from '$lib/api';
  import {
    emptyProviderForm,
    providerFormForEdit,
    validateProvider,
    buildAddKindInput,
    normalizeModel,
    PROVIDER_KINDS,
    type AiProviderView,
    type ProviderForm,
    type ProviderKind,
    type ProviderField,
    type ProviderMode,
  } from '$lib/ai/panel';

  interface Props {
    onClose: () => void;
    // Called after any change that may flip the active provider, so the panel
    // can re-fetch `ai_status` (add/edit/delete/use).
    onChanged?: () => void;
  }
  let { onClose, onChanged }: Props = $props();

  type Mode = 'list' | 'form';
  let mode = $state<Mode>('list');
  let editorMode = $state<ProviderMode>('add');
  let providers = $state<AiProviderView[]>([]);
  let form = $state<ProviderForm>(emptyProviderForm());
  let invalid = $state<ProviderField[]>([]);
  let busy = $state(false);
  let error = $state('');

  const KIND_LABEL: Record<ProviderKind, MessageKey> = {
    anthropic: 'ai-kind-anthropic',
    openai: 'ai-kind-openai',
  };

  onMount(load);

  async function load(): Promise<void> {
    error = '';
    busy = true;
    try {
      providers = await listAiProviders();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function goList(): void {
    error = '';
    invalid = [];
    mode = 'list';
  }

  function startAdd(): void {
    error = '';
    invalid = [];
    form = emptyProviderForm();
    editorMode = 'add';
    mode = 'form';
  }

  function startEdit(p: AiProviderView): void {
    error = '';
    invalid = [];
    form = providerFormForEdit(p);
    editorMode = 'edit';
    mode = 'form';
  }

  async function saveForm(): Promise<void> {
    invalid = validateProvider(form, editorMode);
    if (invalid.length > 0) return;
    busy = true;
    error = '';
    try {
      if (editorMode === 'add') {
        await addAiProvider(form.id, form.name, buildAddKindInput(form));
      } else {
        // A blank key is sent as undefined so the backend keeps the stored one.
        const key = form.apiKey.trim().length > 0 ? form.apiKey : undefined;
        await updateAiProvider(form.id, form.name, normalizeModel(form.model), key);
      }
      await load();
      onChanged?.();
      goList();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function use(p: AiProviderView): Promise<void> {
    busy = true;
    error = '';
    try {
      await setActiveAiProvider(p.id);
      await load();
      onChanged?.();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function remove(p: AiProviderView): Promise<void> {
    if (!confirm(i18n.t('ai-provider-delete-confirm', { name: p.name }))) return;
    busy = true;
    error = '';
    try {
      await deleteAiProvider(p.id);
      await load();
      onChanged?.();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      if (mode === 'list') onClose();
      else goList();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
  role="presentation"
>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={i18n.t('ai-providers-title')}>
    <header class="head">
      <h2 class="title">{i18n.t('ai-providers-title')}</h2>
      <button type="button" class="icon-btn" onclick={onClose} title={i18n.t('ai-close')}>✕</button>
    </header>

    {#if error}<p class="banner error">{error}</p>{/if}

    {#if mode === 'list'}
      <div class="list">
        {#if providers.length === 0}
          <p class="empty">{i18n.t('ai-providers-empty')}</p>
        {:else}
          {#each providers as p (p.id)}
            <div class="row">
              <div class="row-main">
                <span class="row-name">
                  {p.name}
                  {#if p.active}
                    <span class="marker">({i18n.t('ai-provider-active-marker')})</span>
                  {/if}
                </span>
                <span class="row-meta">
                  {i18n.t(KIND_LABEL[p.kind])}{p.model ? ` · ${p.model}` : ''} · {p.id}
                </span>
              </div>
              <div class="row-actions">
                {#if !p.active}
                  <button type="button" class="ghost" disabled={busy} onclick={() => use(p)}>
                    {i18n.t('ai-provider-use')}
                  </button>
                {/if}
                <button type="button" class="ghost" disabled={busy} onclick={() => startEdit(p)}>
                  {i18n.t('ai-provider-edit')}
                </button>
                <button
                  type="button"
                  class="ghost danger"
                  disabled={busy}
                  onclick={() => remove(p)}
                >
                  {i18n.t('ai-provider-delete')}
                </button>
              </div>
            </div>
          {/each}
        {/if}
      </div>

      <footer class="foot">
        <button type="button" class="primary" disabled={busy} onclick={startAdd}>
          {i18n.t('ai-provider-add')}
        </button>
      </footer>
    {:else}
      <div class="form">
        <h3 class="sub">
          {editorMode === 'add' ? i18n.t('ai-provider-add-title') : i18n.t('ai-provider-edit-title')}
        </h3>

        {#if editorMode === 'add'}
          <label class="field">
            <span class="label">{i18n.t('ai-field-id')}</span>
            <input
              class:bad={invalid.includes('id')}
              value={form.id}
              oninput={(e) => (form.id = e.currentTarget.value)}
              spellcheck="false"
            />
            <span class="hint">{i18n.t('ai-field-id-hint')}</span>
          </label>
        {/if}

        <label class="field">
          <span class="label">{i18n.t('ai-field-name')}</span>
          <input
            class:bad={invalid.includes('name')}
            value={form.name}
            oninput={(e) => (form.name = e.currentTarget.value)}
          />
        </label>

        <label class="field">
          <span class="label">{i18n.t('ai-field-kind')}</span>
          {#if editorMode === 'add'}
            <select
              value={form.kind}
              onchange={(e) => (form.kind = e.currentTarget.value as ProviderKind)}
            >
              {#each PROVIDER_KINDS as k (k)}
                <option value={k}>{i18n.t(KIND_LABEL[k])}</option>
              {/each}
            </select>
          {:else}
            <input class="readonly" value={i18n.t(KIND_LABEL[form.kind])} readonly />
          {/if}
        </label>

        <label class="field">
          <span class="label">{i18n.t('ai-field-model')}</span>
          <input
            value={form.model}
            oninput={(e) => (form.model = e.currentTarget.value)}
            spellcheck="false"
            autocomplete="off"
          />
        </label>

        <label class="field">
          <span class="label">{i18n.t('ai-field-api-key')}</span>
          <input
            class:bad={invalid.includes('apiKey')}
            type="password"
            value={form.apiKey}
            oninput={(e) => (form.apiKey = e.currentTarget.value)}
            spellcheck="false"
            autocomplete="off"
          />
          {#if editorMode === 'edit'}
            <span class="hint">{i18n.t('ai-secret-keep-hint')}</span>
          {/if}
        </label>

        <div class="actions">
          <button type="button" class="ghost" disabled={busy} onclick={goList}>
            {i18n.t('ai-cancel')}
          </button>
          <button type="button" class="primary" disabled={busy} onclick={saveForm}>
            {i18n.t('ai-save')}
          </button>
        </div>
      </div>
    {/if}
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
    z-index: 60;
  }
  .dialog {
    width: min(540px, 94vw);
    max-height: 86vh;
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
    align-items: center;
    justify-content: space-between;
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    color: var(--text);
  }
  .icon-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-body);
    cursor: pointer;
    padding: 4px 8px;
    border-radius: var(--radius-widget);
  }
  .icon-btn:hover {
    background: var(--bg-surface-alt);
    color: var(--text);
  }

  .banner {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-widget);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }
  .banner.error {
    background: var(--danger-weak, rgba(220, 38, 38, 0.12));
    color: var(--danger);
    font-family: var(--font-mono);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow-y: auto;
    min-height: 0;
  }
  .empty {
    margin: 0;
    padding: var(--space-4);
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-widget);
  }
  .row:hover {
    background: var(--bg-surface-alt);
  }
  .row-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .row-name {
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .marker {
    color: var(--text-accent);
    font-weight: 500;
    font-size: var(--text-hint);
  }
  .row-meta {
    font-size: var(--text-hint);
    color: var(--faint);
    font-family: var(--font-mono);
  }
  .row-actions {
    display: flex;
    gap: var(--space-2);
    flex: none;
  }

  .foot {
    display: flex;
    justify-content: flex-end;
    padding-top: var(--space-2);
    border-top: 1px solid var(--border);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    overflow-y: auto;
    min-height: 0;
  }
  .sub {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
    color: var(--text);
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
  .hint {
    font-size: var(--text-hint);
    color: var(--faint);
  }
  .field input,
  .field select {
    width: 100%;
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 7px 10px;
    font-size: var(--text-body);
  }
  .field input:focus-visible,
  .field select:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  .field input.bad {
    border-color: var(--danger);
  }
  .field input.readonly {
    color: var(--text-muted);
    background: var(--bg-surface);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-1);
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
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 6px 14px;
    font-size: var(--text-small);
    font-weight: 600;
    cursor: pointer;
  }
  .ghost:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ghost.danger {
    color: var(--danger);
  }
  .ghost.danger:hover:not(:disabled) {
    border-color: var(--danger);
  }
</style>
