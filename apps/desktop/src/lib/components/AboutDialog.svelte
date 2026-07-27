<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { i18n } from '$lib/i18n/i18n.svelte';

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  const REPO_URL = 'https://github.com/meta-taro/dbboard';

  let version = $state('—');

  onMount(async () => {
    try {
      version = await getVersion();
    } catch {
      // Outside a Tauri runtime (e.g. a plain browser preview) the app version
      // isn't available; leave the placeholder rather than surfacing an error.
    }
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose();
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
  <div class="dialog" role="dialog" aria-modal="true" aria-label={i18n.t('about-title')}>
    <header class="head">
      <span class="brand-dot" aria-hidden="true"></span>
      <h2 class="title">dbboard</h2>
    </header>

    <dl class="meta">
      <dt>{i18n.t('about-version')}</dt>
      <dd class="mono">{version}</dd>
    </dl>

    <p class="docs">{i18n.t('help-docs-hint')}</p>

    <p class="repo">
      <span class="repo-label">{i18n.t('help-repo-link')}</span>
      <code class="repo-url">{REPO_URL}</code>
    </p>

    <div class="actions">
      <button type="button" class="primary" onclick={onClose}>
        {i18n.t('about-close')}
      </button>
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
    width: min(440px, 92vw);
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
    align-items: center;
    gap: var(--space-2);
  }
  .brand-dot {
    width: 20px;
    height: 20px;
    border-radius: 6px;
    background: linear-gradient(150deg, #6366f1, #4f46e5);
    box-shadow: 0 1px 3px rgba(79, 70, 229, 0.4);
    flex: none;
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    color: var(--text);
  }

  .meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px var(--space-3);
    margin: 0;
  }
  .meta dt {
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--faint);
    align-self: center;
  }
  .meta dd {
    margin: 0;
    color: var(--text);
  }
  .mono {
    font-family: var(--font-mono);
  }

  .docs {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .repo {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .repo-label {
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-muted);
  }
  .repo-url {
    font-family: var(--font-mono);
    font-size: var(--text-small);
    color: var(--text-accent);
    user-select: all;
    word-break: break-all;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
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
</style>
