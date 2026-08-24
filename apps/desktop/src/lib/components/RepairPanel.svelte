<!--
  Re-mint a connection's keychain slot when it names another entry's
  (issue #213). dbboard never writes that state itself, so reaching this panel
  means a hand-edited or pre-ADR-0038 file.
-->
<script lang="ts">
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import { repairConnectionRef, type ForeignRef } from '$lib/api';
  import { secretLabelKey } from '$lib/connections/repair';

  interface Props {
    target: { name: string; ref: ForeignRef };
    busy: boolean;
    setBusy: (value: boolean) => void;
    onError: (message: string) => void;
    /** Leave for the list. The parent re-reads the list before showing this. */
    onDone: (info: string) => Promise<void> | void;
    onCancel: () => void;
  }
  let { target, busy, setBusy, onError, onDone, onCancel }: Props = $props();

  let repairSecret = $state('');
  let repairSecretMissing = $state(false);

  async function runRepair() {
    if (repairSecret === '') {
      repairSecretMissing = true;
      return;
    }
    const { id, key_ref } = target.ref;
    setBusy(true);
    try {
      await repairConnectionRef(id, key_ref, repairSecret);
      // The value is in the keychain now; there is no reason to keep a copy in
      // a field the user cannot see.
      repairSecret = '';
      await onDone(i18n.t('conn-repair-ok', { id }));
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }
</script>

<div class="form">
  <h3 class="sub">{i18n.t('conn-repair-title')}</h3>
  <p class="note">
    {i18n.t('conn-repair-lead', { name: target.name, owner: target.ref.owner })}
  </p>
  <label class="field">
    <span class="label">{i18n.t(secretLabelKey(target.ref.key_ref) as MessageKey)}</span>
    <input
      type="password"
      class:bad={repairSecretMissing}
      value={repairSecret}
      oninput={(e) => {
        repairSecret = e.currentTarget.value;
        repairSecretMissing = false;
      }}
      autocomplete="off"
    />
    {#if repairSecretMissing}
      <span class="hint bad-text">{i18n.t('conn-repair-secret-required')}</span>
    {/if}
  </label>
  <div class="actions">
    <button type="button" class="ghost" disabled={busy} onclick={onCancel}>
      {i18n.t('conn-cancel')}
    </button>
    <button type="button" class="primary" disabled={busy} onclick={runRepair}>
      {i18n.t('conn-repair-run')}
    </button>
  </div>
</div>
