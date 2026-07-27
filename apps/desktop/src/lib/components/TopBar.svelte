<script lang="ts">
  import { onMount } from 'svelte';
  import { titlebarController } from '$lib/window/titlebar.svelte';

  // Frameless (decorations:false): this bar IS the OS title bar. The header
  // ground is the drag region (data-tauri-drag-region); .lead is pointer-events
  // transparent so you can grab the bar anywhere, while the window controls on
  // the right stay clickable.
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
  /* Dark-only, matching the spike palette in +page.svelte (a full theme system
     is out of scope for the spike). The point of this bar is that the whole
     window frame is now dbboard-dark, not the OS light chrome. */
  .topbar {
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: #0c0e14;
    border-bottom: 1px solid #1e2130;
    user-select: none;
  }

  /* Transparent to pointer events so the whole lead area drags the window. */
  .lead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 14px;
    min-width: 0;
    pointer-events: none;
  }

  .brand-dot {
    width: 10px;
    height: 10px;
    border-radius: 999px;
    background: linear-gradient(135deg, #818cf8, #6366f1);
    flex: none;
  }

  .brand {
    font-size: 13px;
    font-weight: 600;
    color: #e6e8f0;
    letter-spacing: -0.01em;
  }

  .tag {
    font-size: 11px;
    font-weight: 600;
    color: #a5b4fc;
    border: 1px solid #282c39;
    border-radius: 6px;
    padding: 1px 8px;
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
    color: #8b90a3;
    font-size: 13px;
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
    background: #171922;
    color: #e6e8f0;
  }

  .wc.close:hover {
    background: #e5484d;
    color: #ffffff;
  }

  .wc:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px #6366f1;
    color: #e6e8f0;
  }
</style>
