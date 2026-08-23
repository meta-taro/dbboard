<script lang="ts">
  import { save, open } from '@tauri-apps/plugin-dialog';
  import { timestampedFileName } from '$lib/export/filename';
  import { workspace } from '$lib/state/workspace.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import {
    addConnection,
    updateConnection,
    deleteConnection,
    duplicateConnection,
    repairConnectionRef,
    foreignConnectionRefs,
    connectionEditFields,
    exportConnections,
    importConnections,
    configPath,
    type ConnectionView,
    type ForeignRef,
  } from '$lib/api';
  import {
    emptyForm,
    formForEdit,
    keepStoredPassword,
    fieldsForKind,
    secretFields,
    validate,
    buildKindInput,
    buildKindEditInput,
    supportsSshTunnel,
    validateSsh,
    validateDsnFields,
    isEditableInApp,
    buildSshInput,
    buildSshEditInput,
    CONNECTION_KINDS,
    type ConnectionForm,
    type ConnectionKind,
    type FormField,
    type SshFormField,
    type EditorMode,
  } from '$lib/connections/draft';
  import {
    usesDsnFields,
    type DsnField,
  } from '$lib/connections/dsn';
  import {
    isPathField,
    pickerFilters,
    pickerTitle,
    type PathField,
  } from '$lib/connections/file-picker';
  import {
    foreignRefFor,
    suggestCopyId,
    validateCopy,
    secretLabelKey,
    type CopyForm,
    type CopyField,
  } from '$lib/connections/repair';
  import { refreshConnectionList } from '$lib/connections/refresh';
  import DsnFieldset from './DsnFieldset.svelte';
  import SshFieldset from './SshFieldset.svelte';
  import '$lib/styles/connection-dialog.css';
  import { exportSummary } from '$lib/connections/export-report';
  import { importSummary } from '$lib/connections/import-report';

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  type Mode = 'list' | 'form' | 'export' | 'import' | 'duplicate' | 'repair';
  let mode = $state<Mode>('list');
  let editorMode = $state<EditorMode>('add');
  let form = $state<ConnectionForm>(emptyForm());
  let invalid = $state<FormField[]>([]);
  let invalidSsh = $state<SshFormField[]>([]);
  let invalidDsn = $state<DsnField[]>([]);
  let busy = $state(false);
  let error = $state('');
  let info = $state('');

  // Bundle passphrase buffers, cleared on every mode change so a secret never
  // lingers in a hidden form.
  let passphrase = $state('');
  let passphraseConfirm = $state('');
  let importPath = $state('');
  let importFileName = $state('');
  // Which connections the next export includes (ADR-0105). Seeded to all of
  // them on entry, so the panel opens on the behaviour it had before the
  // picker existed and narrowing is the deliberate act.
  let exportIds = $state<string[]>([]);
  // Off by default: replacing an entry destroys a credential the bundle may
  // not carry, so it is never the choice a stray click makes.
  let overwriteExisting = $state(false);

  // Entries whose saved-secret slot was minted for a *different* connection
  // (issue #213). dbboard never writes that state itself, so this is only ever
  // a hand-edited or pre-ADR-0038 file — but it is silent until an export
  // refuses, which is too late. The list shows it instead.
  let foreignRefs = $state<ForeignRef[]>([]);
  let copySource = $state<ConnectionView | null>(null);
  let copyForm = $state<CopyForm>({ id: '', name: '' });
  let invalidCopy = $state<CopyField[]>([]);
  let repairTarget = $state<{ name: string; ref: ForeignRef } | null>(null);
  let repairSecret = $state('');
  let repairSecretMissing = $state(false);

  const KIND_LABEL: Record<ConnectionKind, MessageKey> = {
    turso: 'conn-kind-turso',
    turso_remote: 'conn-kind-turso_remote',
    d1: 'conn-kind-d1',
    postgres: 'conn-kind-postgres',
    mysql: 'conn-kind-mysql',
    neon: 'conn-kind-neon',
    supabase: 'conn-kind-supabase',
    aurora_dsql: 'conn-kind-aurora_dsql',
    aurora_dsql_iam: 'conn-kind-aurora_dsql_iam',
    firestore: 'conn-kind-firestore',
    mongodb: 'conn-kind-mongodb',
  };

  const FIELD_LABEL: Record<FormField, MessageKey> = {
    id: 'conn-field-id',
    name: 'conn-field-name',
    path: 'conn-field-path',
    account_id: 'conn-field-account-id',
    database_id: 'conn-field-database-id',
    base_url: 'conn-field-base-url',
    token: 'conn-field-token',
    url: 'conn-field-url',
    project_id: 'conn-field-project-id',
    service_account: 'conn-field-service-account',
    uri: 'conn-field-uri',
    database: 'conn-field-database',
    endpoint: 'conn-field-endpoint',
    region: 'conn-field-region',
    username: 'conn-field-username',
    access_key_id: 'conn-field-access-key-id',
    secret_access_key: 'conn-field-secret-access-key',
  };

  // "Not editable here" is only half an answer; the other half is *where*. The
  // path is resolved lazily and only when a row actually needs it, so the
  // common case (every connection editable) costs nothing.
  let configFilePath = $state('');
  $effect(() => {
    const needsPath = workspace.connections.some((c) => !isEditableInApp(c.kind));
    if (needsPath && !configFilePath) {
      configPath()
        .then((p) => (configFilePath = p))
        .catch(() => {
          // Without the path the note still says the entry lives in
          // connections.toml — less helpful, but not wrong.
        });
    }
  });

  // Not reactive on purpose: the list is fetched once per open of this dialog,
  // then refreshed explicitly by whatever changed it.
  let refsLoaded = false;
  $effect(() => {
    if (refsLoaded) return;
    refsLoaded = true;
    void loadForeignRefs();
  });

  async function loadForeignRefs() {
    try {
      foreignRefs = await foreignConnectionRefs();
    } catch {
      // A list we cannot check is shown without badges rather than not at all.
      // Export still refuses a bundle that carries one, so nothing slips out.
    }
  }

  async function refreshAll() {
    await refreshConnectionList({
      connections: () => workspace.refreshConnections(),
      foreignRefs: loadForeignRefs,
    });
  }

  // Fill a path field from the native open dialog. The user is looking at the
  // file in Explorer while filling this form; asking them to transcribe the
  // path is how a stray quote or the wrong slash gets in.
  async function browseFor(field: PathField) {
    const filters = pickerFilters(field);
    try {
      const picked = await open({
        title: i18n.t(pickerTitle(field)),
        multiple: false,
        directory: false,
        ...(filters.length > 0 ? { filters } : {}),
      });
      if (typeof picked !== 'string') return; // cancelled
      form[field] = picked;
      invalid = invalid.filter((f) => f !== field);
      invalidSsh = invalidSsh.filter((f) => f !== field);
    } catch (e) {
      error = String(e);
    }
  }

  function resetTransient() {
    error = '';
    info = '';
    invalid = [];
    invalidSsh = [];
    invalidDsn = [];
    // The host-key probe's error is not listed: it lives in SshFieldset,
    // which is unmounted at every point this runs — each of the seven
    // callers is either leaving the form or entering it from the list.
    passphrase = '';
    passphraseConfirm = '';
    importPath = '';
    importFileName = '';
    exportIds = [];
    overwriteExisting = false;
    copySource = null;
    copyForm = { id: '', name: '' };
    invalidCopy = [];
    repairTarget = null;
    repairSecret = '';
    repairSecretMissing = false;
  }

  function goList() {
    resetTransient();
    mode = 'list';
  }

  function startAdd() {
    resetTransient();
    form = emptyForm();
    editorMode = 'add';
    mode = 'form';
  }

  async function startEdit(c: ConnectionView) {
    resetTransient();
    busy = true;
    try {
      const fields = await connectionEditFields(c.id);
      form = formForEdit(c.id, c.name, fields);
      editorMode = 'edit';
      mode = 'form';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function isSecret(f: FormField): boolean {
    return secretFields(form.kind).includes(f);
  }

  function setField(f: FormField, value: string) {
    form[f] = value;
  }

  // Switching modes clears the other side's stale highlights: the fields it
  // flagged are no longer on screen, so keeping them would strand a red border
  // the user cannot reach.
  function setUrlMode(useUrl: boolean) {
    form.use_url = useUrl;
    invalid = invalid.filter((f) => f !== 'url');
    invalidDsn = [];
  }

  async function saveForm() {
    invalid = validate(form, editorMode);
    invalidSsh = validateSsh(form, editorMode);
    invalidDsn = validateDsnFields(form);
    if (invalid.length > 0 || invalidSsh.length > 0 || invalidDsn.length > 0) return;
    busy = true;
    error = '';
    try {
      if (editorMode === 'add') {
        await addConnection(
          form.id,
          form.name,
          buildKindInput(form),
          buildSshInput(form),
          form.mcp_write,
          form.mcp_alias,
        );
      } else {
        await updateConnection(
          form.id,
          form.name,
          buildKindEditInput(form),
          buildSshEditInput(form),
          keepStoredPassword(form, editorMode),
          form.mcp_write,
          form.mcp_alias,
        );
      }
      await refreshAll();
      goList();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function remove(c: ConnectionView) {
    if (!confirm(i18n.t('conn-delete-confirm', { name: c.name }))) return;
    busy = true;
    error = '';
    try {
      await deleteConnection(c.id);
      await refreshAll();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function startDuplicate(c: ConnectionView) {
    resetTransient();
    copySource = c;
    copyForm = {
      id: suggestCopyId(
        c.id,
        workspace.connections.map((x) => x.id),
      ),
      name: i18n.t('conn-duplicate-name-default', { name: c.name }),
    };
    mode = 'duplicate';
  }

  async function runDuplicate() {
    if (!copySource) return;
    invalidCopy = validateCopy(
      copyForm,
      workspace.connections.map((c) => c.id),
    );
    if (invalidCopy.length > 0) return;
    const newId = copyForm.id.trim();
    busy = true;
    error = '';
    try {
      await duplicateConnection(copySource.id, newId, copyForm.name.trim());
      await refreshAll();
      // goList clears the banners, so the confirmation is set after it.
      goList();
      info = i18n.t('conn-duplicate-ok', { id: newId });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function startRepair(c: ConnectionView) {
    const ref = foreignRefFor(foreignRefs, c.id);
    if (!ref) return;
    resetTransient();
    repairTarget = { name: c.name, ref };
    mode = 'repair';
  }

  async function runRepair() {
    if (!repairTarget) return;
    if (repairSecret === '') {
      repairSecretMissing = true;
      return;
    }
    const { id, key_ref } = repairTarget.ref;
    busy = true;
    error = '';
    try {
      await repairConnectionRef(id, key_ref, repairSecret);
      // The value is in the keychain now; there is no reason to keep a copy in
      // a field the user cannot see.
      repairSecret = '';
      await refreshAll();
      goList();
      info = i18n.t('conn-repair-ok', { id });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function startExport() {
    resetTransient();
    exportIds = workspace.connections.map((c) => c.id);
    mode = 'export';
  }

  function toggleExportId(id: string) {
    exportIds = exportIds.includes(id)
      ? exportIds.filter((x) => x !== id)
      : [...exportIds, id];
  }

  function startImport() {
    resetTransient();
    mode = 'import';
  }

  async function runExport() {
    error = '';
    if (passphrase !== passphraseConfirm) {
      error = i18n.t('conn-passphrase-mismatch');
      return;
    }
    if (passphrase.trim().length === 0) {
      error = i18n.t('conn-required');
      return;
    }
    if (exportIds.length === 0) {
      error = i18n.t('conn-export-none-selected');
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
      error = String(e);
      return;
    }
    if (!path) return; // user cancelled the dialog
    busy = true;
    try {
      const report = await exportConnections(path, passphrase, exportIds);
      passphrase = '';
      passphraseConfirm = '';
      // Wording rules live in `export-report.ts` so they are testable. The
      // warning about a foreign keychain slot must not read as a failure —
      // the bundle is on disk either way (issue #194).
      info = exportSummary(report, (key, params) =>
        i18n.t(key as Parameters<typeof i18n.t>[0], params),
      ).join(' ');
      mode = 'list';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function chooseImportFile() {
    error = '';
    try {
      const picked = await open({
        title: i18n.t('conn-import-heading'),
        multiple: false,
        directory: false,
        filters: [{ name: i18n.t('conn-manager-title'), extensions: ['dbbx'] }],
      });
      if (typeof picked === 'string') {
        importPath = picked;
        importFileName = picked.split(/[\\/]/).pop() ?? picked;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function runImport() {
    error = '';
    if (!importPath) {
      error = i18n.t('conn-required');
      return;
    }
    busy = true;
    try {
      const report = await importConnections(importPath, passphrase, overwriteExisting);
      await refreshAll();
      passphrase = '';
      // Wording rules live in `import-report.ts` so they are testable; which
      // reason gets which sentence is the whole substance of ADR-0112.
      info = importSummary(report, (key, params) =>
        i18n.t(key as Parameters<typeof i18n.t>[0], params),
      ).join(' ');
      mode = 'list';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (mode === 'list') onClose();
      else goList();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="backdrop conn-dialog"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
  role="presentation"
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={i18n.t('conn-manager-title')}
  >
    <header class="head">
      <h2 class="title">{i18n.t('conn-manager-title')}</h2>
      <button type="button" class="icon-btn" onclick={onClose} title={i18n.t('conn-close')}>
        ✕
      </button>
    </header>

    {#if error}<p class="banner error">{error}</p>{/if}
    {#if info && mode === 'list'}<p class="banner info">{info}</p>{/if}

    {#if mode === 'list'}
      <div class="list">
        {#if workspace.connections.length === 0}
          <p class="empty">{i18n.t('conn-empty')}</p>
        {:else}
          {#each workspace.connections as c (c.id)}
            <div class="row">
              <div class="row-main">
                <span class="row-name">{c.name}</span>
                <span class="row-meta">{c.kind} · {c.id}</span>
                {#if foreignRefFor(foreignRefs, c.id)}
                  {@const fr = foreignRefFor(foreignRefs, c.id)}
                  <span
                    class="row-warn"
                    title={i18n.t('conn-foreign-note', {
                      ref: fr?.key_ref ?? '',
                      owner: fr?.owner ?? '',
                    })}
                  >
                    {i18n.t('conn-foreign-badge')}
                  </span>
                {/if}
                {#if !isEditableInApp(c.kind)}
                  <span class="row-note">
                    {i18n.t('conn-edit-toml-only')}
                    {#if configFilePath}
                      <code class="row-path">{configFilePath}</code>
                    {/if}
                  </span>
                {/if}
              </div>
              <div class="row-actions">
                <button
                  type="button"
                  class="ghost"
                  disabled={busy || !isEditableInApp(c.kind)}
                  title={isEditableInApp(c.kind) ? undefined : i18n.t('conn-edit-toml-only')}
                  onclick={() => startEdit(c)}
                >
                  {i18n.t('conn-edit')}
                </button>
                {#if foreignRefFor(foreignRefs, c.id)}
                  <button type="button" class="ghost" disabled={busy} onclick={() => startRepair(c)}>
                    {i18n.t('conn-repair')}
                  </button>
                {:else}
                  <button
                    type="button"
                    class="ghost"
                    disabled={busy}
                    onclick={() => startDuplicate(c)}
                  >
                    {i18n.t('conn-duplicate')}
                  </button>
                {/if}
                <button type="button" class="ghost danger" disabled={busy} onclick={() => remove(c)}>
                  {i18n.t('conn-delete')}
                </button>
              </div>
            </div>
          {/each}
        {/if}
      </div>

      <footer class="foot">
        <div class="foot-left">
          <button type="button" class="ghost" onclick={startImport}>{i18n.t('conn-import')}</button>
          <button
            type="button"
            class="ghost"
            disabled={workspace.connections.length === 0}
            onclick={startExport}
          >
            {i18n.t('conn-export')}
          </button>
        </div>
        <button type="button" class="primary" onclick={startAdd}>{i18n.t('conn-add')}</button>
      </footer>
    {:else if mode === 'form'}
      <div class="form">
        <h3 class="sub">
          {editorMode === 'add' ? i18n.t('conn-add-title') : i18n.t('conn-edit-title')}
        </h3>

        {#if editorMode === 'add'}
          <label class="field">
            <span class="label">{i18n.t('conn-field-id')}</span>
            <input
              class:bad={invalid.includes('id')}
              value={form.id}
              oninput={(e) => setField('id', e.currentTarget.value)}
              spellcheck="false"
            />
            <span class="hint">{i18n.t('conn-field-id-hint')}</span>
          </label>
        {/if}

        <label class="field">
          <span class="label">{i18n.t('conn-field-name')}</span>
          <input
            class:bad={invalid.includes('name')}
            value={form.name}
            oninput={(e) => setField('name', e.currentTarget.value)}
          />
        </label>

        <label class="field">
          <span class="label">{i18n.t('conn-field-kind')}</span>
          {#if editorMode === 'add'}
            <select value={form.kind} onchange={(e) => (form.kind = e.currentTarget.value as ConnectionKind)}>
              {#each CONNECTION_KINDS as k (k)}
                <option value={k}>{i18n.t(KIND_LABEL[k])}</option>
              {/each}
            </select>
          {:else}
            <input class="readonly" value={i18n.t(KIND_LABEL[form.kind])} readonly />
          {/if}
        </label>

        <!-- The DSN kinds render their credential in the Server fieldset below,
             either as parts or as one URL; every other field is plain. -->
        {#each fieldsForKind(form.kind) as f (f)}
          <!-- The emulator toggle sits directly above the credential it
               replaces, and hides it: a blank credential box already means
               "keep the stored one" on edit, so leaving both visible would
               offer two contradictory ways to say the same thing. Rendered
               inside the loop so add and edit cannot drift apart. -->
          {#if f === 'service_account'}
            <label class="check">
              <input
                type="checkbox"
                checked={form.use_emulator}
                onchange={(e) => (form.use_emulator = e.currentTarget.checked)}
              />
              <span>{i18n.t('conn-firestore-emulator')}</span>
            </label>
            <span class="hint">{i18n.t('conn-firestore-emulator-hint')}</span>
          {/if}
          {#if !(f === 'url' && usesDsnFields(form.kind)) && !(f === 'service_account' && form.use_emulator)}
            <label class="field">
              <span class="label">{i18n.t(FIELD_LABEL[f])}</span>
              {#if f === 'service_account'}
                <!-- A textarea, not a masked input: the service-account key is
                     a multi-line JSON document, and a paste nobody can read
                     back is a paste nobody can tell went wrong. It still never
                     leaves the keychain once saved. -->
                <textarea
                  class:bad={invalid.includes(f)}
                  rows="4"
                  value={form[f]}
                  oninput={(e) => setField(f, e.currentTarget.value)}
                  spellcheck="false"
                  autocomplete="off"
                ></textarea>
                <span class="hint">{i18n.t('conn-field-service-account-hint')}</span>
              {:else}
                <div class="with-action">
                  <input
                    class:bad={invalid.includes(f)}
                    type={isSecret(f) ? 'password' : 'text'}
                    value={form[f]}
                    oninput={(e) => setField(f, e.currentTarget.value)}
                    spellcheck="false"
                    autocomplete="off"
                  />
                  {#if isPathField(f)}
                    <button
                      type="button"
                      class="ghost"
                      disabled={busy}
                      onclick={() => browseFor(f)}
                    >
                      {i18n.t('conn-browse')}
                    </button>
                  {/if}
                </div>
              {/if}
              {#if f === 'database_id' && form.kind === 'firestore'}
                <span class="hint">{i18n.t('conn-firestore-database-hint')}</span>
              {/if}
              <!-- `uri` belongs to MongoDB alone, so it needs no kind guard. It
                   is masked like any other secret; the hint says why the
                   password is in it rather than in its own box. -->
              {#if f === 'uri'}
                <span class="hint">{i18n.t('conn-field-uri-hint')}</span>
              {/if}
              <!-- `database` is shared with Aurora DSQL (IAM), where it is
                   required and means something else entirely — hence the guard. -->
              {#if f === 'database' && form.kind === 'mongodb'}
                <span class="hint">{i18n.t('conn-mongodb-database-hint')}</span>
              {/if}
              <!-- Aurora DSQL (IAM) fields belong to that kind alone. -->
              {#if f === 'endpoint'}
                <span class="hint">{i18n.t('conn-field-endpoint-hint')}</span>
              {/if}
              {#if f === 'username'}
                <span class="hint">{i18n.t('conn-field-username-hint')}</span>
              {/if}
              {#if f === 'access_key_id'}
                <span class="hint">{i18n.t('conn-field-access-key-id-hint')}</span>
              {/if}
              {#if f === 'secret_access_key'}
                <span class="hint">{i18n.t('conn-field-secret-access-key-hint')}</span>
              {/if}
              {#if isSecret(f) && editorMode === 'edit'}
                <span class="hint">{i18n.t('conn-secret-keep-hint')}</span>
              {/if}
            </label>
          {/if}
        {/each}

        {#if usesDsnFields(form.kind)}
          <DsnFieldset {form} {invalid} {invalidDsn} {editorMode} {setField} {setUrlMode} />
        {/if}

        {#if supportsSshTunnel(form.kind)}
          <SshFieldset
            {form}
            {invalidSsh}
            {editorMode}
            {busy}
            {browseFor}
            clearInvalid={(f) => (invalidSsh = invalidSsh.filter((x) => x !== f))}
          />
        {/if}

        <!-- Rendered in both modes on purpose: an edit form without this
             toggle would send no opinion and the gate would look absent,
             which is the opposite of what a permission screen should do. -->
        <fieldset class="ssh">
          <legend>{i18n.t('conn-mcp-section')}</legend>
          <label class="check">
            <input
              type="checkbox"
              checked={form.mcp_write}
              onchange={(e) => (form.mcp_write = e.currentTarget.checked)}
            />
            <span>{i18n.t('conn-mcp-write')}</span>
          </label>
          <p class="note">{i18n.t('conn-mcp-write-hint')}</p>
          <label class="field">
            <span class="label">{i18n.t('conn-mcp-alias')}</span>
            <input
              value={form.mcp_alias}
              oninput={(e) => (form.mcp_alias = e.currentTarget.value)}
              placeholder={i18n.t('conn-mcp-alias-placeholder')}
              spellcheck="false"
            />
            <span class="hint">{i18n.t('conn-mcp-alias-hint')}</span>
          </label>
        </fieldset>

        <div class="actions">
          <button type="button" class="ghost" disabled={busy} onclick={goList}>
            {i18n.t('conn-cancel')}
          </button>
          <button type="button" class="primary" disabled={busy} onclick={saveForm}>
            {i18n.t('conn-save')}
          </button>
        </div>
      </div>
    {:else if mode === 'export'}
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
          <input type="password" value={passphrase} oninput={(e) => (passphrase = e.currentTarget.value)} autocomplete="off" />
        </label>
        <label class="field">
          <span class="label">{i18n.t('conn-passphrase-confirm')}</span>
          <input type="password" value={passphraseConfirm} oninput={(e) => (passphraseConfirm = e.currentTarget.value)} autocomplete="off" />
        </label>
        <div class="actions">
          <button type="button" class="ghost" disabled={busy} onclick={goList}>{i18n.t('conn-cancel')}</button>
          <button type="button" class="primary" disabled={busy} onclick={runExport}>{i18n.t('conn-export')}</button>
        </div>
      </div>
    {:else if mode === 'import'}
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
          <input type="password" value={passphrase} oninput={(e) => (passphrase = e.currentTarget.value)} autocomplete="off" />
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
          <button type="button" class="ghost" disabled={busy} onclick={goList}>{i18n.t('conn-cancel')}</button>
          <button type="button" class="primary" disabled={busy || !importPath} onclick={runImport}>
            {i18n.t('conn-import')}
          </button>
        </div>
      </div>
    {:else if mode === 'duplicate'}
      <div class="form">
        <h3 class="sub">{i18n.t('conn-duplicate-title')}</h3>
        <p class="note">{i18n.t('conn-duplicate-lead', { name: copySource?.name ?? '' })}</p>
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
          <button type="button" class="ghost" disabled={busy} onclick={goList}>
            {i18n.t('conn-cancel')}
          </button>
          <button type="button" class="primary" disabled={busy} onclick={runDuplicate}>
            {i18n.t('conn-duplicate-run')}
          </button>
        </div>
      </div>
    {:else if mode === 'repair'}
      <div class="form">
        <h3 class="sub">{i18n.t('conn-repair-title')}</h3>
        <p class="note">
          {i18n.t('conn-repair-lead', {
            name: repairTarget?.name ?? '',
            owner: repairTarget?.ref.owner ?? '',
          })}
        </p>
        <label class="field">
          <span class="label">
            {i18n.t(secretLabelKey(repairTarget?.ref.key_ref ?? '') as MessageKey)}
          </span>
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
          <button type="button" class="ghost" disabled={busy} onclick={goList}>
            {i18n.t('conn-cancel')}
          </button>
          <button type="button" class="primary" disabled={busy} onclick={runRepair}>
            {i18n.t('conn-repair-run')}
          </button>
        </div>
      </div>
    {/if}
  </div>
</div>

