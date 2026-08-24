<!--
  Copy an existing connection under a new id (issue #210).
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import { workspace } from '$lib/state/workspace.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { duplicateConnection, type ConnectionView } from '$lib/api';
  import { suggestCopyId, validateCopy, type CopyForm, type CopyField } from '$lib/connections/repair';

  interface Props {
    source: ConnectionView;
    busy: boolean;
    setBusy: (value: boolean) => void;
    onError: (message: string) => void;
    /** Leave for the list. The parent re-reads the list before showing this. */
    onDone: (info: string) => Promise<void> | void;
    onCancel: () => void;
  }
  let { source, busy, setBusy, onError, onDone, onCancel }: Props = $props();

  // Seeded once, on purpose: from here the fields are the operator's to edit,
  // and the panel is mounted fresh for each source, so there is no later
  // `source` to follow.
  let copyForm = $state<CopyForm>(
    untrack(() => ({
      id: suggestCopyId(
        source.id,
        workspace.connections.map((c) => c.id),
      ),
      name: i18n.t('conn-duplicate-name-default', { name: source.name }),
    })),
  );
  let invalidCopy = $state<CopyField[]>([]);

  async function runDuplicate() {
    invalidCopy = validateCopy(
      copyForm,
      workspace.connections.map((c) => c.id),
    );
    if (invalidCopy.length > 0) return;
    const newId = copyForm.id.trim();
    setBusy(true);
    try {
      await duplicateConnection(source.id, newId, copyForm.name.trim());
      await onDone(i18n.t('conn-duplicate-ok', { id: newId }));
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }
</script>

<div class="form">
  <h3 class="sub">{i18n.t('conn-duplicate-title')}</h3>
  <p class="note">{i18n.t('conn-duplicate-lead', { name: source.name })}</p>
  <label class="field">
    <span class="label">{i18n.t('conn-field-id')}</span>
    <input
      class:bad={invalidCopy.includes('id')}
      value={copyForm.id}
      oninput={(e) => (copyForm.id = e.currentTarget.value)}
      autocomplete="off"
    />
    {#if invalidCopy.includes('id') && copyForm.id.trim() !== ''}
      <span class="hint bad-text">{i18n.t('conn-duplicate-id-taken')}</span>
    {/if}
  </label>
  <label class="field">
    <span class="label">{i18n.t('conn-field-name')}</span>
    <input
      class:bad={invalidCopy.includes('name')}
      value={copyForm.name}
      oninput={(e) => (copyForm.name = e.currentTarget.value)}
      autocomplete="off"
    />
  </label>
  <div class="actions">
    <button type="button" class="ghost" disabled={busy} onclick={onCancel}>
      {i18n.t('conn-cancel')}
    </button>
    <button type="button" class="primary" disabled={busy} onclick={runDuplicate}>
      {i18n.t('conn-duplicate-run')}
    </button>
  </div>
</div>
