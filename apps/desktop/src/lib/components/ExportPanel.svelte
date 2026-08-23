<!--
  The connection-bundle export panel (ADR-0105).

  The passphrase fields live here rather than in the dialog above, so that
  leaving the panel unmounts them. That is the whole reason this is a
  component: the parent used to clear the buffers by hand in a reset function
  that also cleared six unrelated things, and a secret that survives because
  somebody remembered to list it is one edit away from surviving because they
  did not.
-->
<script lang="ts">
  import { save } from '@tauri-apps/plugin-dialog';
  import { timestampedFileName } from '$lib/export/filename';
  import { workspace } from '$lib/state/workspace.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { exportConnections } from '$lib/api';
  import { exportSummary } from '$lib/connections/export-report';

  interface Props {
    busy: boolean;
    setBusy: (value: boolean) => void;
    /** Report a failure. The panel stays open so the operator can retry. */
    onError: (message: string) => void;
    /** Leave for the list carrying one sentence of confirmation. */
    onDone: (info: string) => void;
    onCancel: () => void;
  }
  let { busy, setBusy, onError, onDone, onCancel }: Props = $props();

  let passphrase = $state('');
  let passphraseConfirm = $state('');
  // Seeded to every connection, so the panel opens on the behaviour it had
  // before the picker existed and narrowing is the deliberate act.
  let exportIds = $state<string[]>(workspace.connections.map((c) => c.id));

  function toggleExportId(id: string) {
    exportIds = exportIds.includes(id)
      ? exportIds.filter((x) => x !== id)
      : [...exportIds, id];
  }

  async function runExport() {
    if (passphrase !== passphraseConfirm) {
      onError(i18n.t('conn-passphrase-mismatch'));
      return;
    }
    if (passphrase.trim().length === 0) {
      onError(i18n.t('conn-required'));
      return;
    }
    if (exportIds.length === 0) {
      onError(i18n.t('conn-export-none-selected'));
      return;
    }
    let path: string | null;
    try {
      path = await save({
        title: i18n.t('conn-export-heading'),
        defaultPath: timestampedFileName('dbboard-connections', 'dbbx'),
        filters: [{ name: i18n.t('conn-manager-title'), extensions: ['dbbx'] }],
      });
    } catch (e) {
      onError(String(e));
      return;
    }
    if (!path) return; // user cancelled the dialog
    setBusy(true);
    try {
      const report = await exportConnections(path, passphrase, exportIds);
      // Wording rules live in `export-report.ts` so they are testable. The
      // warning about a foreign keychain slot must not read as a failure —
      // the bundle is on disk either way (issue #194).
      onDone(
        exportSummary(report, (key, params) =>
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
  <h3 class="sub">{i18n.t('conn-export-heading')}</h3>
  <p class="note">{i18n.t('conn-bundle-note')}</p>
  <div class="field">
    <span class="label">{i18n.t('conn-export-select')}</span>
    <div class="picker">
      {#each workspace.connections as c (c.id)}
        <label class="pick">
          <input
            type="checkbox"
            checked={exportIds.includes(c.id)}
            onchange={() => toggleExportId(c.id)}
          />
          <span class="pick-name">{c.name}</span>
          <span class="pick-meta">{c.kind} · {c.id}</span>
        </label>
      {/each}
    </div>
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
  <label class="field">
    <span class="label">{i18n.t('conn-passphrase-confirm')}</span>
    <input
      type="password"
      value={passphraseConfirm}
      oninput={(e) => (passphraseConfirm = e.currentTarget.value)}
      autocomplete="off"
    />
  </label>
  <div class="actions">
    <button type="button" class="ghost" disabled={busy} onclick={onCancel}>
      {i18n.t('conn-cancel')}
    </button>
    <button type="button" class="primary" disabled={busy} onclick={runExport}>
      {i18n.t('conn-export')}
    </button>
  </div>
</div>
