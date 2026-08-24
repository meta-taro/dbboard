<!--
  The connection-bundle import panel (ADR-0112).

  Like ExportPanel, this owns its passphrase so that leaving unmounts it.
-->
<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { importConnections } from '$lib/api';
  import { importSummary } from '$lib/connections/import-report';

  interface Props {
    busy: boolean;
    setBusy: (value: boolean) => void;
    onError: (message: string) => void;
    /** Leave for the list. The parent re-reads the list before showing this. */
    onDone: (info: string) => Promise<void> | void;
    onCancel: () => void;
  }
  let { busy, setBusy, onError, onDone, onCancel }: Props = $props();

  let passphrase = $state('');
  let importPath = $state('');
  let importFileName = $state('');
  // Off by default: replacing an entry destroys a credential the bundle may
  // not carry, so it is never the choice a stray click makes.
  let overwriteExisting = $state(false);

  async function chooseImportFile() {
    try {
      const picked = await open({
        title: i18n.t('conn-import-heading'),
        multiple: false,
        directory: false,
        filters: [{ name: i18n.t('conn-manager-title'), extensions: ['dbbx'] }],
      });
      if (typeof picked === 'string') {
        importPath = picked;
        importFileName = picked.split(/[\/]/).pop() ?? picked;
      }
    } catch (e) {
      onError(String(e));
    }
  }

  async function runImport() {
    if (!importPath) {
      onError(i18n.t('conn-required'));
      return;
    }
    setBusy(true);
    try {
      const report = await importConnections(importPath, passphrase, overwriteExisting);
      // Wording rules live in `import-report.ts` so they are testable; which
      // reason gets which sentence is the whole substance of ADR-0112.
      await onDone(
        importSummary(report, (key, params) =>
          i18n.t(key as Parameters<typeof i18n.t>[0], params),
        ).join(' '),
      );
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }
</script>

<div class="form">
  <h3 class="sub">{i18n.t('conn-import-heading')}</h3>
  <div class="field">
    <button type="button" class="ghost" disabled={busy} onclick={chooseImportFile}>
      {i18n.t('conn-choose-file')}
    </button>
    {#if importFileName}<code class="file-name">{importFileName}</code>{/if}
  </div>
  <label class="field">
    <span class="label">{i18n.t('conn-passphrase')}</span>
    <input
      type="password"
      value={passphrase}
      oninput={(e) => (passphrase = e.currentTarget.value)}
      autocomplete="off"
    />
  </label>
  <label class="pick">
    <input
      type="checkbox"
      checked={overwriteExisting}
      onchange={(e) => (overwriteExisting = e.currentTarget.checked)}
    />
    <span class="pick-name">{i18n.t('conn-import-overwrite')}</span>
    <span class="pick-meta">{i18n.t('conn-import-overwrite-note')}</span>
  </label>
  <div class="actions">
    <button type="button" class="ghost" disabled={busy} onclick={onCancel}>
      {i18n.t('conn-cancel')}
    </button>
    <button type="button" class="primary" disabled={busy || !importPath} onclick={runImport}>
      {i18n.t('conn-import')}
    </button>
  </div>
</div>
