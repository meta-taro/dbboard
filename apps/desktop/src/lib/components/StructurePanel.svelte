<script lang="ts">
  import { workspace } from '$lib/state/workspace.svelte';
  // Column listing + local notes land in the next task (#20). For now the panel
  // reflects the selection so the shell wiring is verifiable end to end.
</script>

<div class="panel">
  {#if workspace.selectedTable}
    <h2 class="title">{workspace.key(workspace.selectedTable)}</h2>
    <p class="hint">Column details and notes are coming next.</p>
  {:else}
    <div class="empty">
      <p>Select a table from the sidebar to inspect its structure.</p>
    </div>
  {/if}
</div>

<style>
  .panel {
    padding: var(--space-5);
    min-height: 0;
  }
  .title {
    margin: 0 0 var(--space-2);
    font-size: var(--text-heading);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--text);
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
</style>
