<script lang="ts">
  /**
   * The full cell editor (ADR-0082): the same staging as the inline box, on a
   * surface big enough for a `varchar(500)`.
   *
   * The draft is the caller's, bound rather than copied, because the inline
   * editor hands its half-typed text over when the operator runs out of room
   * and the grid is the one that stages the result.
   */
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { charCount } from '$lib/grid/edit';
  import { enumOptions } from '$lib/grid/enum';

  interface Props {
    /** Column name, shown as the dialog's title. */
    col: string;
    draft: string;
    /** Declared ENUM members, or null when the column is free text. */
    variants: string[] | null;
    /** Stage the draft as the cell's new value. */
    onCommit: () => void;
    /** Stage SQL NULL, whatever the draft says. */
    onNull: () => void;
    onCancel: () => void;
  }
  let { col, draft = $bindable(), variants, onCommit, onNull, onCancel }: Props = $props();

  // Enter inserts a newline in the textarea, so committing needs a modifier.
  // Escape is handled by the grid's window listener, like the value popup.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      onCommit();
    }
  }
</script>

<div
  class="backdrop result-grid"
  onclick={(e) => {
    // Clicking away cancels rather than commits: a dialog opened by accident
    // must not be able to stage an edit on its way out.
    if (e.target === e.currentTarget) onCancel();
  }}
  role="presentation"
>
  <div class="popup editor" role="dialog" aria-modal="true" aria-label={i18n.t('edit-cell-dialog')}>
    <div class="popup-head">
      <span class="popup-col">{col}</span>
      {#if !variants}
        <span class="popup-len">
          {i18n.t('edit-cell-chars', { count: charCount(draft) })}
        </span>
      {/if}
    </div>
    {#if variants}
      <!-- Reachable when a declared member is long enough to route past the
           inline box; it must still be a choice, not a text field. -->
      <!-- svelte-ignore a11y_autofocus -->
      <select class="popup-select" bind:value={draft} autofocus>
        {#each enumOptions(variants, draft) as v (v)}
          <option value={v}>{v === '' ? i18n.t('edit-enum-blank') : v}</option>
        {/each}
      </select>
    {:else}
      <!-- svelte-ignore a11y_autofocus -->
      <textarea
        class="popup-edit"
        bind:value={draft}
        autofocus
        spellcheck="false"
        onkeydown={onKeydown}
      ></textarea>
    {/if}
    <div class="popup-foot">
      <button type="button" class="ghost" onclick={onNull} title={i18n.t('edit-null-title')}>
        ∅ NULL
      </button>
      {#if !variants}
        <span class="popup-hint">{i18n.t('edit-cell-dialog-hint')}</span>
      {:else}
        <span class="popup-hint"></span>
      {/if}
      <button type="button" onclick={onCancel}>{i18n.t('edit-cell-cancel')}</button>
      <button type="button" class="primary" onclick={onCommit}>
        {i18n.t('edit-cell-apply')}
      </button>
    </div>
  </div>
</div>
