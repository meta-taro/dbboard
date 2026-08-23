<script lang="ts">
  /**
   * The read-only value dialog: a cell too wide to have been shown in full, or
   * a document shown as a tree.
   *
   * It owns `treeClosed` rather than taking it from the grid, so that opening
   * a second cell starts expanded again. The grid used to reset that set by
   * hand on the way in; here the dialog is unmounted when it closes and the
   * set goes with it, which is the same reset without anybody remembering it.
   * That holds because the backdrop is `position: fixed; inset: 0` — no cell
   * is reachable while this is up, so there is no way to swap one open value
   * for another without closing first.
   */
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { allContainerPaths, flattenDocument, toggled } from '$lib/grid/tree';

  interface Props {
    /** Column name, shown as the dialog's title. */
    col: string;
    /** The full text — the JSON serialisation when this is a document. */
    value: string;
    /**
     * Set only for a document cell, and wrapped so that a document whose value
     * is literally `null` stays distinguishable from "not a document"
     * (ADR-0100).
     */
    doc: { value: unknown } | null;
    onClose: () => void;
  }
  let { col, value, doc, onClose }: Props = $props();

  // Which subtrees are closed, by dotted path. Documents open fully expanded:
  // the point of the view is that the shape is visible without further
  // clicking.
  let treeClosed = $state<Set<string>>(new Set());
  let treeNodes = $derived(doc ? flattenDocument(doc.value, treeClosed) : []);
</script>

<div
  class="backdrop result-grid"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
  role="presentation"
>
  <div class="popup" role="dialog" aria-modal="true" aria-label={i18n.t('result-cell-dialog')} tabindex="-1">
    <div class="popup-head">
      <span class="popup-col">{col}</span>
      <span class="popup-actions">
        {#if doc}
          <button
            type="button"
            class="ghost"
            onclick={() =>
              (treeClosed = treeClosed.size > 0 ? new Set() : allContainerPaths(doc?.value))}
          >
            {treeClosed.size > 0
              ? i18n.t('cell-tree-expand-all')
              : i18n.t('cell-tree-collapse-all')}
          </button>
        {/if}
        <button type="button" class="ghost" onclick={() => navigator.clipboard.writeText(value)}>
          {i18n.t('cell-copy')}
        </button>
      </span>
    </div>
    {#if doc}
      <div class="tree" role="tree" aria-label={i18n.t('cell-tree')}>
        {#each treeNodes as node (node.path)}
          <div
            class="tree-row"
            role="treeitem"
            aria-level={node.depth + 1}
            aria-expanded={node.hasChildren ? !node.collapsed : undefined}
            aria-selected="false"
            style="padding-left: {node.depth * 1.25}rem"
          >
            {#if node.hasChildren}
              <button
                type="button"
                class="twist"
                aria-label={node.collapsed
                  ? i18n.t('cell-tree-expand', { path: node.path })
                  : i18n.t('cell-tree-collapse', { path: node.path })}
                onclick={() => (treeClosed = toggled(treeClosed, node.path))}
              >
                {node.collapsed ? '▸' : '▾'}
              </button>
            {:else}
              <span class="twist" aria-hidden="true"></span>
            {/if}
            {#if node.label !== ''}<span class="tree-label">{node.label}</span>{/if}
            <span class="tree-value {node.kind}">{node.preview}</span>
          </div>
        {/each}
      </div>
    {:else}
      <pre class="popup-body">{value}</pre>
    {/if}
  </div>
</div>
