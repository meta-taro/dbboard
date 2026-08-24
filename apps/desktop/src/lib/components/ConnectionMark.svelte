<!-- The identity mark of one connection: which server this is (ADR-0126).
     
     A component rather than two `{#if}` blocks because the swatch and the tag
     must always travel together — DESIGN.md calls a row that shows the swatch
     alone a bug, and the only way to keep that true in both the sidebar and
     the connection manager is to make it impossible to render one half. -->
<script lang="ts">
  import { colorVar, type ConnectionMarkView } from '$lib/connections/marks';

  let { mark }: { mark: ConnectionMarkView } = $props();
</script>

<span
  class="mark"
  class:uncoloured={!mark.color}
  style={mark.color ? `--mark: ${colorVar(mark.color)}` : undefined}
>
  {#if mark.color}
    <!-- Decorative: the tag beside it already says everything the colour does,
         so announcing "red" as well would only add noise. -->
    <span class="dot" aria-hidden="true"></span>
  {/if}
  <span class="tag">{mark.tag}</span>
</span>

<style>
  .mark {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    max-width: 12ch;
    padding: 0 6px;
    border: 1px solid color-mix(in srgb, var(--mark) 45%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, var(--mark) 14%, transparent);
    color: var(--mark);
    font-size: var(--text-hint);
    line-height: 1.6;
  }
  /* A tag with no colour still reads; it just borrows the ordinary text
     colours rather than inventing a ninth one. */
  .mark.uncoloured {
    border-color: var(--border);
    background: transparent;
    color: var(--faint);
  }
  .dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--mark);
  }
  .tag {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
