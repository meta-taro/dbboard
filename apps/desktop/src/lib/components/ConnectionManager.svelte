<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { workspace } from '$lib/state/workspace.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import {
    addConnection,
    updateConnection,
    deleteConnection,
    moveConnection,
    foreignConnectionRefs,
    connectionEditFields,
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
  import { foreignRefFor } from '$lib/connections/repair';
  import { moveTarget } from '$lib/connections/order';
  import { refreshConnectionList } from '$lib/connections/refresh';
  import DsnFieldset from './DsnFieldset.svelte';
  import SshFieldset from './SshFieldset.svelte';
  import ExportPanel from './ExportPanel.svelte';
  import ImportPanel from './ImportPanel.svelte';
  import DuplicatePanel from './DuplicatePanel.svelte';
  import RepairPanel from './RepairPanel.svelte';
  import '$lib/styles/connection-dialog.css';

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

  // Entries whose saved-secret slot was minted for a *different* connection
  // (issue #213). dbboard never writes that state itself, so this is only ever
  // a hand-edited or pre-ADR-0038 file — but it is silent until an export
  // refuses, which is too late. The list shows it instead.
  let foreignRefs = $state<ForeignRef[]>([]);
  let copySource = $state<ConnectionView | null>(null);
  let repairTarget = $state<{ name: string; ref: ForeignRef } | null>(null);

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
    //
    // Nor are the panels' passphrases, ids and secrets: each panel below owns
    // its own, and leaving unmounts it. A buffer that empties because the
    // markup went away cannot be forgotten here the way these once could.
    copySource = null;
    repairTarget = null;
  }

  /** Leave a panel for the list, carrying one sentence of confirmation. */
  function finishPanel(message: string) {
    goList();
    // goList clears the banners, so the confirmation is set after it.
    info = message;
  }

  /** As `finishPanel`, for the three panels that changed the list itself. */
  async function finishPanelAfterRefresh(message: string) {
    await refreshAll();
    finishPanel(message);
  }

  function setBusy(value: boolean) {
    busy = value;
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

  // Reorder the list the sidebar renders (issue #192). The order lives in the
  // connections file, so this is not a view preference: it is saved, and it
  // travels inside a `.dbbx` bundle.
  async function move(index: number, delta: number) {
    const target = moveTarget(index, delta, workspace.connections.length);
    if (target === null) return;
    const c = workspace.connections[index];
    busy = true;
    error = '';
    try {
      await moveConnection(c.id, target);
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
    mode = 'duplicate';
  }

  function startRepair(c: ConnectionView) {
    const ref = foreignRefFor(foreignRefs, c.id);
    if (!ref) return;
    resetTransient();
    repairTarget = { name: c.name, ref };
    mode = 'repair';
  }

  function startExport() {
    resetTransient();
    mode = 'export';
  }

  function startImport() {
    resetTransient();
    mode = 'import';
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
          {#each workspace.connections as c, i (c.id)}
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
                  class="ghost move"
                  disabled={busy || i === 0}
                  title={i18n.t('conn-move-up')}
                  aria-label={i18n.t('conn-move-up')}
                  onclick={() => move(i, -1)}
                >
                  ▲
                </button>
                <button
                  type="button"
                  class="ghost move"
                  disabled={busy || i === workspace.connections.length - 1}
                  title={i18n.t('conn-move-down')}
                  aria-label={i18n.t('conn-move-down')}
                  onclick={() => move(i, 1)}
                >
                  ▼
                </button>
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
      <ExportPanel
        {busy}
        {setBusy}
        onError={(m) => (error = m)}
        onDone={finishPanel}
        onCancel={goList}
      />
    {:else if mode === 'import'}
      <ImportPanel
        {busy}
        {setBusy}
        onError={(m) => (error = m)}
        onDone={finishPanelAfterRefresh}
        onCancel={goList}
      />
    {:else if mode === 'duplicate' && copySource}
      <DuplicatePanel
        source={copySource}
        {busy}
        {setBusy}
        onError={(m) => (error = m)}
        onDone={finishPanelAfterRefresh}
        onCancel={goList}
      />
    {:else if mode === 'repair' && repairTarget}
      <RepairPanel
        target={repairTarget}
        {busy}
        {setBusy}
        onError={(m) => (error = m)}
        onDone={finishPanelAfterRefresh}
        onCancel={goList}
      />
    {/if}
  </div>
</div>

