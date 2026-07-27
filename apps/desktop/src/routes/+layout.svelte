<script lang="ts">
  import TopBar from '$lib/components/TopBar.svelte';
  let { children } = $props();
</script>

<!-- Frameless shell: custom title bar on top, routed page fills the rest. -->
<div class="shell">
  <TopBar />
  <div class="body">
    {@render children()}
  </div>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
</style>
