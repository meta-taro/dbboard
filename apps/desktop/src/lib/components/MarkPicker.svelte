<script lang="ts">
  import { untrack } from 'svelte';
  // Set a connection's identity mark from where it is displayed, rather than
  // from inside the edit form (ADR-0130).
  //
  // A swatch grid rather than the form's `<select>` of colour names: this
  // opens on the row it acts on, and the point is that one click paints that
  // row. A dropdown would make it three (open, read eight words, choose), and
  // reading "teal" is a slower way to pick a colour than seeing it.
  //
  // Positioning, dismissal and the backdrop are deliberately the same shape as
  // ContextMenu — this opens the same way, on the same gesture, in the same
  // list.
  import {
    CONNECTION_COLORS,
    CONNECTION_TAG_MAX_CHARS,
    colorVar,
    isConnectionTag,
    type ConnectionColor,
  } from '$lib/connections/marks';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';

  // The same eight labels the edit form offers, so a colour is called the
  // same thing in both places.
  const COLOR_LABEL: Record<ConnectionColor, MessageKey> = {
    red: 'conn-color-red',
    orange: 'conn-color-orange',
    yellow: 'conn-color-yellow',
    green: 'conn-color-green',
    teal: 'conn-color-teal',
    blue: 'conn-color-blue',
    purple: 'conn-color-purple',
    pink: 'conn-color-pink',
  };

  interface Props {
    x: number;
    y: number;
    /** The connection being marked, for the heading. */
    name: string;
    color: ConnectionColor | null;
    tag: string;
    /** Blank `color` means no colour, blank `tag` means no tag. */
    onApply: (color: string, tag: string) => void;
    onClose: () => void;
  }
  let { x, y, name, color, tag, onApply, onClose }: Props = $props();

  let el: HTMLDivElement | undefined = $state();
  let clamped = $state<{ left: number; top: number } | null>(null);
  // Seeded once, deliberately: Sidebar mounts the picker fresh every time it
  // opens, so there is no later `tag` to follow — and following one would
  // overwrite what is being typed the moment a save lands.
  let draftTag = $state(untrack(() => tag));

  // Same measure-then-snap as ContextMenu: opens at the cursor, then moves
  // inside the viewport once it has a size.
  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    const pad = 8;
    clamped = {
      left: Math.max(pad, Math.min(x, window.innerWidth - r.width - pad)),
      top: Math.max(pad, Math.min(y, window.innerHeight - r.height - pad)),
    };
  });

  // A swatch is the whole gesture: apply and close. Nothing to confirm — the
  // mark is reversible in one more click, and a picker that needs an OK is
  // the edit form again.
  function pick(next: string) {
    onApply(next, draftTag.trim());
    onClose();
  }

  // The tag needs a keystroke to end on, so it commits on Enter or on close
  // rather than per character: every character would be a file write.
  function commitTag() {
    const next = draftTag.trim();
    if (next === tag.trim()) return;
    if (!isConnectionTag(next)) return;
    onApply(color ?? '', next);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }

  function dismiss() {
    commitTag();
    onClose();
  }
</script>

<svelte:window onkeydown={onKey} onresize={dismiss} onblur={dismiss} />

<div
  class="mp-backdrop"
  role="presentation"
  onpointerdown={dismiss}
  oncontextmenu={(e) => {
    e.preventDefault();
    dismiss();
  }}
></div>

<div
  class="mp"
  bind:this={el}
  style="left:{clamped?.left ?? x}px; top:{clamped?.top ?? y}px"
  role="dialog"
  aria-label={i18n.t('mark-picker-title', { name })}
>
  <div class="mp-head">{i18n.t('mark-picker-title', { name })}</div>

  <div class="swatches" role="group" aria-label={i18n.t('conn-mark-color')}>
    {#each CONNECTION_COLORS as c (c)}
      <button
        type="button"
        class="swatch"
        class:chosen={color === c}
        style="--mark: {colorVar(c)}"
        title={i18n.t(COLOR_LABEL[c])}
        aria-label={i18n.t(COLOR_LABEL[c])}
        aria-pressed={color === c}
        onclick={() => pick(c)}
      ></button>
    {/each}
    <!-- Clearing is a swatch too, in the same row: "no colour" is a choice
         among the nine, not a separate act of undoing. -->
    <button
      type="button"
      class="swatch none"
      class:chosen={!color}
      title={i18n.t('conn-mark-color-none')}
      aria-label={i18n.t('conn-mark-color-none')}
      aria-pressed={!color}
      onclick={() => pick('')}
    ></button>
  </div>

  <label class="tag-field">
    <span class="label">{i18n.t('conn-mark-tag')}</span>
    <input
      value={draftTag}
      maxlength={CONNECTION_TAG_MAX_CHARS}
      placeholder={i18n.t('conn-mark-tag-placeholder')}
      oninput={(e) => (draftTag = e.currentTarget.value)}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          commitTag();
          onClose();
        }
      }}
    />
  </label>
</div>

<style>
  .mp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }
  .mp {
    position: fixed;
    z-index: 41;
    width: 208px;
    padding: var(--space-2);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    box-shadow: var(--shadow-popover);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .mp-head {
    font-size: var(--text-hint);
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .swatches {
    display: grid;
    /* Three rows of three keeps every swatch a comfortable target; a single
       row of nine would make each one narrower than a fingertip. */
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }
  .swatch {
    height: 26px;
    border: 1px solid color-mix(in srgb, var(--mark) 55%, transparent);
    border-radius: var(--radius-widget);
    background: var(--mark);
    cursor: pointer;
    padding: 0;
  }
  /* "No colour" reads as an empty slot rather than as a ninth colour. */
  .swatch.none {
    background: transparent;
    border: 1px dashed var(--border);
    position: relative;
  }
  .swatch.none::after {
    content: '';
    position: absolute;
    inset: 0;
    margin: auto;
    width: 60%;
    height: 1px;
    background: var(--border);
    transform: rotate(-45deg);
  }
  .swatch:hover {
    outline: 2px solid var(--accent-weak);
  }
  /* The current colour is stated by an outline rather than a tick: a tick
     drawn on eight different backgrounds needs a contrast rule of its own. */
  .swatch.chosen {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }
  .swatch:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .tag-field {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .label {
    font-size: var(--text-hint);
    color: var(--text-muted);
  }
  .tag-field input {
    width: 100%;
    box-sizing: border-box;
    padding: 4px 6px;
    font-size: var(--text-body);
    color: var(--text);
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
</style>
