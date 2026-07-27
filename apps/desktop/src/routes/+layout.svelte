<script lang="ts">
  import { onMount } from 'svelte';
  import '$lib/styles/tokens.css';
  import { theme } from '$lib/theme/theme.svelte';
  import TopBar from '$lib/components/TopBar.svelte';

  let { children } = $props();

  // Read the persisted choice and start following the OS when on Auto. The
  // pre-paint script in app.html has already stamped an explicit choice, so
  // this only reconciles state and wires the live OS listener.
  onMount(() => theme.init());
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
    /* The app shell inside owns its scroll regions (sidebar list, result
       grid), so the body itself never scrolls. */
    overflow: hidden;
  }
</style>
