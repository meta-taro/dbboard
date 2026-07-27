<script lang="ts">
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { LOCALES } from '$lib/i18n/locales';

  let open = $state(false);

  const current = $derived(
    LOCALES.find((l) => l.code === i18n.locale) ?? LOCALES[0],
  );

  function choose(code: string) {
    i18n.setLocale(code);
    open = false;
  }
</script>

<div class="langmenu">
  <button
    type="button"
    class="trigger"
    onclick={() => (open = !open)}
    aria-haspopup="menu"
    aria-expanded={open}
    title={i18n.t('language-menu')}
  >
    🌐 <span class="native">{current.native}</span>
  </button>

  {#if open}
    <div class="pop" role="menu">
      {#each LOCALES as loc (loc.code)}
        <button
          type="button"
          class="item"
          class:active={loc.code === i18n.locale}
          role="menuitemradio"
          aria-checked={loc.code === i18n.locale}
          onclick={() => choose(loc.code)}
        >
          <span class="native">{loc.native}</span>
          <span class="code">{loc.code}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<svelte:window
  onclick={(e) => {
    if (open && !(e.target as HTMLElement).closest('.langmenu')) open = false;
  }}
/>

<style>
  .langmenu {
    position: relative;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--bg-surface-alt);
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 4px 10px;
    font-size: var(--text-hint);
    font-weight: 600;
    cursor: pointer;
  }
  .trigger:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .trigger .native {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pop {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    width: 200px;
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    z-index: 30;
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: var(--text-small);
    padding: 6px 10px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .item:hover {
    background: var(--bg-surface-alt);
  }
  .item.active {
    background: var(--accent-weak);
    color: var(--text-accent);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .item .code {
    font-family: var(--font-mono);
    font-size: var(--text-hint);
    color: var(--faint);
    text-transform: uppercase;
  }
</style>
