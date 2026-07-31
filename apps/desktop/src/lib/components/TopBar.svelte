<script lang="ts">
  import { onMount } from 'svelte';
  import { titlebarController } from '$lib/window/titlebar.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import ThemeToggle from './ThemeToggle.svelte';
  import LanguageMenu from './LanguageMenu.svelte';
  import AboutDialog from './AboutDialog.svelte';

  let aboutOpen = $state(false);

  // Frameless (decorations:false): this bar IS the OS title bar. The bar itself
  // is the drag region (data-tauri-drag-region); .lead is pointer-events
  // transparent so you can grab the bar there, while the theme toggle and the
  // window controls stay interactive.
  onMount(() => {
    titlebarController.init();
  });
</script>

<header class="topbar" data-tauri-drag-region>
  <div class="lead">
    <span class="brand-dot" aria-hidden="true"></span>
    <span class="brand">dbboard</span>
  </div>

  <div class="actions">
    <button
      type="button"
      class="help"
      onclick={() => (aboutOpen = true)}
      title={i18n.t('help-menu')}
      aria-label={i18n.t('about-title')}
    >
      ?
    </button>
    <LanguageMenu />
    <ThemeToggle />
  </div>

  <div class="window-controls">
    <button
      class="wc"
      type="button"
      onclick={() => titlebarController.minimize()}
      title={i18n.t('win-minimize')}
      aria-label={i18n.t('win-minimize')}
    >
      ─
    </button>
    <button
      class="wc"
      type="button"
      onclick={() => titlebarController.toggleMaximize()}
      title={titlebarController.isMaximized ? i18n.t('win-restore') : i18n.t('win-maximize')}
      aria-label={titlebarController.isMaximized ? i18n.t('win-restore') : i18n.t('win-maximize')}
    >
      {titlebarController.maxGlyph}
    </button>
    <button
      class="wc close"
      type="button"
      onclick={() => titlebarController.close()}
      title={i18n.t('win-close')}
      aria-label={i18n.t('win-close')}
    >
      ✕
    </button>
  </div>
</header>

{#if aboutOpen}
  <AboutDialog onClose={() => (aboutOpen = false)} />
{/if}

<style>
  .topbar {
    height: 40px;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--bg-canvas);
    border-bottom: 1px solid var(--border);
    user-select: none;
  }

  /* Transparent to pointer events so the whole lead area drags the window. */
  .lead {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-left: 14px;
    min-width: 0;
    pointer-events: none;
  }

  /* Rounded-square gradient logo mark, matching the design mock. */
  .brand-dot {
    width: 18px;
    height: 18px;
    border-radius: 5px;
    background: linear-gradient(150deg, #6366f1, #4f46e5);
    box-shadow: 0 1px 3px rgba(79, 70, 229, 0.4);
    flex: none;
  }

  .brand {
    font-size: var(--text-body);
    font-weight: 600;
    color: var(--text);
    letter-spacing: -0.01em;
  }

  /* Pushed to the right, just left of the window controls. Interactive, so it
     opts back into pointer events that .lead gave up. */
  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-right: var(--space-2);
  }

  /* Circular ghost help button — quiet until hovered. */
  .help {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--bg-surface-alt);
    color: var(--text-muted);
    font-size: var(--text-hint);
    font-weight: 700;
    line-height: 1;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .help:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }

  /* Windows convention: controls flush to the top-right corner, full height. */
  .window-controls {
    display: flex;
    align-items: stretch;
    align-self: stretch;
  }

  .wc {
    width: 46px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-body);
    line-height: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition:
      background 0.12s ease,
      color 0.12s ease;
  }

  .wc:hover {
    background: var(--bg-surface-alt);
    color: var(--text);
  }

  .wc.close:hover {
    background: #e5484d;
    color: #ffffff;
  }

  .wc:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--accent);
    color: var(--text);
  }
</style>
