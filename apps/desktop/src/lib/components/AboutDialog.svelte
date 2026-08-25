<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { bundledReleases } from '$lib/about/bundled';
  import { findRelease, releaseHistory } from '$lib/about/changelog';

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  const REPO_URL = 'https://github.com/meta-taro/dbboard';

  let version = $state('—');

  // Shipped versions only: [Unreleased] describes a build nobody is running.
  const releases = releaseHistory(bundledReleases());
  let shownVersion = $state('');
  const shown = $derived(releases.find((r) => r.version === shownVersion) ?? null);

  onMount(async () => {
    try {
      version = await getVersion();
      // A build the changelog has never heard of leaves the picker empty
      // rather than showing someone else's release as if it were theirs.
      shownVersion = findRelease(releases, version)?.version ?? '';
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

    <section class="changes">
      <div class="changes-head">
        <h3 class="changes-title">{i18n.t('about-changes-title')}</h3>
        <select
          class="changes-pick"
          bind:value={shownVersion}
          aria-label={i18n.t('about-changes-release')}
        >
          {#each releases as release (release.version)}
            <option value={release.version}>
              {release.version}{release.date ? ` — ${release.date}` : ''}
            </option>
          {/each}
        </select>
      </div>

      {#if shown}
        {#if shown.headline}<p class="changes-headline">{shown.headline}</p>{/if}
        {#if shown.lead}<p class="changes-lead">{shown.lead}</p>{/if}
        <div class="changes-body">
          {#each shown.groups as group, gi (gi)}
            <h4 class="changes-group">{group.heading}</h4>
            <ul class="changes-list">
              {#each group.changes as change, ci (ci)}
                <li class:nested={change.depth > 0}>
                  {#if change.title}<b>{change.title}</b>{#if change.body}&nbsp;—&nbsp;{/if}{/if}{change.body}
                </li>
              {/each}
            </ul>
          {/each}
        </div>
      {:else}
        <p class="changes-lead">{i18n.t('about-changes-none')}</p>
      {/if}

      <!-- CHANGELOG.md is written in English, and translating it would double
           the work of every release. Say so rather than let a Japanese reader
           wonder whether the dialog failed to localise. -->
      {#if i18n.locale !== 'en'}
        <p class="changes-note">{i18n.t('about-changes-english')}</p>
      {/if}
    </section>

    <p class="docs">{i18n.t('help-docs-hint')}</p>

    <p class="repo">
      <span class="repo-label">{i18n.t('help-repo-link')}</span>
      <code class="repo-url">{REPO_URL}</code>
    </p>

    <!-- The AI-assistant safeguard note, at egui parity (ADR-0066): it never
         runs SQL, never writes, never sends rows; the key lives in the OS
         keyring. Shown here so the promise is visible from the About dialog. -->
    <section class="ai-about">
      <h3 class="ai-about-title">{i18n.t('help-ai-about-title')}</h3>
      <p class="ai-about-body">{i18n.t('help-ai-about-body')}</p>
    </section>

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
    width: min(560px, 92vw);
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

  .ai-about {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--space-3);
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .ai-about-title {
    margin: 0;
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .ai-about-body {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.5;
  }

  .changes {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .changes-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .changes-title {
    margin: 0;
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .changes-pick {
    font-family: var(--font-mono);
    font-size: var(--text-small);
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 3px 6px;
  }
  .changes-headline {
    margin: 0;
    color: var(--text);
    font-size: var(--text-small);
    font-weight: 600;
  }
  .changes-lead {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  /* Capped, not collapsed: a long release stays scrollable inside the dialog
     instead of pushing the close button off the bottom of the screen. */
  .changes-body {
    max-height: 46vh;
    overflow-y: auto;
    padding: var(--space-3);
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .changes-group {
    margin: var(--space-3) 0 4px;
    font-size: var(--text-hint);
    font-weight: 700;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .changes-group:first-child {
    margin-top: 0;
  }
  .changes-list {
    margin: 0;
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .changes-list li {
    color: var(--text-muted);
    font-size: var(--text-small);
    line-height: 1.5;
  }
  .changes-list li.nested {
    margin-left: var(--space-3);
  }
  .changes-list b {
    color: var(--text);
  }
  .changes-note {
    margin: 0;
    color: var(--faint);
    font-size: var(--text-hint);
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
