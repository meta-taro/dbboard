<script lang="ts">
  // Lightweight right-click menu, positioned at the cursor and clamped to the
  // viewport. Reusable: the caller supplies items and an onClose. Closes on any
  // outside interaction, Escape, scroll, resize, or window blur.
  export interface MenuItem {
    label: string;
    onSelect: () => void;
    danger?: boolean;
    separatorBefore?: boolean;
  }

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onClose: () => void;
  }
  let { x, y, items, onClose }: Props = $props();

  let el: HTMLDivElement | undefined = $state();
  // null until measured: the menu opens at the raw cursor point, then snaps
  // inside the viewport once it has a size.
  let clamped = $state<{ left: number; top: number } | null>(null);

  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    const pad = 8;
    clamped = {
      left: Math.max(pad, Math.min(x, window.innerWidth - r.width - pad)),
      top: Math.max(pad, Math.min(y, window.innerHeight - r.height - pad)),
    };
  });

  function choose(item: MenuItem) {
    item.onSelect();
    onClose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') onClose();
  }
</script>

<svelte:window
  onkeydown={onKey}
  onresize={onClose}
  onblur={onClose}
  onscrollcapture={onClose}
/>

<!-- Catches any outside click (or a second right-click) to dismiss the menu. -->
<div
  class="cm-backdrop"
  role="presentation"
  onpointerdown={onClose}
  oncontextmenu={(e) => {
    e.preventDefault();
    onClose();
  }}
></div>

<div
  class="cm-menu"
  bind:this={el}
  style="left:{clamped?.left ?? x}px; top:{clamped?.top ?? y}px"
  role="menu"
  tabindex="-1"
>
  {#each items as item, i (i)}
    {#if item.separatorBefore}<div class="cm-sep" role="separator"></div>{/if}
    <button
      type="button"
      class="cm-item"
      class:danger={item.danger}
      role="menuitem"
      onclick={() => choose(item)}
    >
      {item.label}
    </button>
  {/each}
</div>

<style>
  .cm-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }
  .cm-menu {
    position: fixed;
    z-index: 41;
    min-width: 176px;
    padding: var(--space-1);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    box-shadow: var(--shadow-popover);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .cm-item {
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--text-body);
    padding: 6px 10px;
    border-radius: 4px;
    cursor: pointer;
  }
  .cm-item:hover {
    background: var(--accent-weak);
    color: var(--text-accent);
  }
  .cm-item.danger:hover {
    background: var(--danger-weak);
    color: var(--danger);
  }
  .cm-sep {
    height: 1px;
    margin: var(--space-1) 0;
    background: var(--border);
  }
</style>
