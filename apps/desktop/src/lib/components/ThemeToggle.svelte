<script lang="ts">
  import { theme, type ThemeMode } from '$lib/theme/theme.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';

  // Segmented Auto | Light | Dark control (DESIGN.md "Segmented theme toggle").
  // The selected segment reads active; the whole group is one rounded field.
  const modes: { value: ThemeMode; labelKey: MessageKey }[] = [
    { value: 'auto', labelKey: 'theme-auto' },
    { value: 'light', labelKey: 'theme-light' },
    { value: 'dark', labelKey: 'theme-dark' },
  ];
</script>

<div class="segmented" role="group" aria-label={i18n.t('theme-menu')}>
  {#each modes as m (m.value)}
    <button
      type="button"
      class="seg"
      class:active={theme.mode === m.value}
      aria-pressed={theme.mode === m.value}
      onclick={() => theme.set(m.value)}
    >
      {i18n.t(m.labelKey)}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    background: var(--bg-surface-alt);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }

  .seg {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-hint);
    font-weight: 600;
    padding: 3px 10px;
    border-radius: calc(var(--radius-widget) - 2px);
    cursor: pointer;
    transition:
      background 0.12s ease,
      color 0.12s ease;
  }

  .seg:hover {
    color: var(--text);
  }

  .seg.active {
    background: var(--bg-surface);
    color: var(--text-accent);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
  }
</style>
