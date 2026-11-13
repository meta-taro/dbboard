<!--
  Hosts the agent's own cursor (ADR-0177) and answers the MCP `screen` verb.
  Renders nothing itself: the cursor draws straight onto the page so it sits
  above every dialog and toast.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import { AgentCursor } from '$lib/agent/cursor';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { uiCommands } from '$lib/ui-command/bus';
  import { workspace, type MainTab } from '$lib/state/workspace.svelte';

  interface Props {
    /** The tabs drawn right now; `go` may switch only to one of these. */
    tabs: MainTab[];
  }
  let { tabs }: Props = $props();

  const cursor = new AgentCursor(document, {
    get tag() {
      return i18n.t('agent-cursor-tag');
    },
    goTo: (hash) => {
      const tab = hash.slice(1) as MainTab;
      if (!tabs.includes(tab)) return false;
      workspace.setTab(tab);
      return true;
    },
  });

  const release = uiCommands.on('screen', (command) => cursor.perform(command.step));

  onDestroy(() => {
    release();
    cursor.dispose();
  });
</script>
