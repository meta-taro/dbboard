<script lang="ts">
  import { onMount } from 'svelte';
  import { titlebarController } from '$lib/window/titlebar.svelte';
  import ThemeToggle from './ThemeToggle.svelte';

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
    <span class="tag">Tauri + SvelteKit spike</span>
  </div>

  <div class="actions">
    <ThemeToggle />
  </div>

  <div class="window-controls">
    <button
      class="wc"
      type="button"
      onclick={() => titlebarController.minimize()}
      title="Minimize"
      aria-label="Minimize"
    >
      ─
    </button>
    <button
      class="wc"
      type="button"
      onclick={() => titlebarController.toggleMaximize()}
      title={titlebarController.isMaximized ? 'Restore' : 'Maximize'}
      aria-label={titlebarController.isMaximized ? 'Restore' : 'Maximize'}
    >
      {titlebarController.maxGlyph}
    </button>
    <button
      class="wc close"
      type="button"
      onclick={() => titlebarController.close()}
      title="Close"
      aria-label="Close"
    >
      ✕
    </button>
  </div>
</header>

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

  .tag {
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-accent);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 1px 8px;
  }

  /* Pushed to the right, just left of the window controls. Interactive, so it
     opts back into pointer events that .lead gave up. */
  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    padding-right: var(--space-2);
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
