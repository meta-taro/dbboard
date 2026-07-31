<script lang="ts">
  import { save, open } from '@tauri-apps/plugin-dialog';
  import { workspace } from '$lib/state/workspace.svelte';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import {
    addConnection,
    updateConnection,
    deleteConnection,
    connectionEditFields,
    exportConnections,
    importConnections,
    probeSshHostKey,
    configPath,
    type ConnectionView,
  } from '$lib/api';
  import {
    emptyForm,
    formForEdit,
    fieldsForKind,
    secretFields,
    validate,
    buildKindInput,
    buildKindEditInput,
    supportsSshTunnel,
    validateSsh,
    validateDsnFields,
    isEditableInApp,
    canProbeHostKey,
    parseSshPort,
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
    DSN_FIELDS,
    SSL_MODES,
    defaultPort,
    schemeFor,
    usesDsnFields,
    type DsnField,
    type SslMode,
  } from '$lib/connections/dsn';
  import {
    isPathField,
    pickerFilters,
    pickerTitle,
    type PathField,
  } from '$lib/connections/file-picker';

  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  type Mode = 'list' | 'form' | 'export' | 'import';
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

  const KIND_LABEL: Record<ConnectionKind, MessageKey> = {
    turso: 'conn-kind-turso',
    d1: 'conn-kind-d1',
    postgres: 'conn-kind-postgres',
    mysql: 'conn-kind-mysql',
    neon: 'conn-kind-neon',
    supabase: 'conn-kind-supabase',
    aurora_dsql: 'conn-kind-aurora_dsql',
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
  };

  const DSN_LABEL: Record<DsnField, MessageKey> = {
    db_host: 'conn-field-db-host',
    db_port: 'conn-field-db-port',
    db_user: 'conn-field-db-user',
    db_password: 'conn-field-db-password',
    db_name: 'conn-field-db-name',
  };

  const SSL_MODE_LABEL: Record<SslMode, MessageKey> = {
    require: 'conn-dsn-ssl-require',
    disable: 'conn-dsn-ssl-disable',
  };

  // A live example of the URL the parts would compose, so the escape hatch
  // shows the exact shape the backend parses for *this* kind.
  const urlExample = $derived(
    `${schemeFor(form.kind)}://user:password@host:${defaultPort(form.kind)}/database`,
  );

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

  // Host-key probe state, separate from `busy`/`error` so a failed lookup does
  // not read as a failed save and does not disable the rest of the form.
  let probing = $state(false);
  let probeError = $state('');

  // Ask the SSH server what its host key is, so the user has something to pin.
  // Only ever runs on this click: the app never contacts a server the user has
  // not asked it to.
  async function fetchFingerprint() {
    probeError = '';
    probing = true;
    try {
      form.ssh_fingerprint = await probeSshHostKey(
        form.ssh_host.trim(),
        parseSshPort(form.ssh_port),
      );
      invalidSsh = invalidSsh.filter((f) => f !== 'ssh_fingerprint');
    } catch (e) {
      probeError = String(e);
    } finally {
      probing = false;
    }
  }

  function resetTransient() {
    error = '';
    info = '';
    invalid = [];
    invalidSsh = [];
    invalidDsn = [];
    probeError = '';
    passphrase = '';
    passphraseConfirm = '';
    importPath = '';
    importFileName = '';
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
        await addConnection(form.id, form.name, buildKindInput(form), buildSshInput(form));
      } else {
        await updateConnection(
          form.id,
          form.name,
          buildKindEditInput(form),
          buildSshEditInput(form),
        );
      }
      await workspace.refreshConnections();
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
      await workspace.refreshConnections();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function startExport() {
    resetTransient();
    mode = 'export';
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
    let path: string | null;
    try {
      path = await save({
        title: i18n.t('conn-export-heading'),
        defaultPath: 'dbboard-connections.dbbx',
        filters: [{ name: i18n.t('conn-manager-title'), extensions: ['dbbx'] }],
      });
    } catch (e) {
      error = String(e);
      return;
    }
    if (!path) return; // user cancelled the dialog
    busy = true;
    try {
      const count = await exportConnections(path, passphrase);
      passphrase = '';
      passphraseConfirm = '';
      info = i18n.t('conn-export-ok', { count });
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
      const report = await importConnections(importPath, passphrase);
      await workspace.refreshConnections();
      passphrase = '';
      let summary = i18n.t('conn-import-ok', {
        imported: report.imported.length,
        skipped: report.skipped.length,
      });
      if (report.skipped.length > 0) {
        summary +=
          ' ' + i18n.t('conn-import-skipped-ids', { ids: report.skipped.join(', ') });
      }
      info = summary;
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
  class="backdrop"
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
          {#if !(f === 'url' && usesDsnFields(form.kind))}
            <label class="field">
              <span class="label">{i18n.t(FIELD_LABEL[f])}</span>
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
              {#if isSecret(f) && editorMode === 'edit'}
                <span class="hint">{i18n.t('conn-secret-keep-hint')}</span>
              {/if}
            </label>
          {/if}
        {/each}

        {#if usesDsnFields(form.kind)}
          <fieldset class="dsn">
            <legend>{i18n.t('conn-dsn-section')}</legend>

            {#if form.use_url}
              <label class="field">
                <span class="label">{i18n.t('conn-field-url')}</span>
                <input
                  class:bad={invalid.includes('url')}
                  type="password"
                  value={form.url}
                  oninput={(e) => setField('url', e.currentTarget.value)}
                  spellcheck="false"
                  autocomplete="off"
                />
                <span class="hint">{i18n.t('conn-dsn-url-example', { example: urlExample })}</span>
                {#if editorMode === 'edit'}
                  <span class="hint">{i18n.t('conn-secret-keep-hint')}</span>
                {/if}
              </label>
            {:else}
              {#each DSN_FIELDS as f (f)}
                <label class="field">
                  <span class="label">{i18n.t(DSN_LABEL[f])}</span>
                  <input
                    class:bad={invalidDsn.includes(f)}
                    type={f === 'db_password' ? 'password' : 'text'}
                    placeholder={f === 'db_port' ? String(defaultPort(form.kind)) : ''}
                    value={form[f]}
                    oninput={(e) => (form[f] = e.currentTarget.value)}
                    spellcheck="false"
                    autocomplete="off"
                  />
                  {#if f === 'db_host' && form.ssh_enabled}
                    <span class="hint">{i18n.t('conn-dsn-host-tunnel-hint')}</span>
                  {/if}
                </label>
              {/each}

              <label class="field">
                <span class="label">{i18n.t('conn-dsn-ssl')}</span>
                <select
                  value={form.db_ssl}
                  onchange={(e) => (form.db_ssl = e.currentTarget.value as SslMode)}
                >
                  {#each SSL_MODES as m (m)}
                    <option value={m}>{i18n.t(SSL_MODE_LABEL[m])}</option>
                  {/each}
                </select>
                <span class="hint">
                  {i18n.t(form.ssh_enabled ? 'conn-dsn-ssl-tunnel-hint' : 'conn-dsn-ssl-hint')}
                </span>
              </label>

              {#if editorMode === 'edit'}
                <p class="note">{i18n.t('conn-dsn-edit-replace-hint')}</p>
              {/if}
            {/if}

            <button type="button" class="linkish" onclick={() => setUrlMode(!form.use_url)}>
              {form.use_url ? i18n.t('conn-dsn-use-fields') : i18n.t('conn-dsn-use-url')}
            </button>
          </fieldset>
        {/if}

        {#if supportsSshTunnel(form.kind)}
          <fieldset class="ssh">
            <legend>{i18n.t('conn-ssh-section')}</legend>
            <label class="check">
              <input
                type="checkbox"
                checked={form.ssh_enabled}
                onchange={(e) => (form.ssh_enabled = e.currentTarget.checked)}
              />
              <span>{i18n.t('conn-ssh-enable')}</span>
            </label>

            {#if form.ssh_enabled}
              <p class="note">{i18n.t('conn-ssh-note')}</p>

              <label class="field">
                <span class="label">{i18n.t('conn-ssh-host')}</span>
                <input
                  class:bad={invalidSsh.includes('ssh_host')}
                  value={form.ssh_host}
                  oninput={(e) => (form.ssh_host = e.currentTarget.value)}
                  spellcheck="false"
                  autocomplete="off"
                />
              </label>

              <label class="field">
                <span class="label">{i18n.t('conn-ssh-port')}</span>
                <input
                  class:bad={invalidSsh.includes('ssh_port')}
                  value={form.ssh_port}
                  oninput={(e) => (form.ssh_port = e.currentTarget.value)}
                  inputmode="numeric"
                  autocomplete="off"
                />
              </label>

              <label class="field">
                <span class="label">{i18n.t('conn-ssh-user')}</span>
                <input
                  class:bad={invalidSsh.includes('ssh_user')}
                  value={form.ssh_user}
                  oninput={(e) => (form.ssh_user = e.currentTarget.value)}
                  spellcheck="false"
                  autocomplete="off"
                />
              </label>

              <label class="field">
                <span class="label">{i18n.t('conn-ssh-auth')}</span>
                <select
                  value={form.ssh_auth_method}
                  onchange={(e) =>
                    (form.ssh_auth_method = e.currentTarget.value as 'key' | 'password')}
                >
                  <option value="key">{i18n.t('conn-ssh-auth-key')}</option>
                  <option value="password">{i18n.t('conn-ssh-auth-password')}</option>
                </select>
              </label>

              {#if form.ssh_auth_method === 'key'}
                <label class="field">
                  <span class="label">{i18n.t('conn-ssh-key-path')}</span>
                  <div class="with-action">
                    <input
                      class:bad={invalidSsh.includes('ssh_key_path')}
                      value={form.ssh_key_path}
                      oninput={(e) => (form.ssh_key_path = e.currentTarget.value)}
                      spellcheck="false"
                      autocomplete="off"
                    />
                    <button
                      type="button"
                      class="ghost"
                      disabled={busy}
                      onclick={() => browseFor('ssh_key_path')}
                    >
                      {i18n.t('conn-browse')}
                    </button>
                  </div>
                  <span class="hint">{i18n.t('conn-ssh-key-path-hint')}</span>
                </label>

                {#if editorMode === 'edit'}
                  <label class="check">
                    <input
                      type="checkbox"
                      checked={form.ssh_key_encrypted}
                      onchange={(e) => (form.ssh_key_encrypted = e.currentTarget.checked)}
                    />
                    <span>{i18n.t('conn-ssh-key-encrypted')}</span>
                  </label>
                {/if}

                {#if editorMode === 'add' || form.ssh_key_encrypted}
                  <label class="field">
                    <span class="label">{i18n.t('conn-ssh-passphrase')}</span>
                    <input
                      class:bad={invalidSsh.includes('ssh_passphrase')}
                      type="password"
                      value={form.ssh_passphrase}
                      oninput={(e) => (form.ssh_passphrase = e.currentTarget.value)}
                      autocomplete="off"
                    />
                    {#if editorMode === 'edit'}
                      <span class="hint">{i18n.t('conn-secret-keep-hint')}</span>
                    {/if}
                  </label>
                {/if}
              {:else}
                <label class="field">
                  <span class="label">{i18n.t('conn-ssh-password')}</span>
                  <input
                    class:bad={invalidSsh.includes('ssh_password')}
                    type="password"
                    value={form.ssh_password}
                    oninput={(e) => (form.ssh_password = e.currentTarget.value)}
                    autocomplete="off"
                  />
                  {#if editorMode === 'edit'}
                    <span class="hint">{i18n.t('conn-secret-keep-hint')}</span>
                  {/if}
                </label>
              {/if}

              <label class="field">
                <span class="label">{i18n.t('conn-ssh-host-key')}</span>
                <select
                  value={form.ssh_host_key_policy}
                  onchange={(e) =>
                    (form.ssh_host_key_policy = e.currentTarget.value as
                      | 'fingerprint'
                      | 'known_hosts')}
                >
                  <option value="fingerprint">{i18n.t('conn-ssh-host-key-fingerprint')}</option>
                  <option value="known_hosts">{i18n.t('conn-ssh-host-key-known-hosts')}</option>
                </select>
                <span class="hint">{i18n.t('conn-ssh-host-key-hint')}</span>
              </label>

              {#if form.ssh_host_key_policy === 'fingerprint'}
                <label class="field">
                  <span class="label">{i18n.t('conn-ssh-fingerprint')}</span>
                  <div class="with-action">
                    <input
                      class:bad={invalidSsh.includes('ssh_fingerprint')}
                      value={form.ssh_fingerprint}
                      oninput={(e) => (form.ssh_fingerprint = e.currentTarget.value)}
                      placeholder="SHA256:…"
                      spellcheck="false"
                      autocomplete="off"
                    />
                    <button
                      type="button"
                      class="ghost"
                      disabled={busy || probing || !canProbeHostKey(form)}
                      onclick={fetchFingerprint}
                    >
                      {probing ? i18n.t('conn-ssh-fetch-busy') : i18n.t('conn-ssh-fetch')}
                    </button>
                  </div>
                  <span class="hint">{i18n.t('conn-ssh-fingerprint-hint')}</span>
                  {#if probeError}<span class="hint bad-text">{probeError}</span>{/if}
                </label>
              {:else}
                <label class="field">
                  <span class="label">{i18n.t('conn-ssh-known-hosts')}</span>
                  <div class="with-action">
                    <input
                      class:bad={invalidSsh.includes('ssh_known_hosts')}
                      value={form.ssh_known_hosts}
                      oninput={(e) => (form.ssh_known_hosts = e.currentTarget.value)}
                      placeholder="~/.ssh/known_hosts"
                      spellcheck="false"
                      autocomplete="off"
                    />
                    <button
                      type="button"
                      class="ghost"
                      disabled={busy}
                      onclick={() => browseFor('ssh_known_hosts')}
                    >
                      {i18n.t('conn-browse')}
                    </button>
                  </div>
                  <span class="hint">{i18n.t('conn-ssh-known-hosts-hint')}</span>
                </label>
              {/if}
            {/if}
          </fieldset>
        {/if}

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
        <div class="actions">
          <button type="button" class="ghost" disabled={busy} onclick={goList}>{i18n.t('conn-cancel')}</button>
          <button type="button" class="primary" disabled={busy || !importPath} onclick={runImport}>
            {i18n.t('conn-import')}
          </button>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: 50;
  }
  .dialog {
    width: min(560px, 94vw);
    max-height: 86vh;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .title {
    margin: 0;
    font-size: var(--text-heading);
    font-weight: 600;
    color: var(--text);
  }
  .icon-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: var(--text-body);
    cursor: pointer;
    padding: 4px 8px;
    border-radius: var(--radius-widget);
  }
  .icon-btn:hover {
    background: var(--bg-surface-alt);
    color: var(--text);
  }

  .banner {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-widget);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }
  .banner.error {
    background: var(--danger-weak, rgba(220, 38, 38, 0.12));
    color: var(--danger);
    font-family: var(--font-mono);
  }
  .banner.info {
    background: var(--accent-weak);
    color: var(--text-accent);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow-y: auto;
    min-height: 0;
  }
  .empty {
    margin: 0;
    padding: var(--space-4);
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-widget);
  }
  .row:hover {
    background: var(--bg-surface-alt);
  }
  .row-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .row-name {
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row-meta {
    font-size: var(--text-hint);
    color: var(--faint);
    font-family: var(--font-mono);
  }
  .row-note {
    font-size: var(--text-hint);
    color: var(--faint);
  }
  .row-path {
    font-family: var(--font-mono);
    word-break: break-all;
  }
  .row-actions {
    display: flex;
    gap: var(--space-2);
    flex: none;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding-top: var(--space-2);
    border-top: 1px solid var(--border);
  }
  .foot-left {
    display: flex;
    gap: var(--space-2);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    overflow-y: auto;
    min-height: 0;
  }
  .sub {
    margin: 0;
    font-size: var(--text-body);
    font-weight: 600;
    color: var(--text);
  }
  .note {
    margin: 0;
    font-size: var(--text-small);
    color: var(--text-muted);
    line-height: 1.5;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .label {
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-muted);
  }
  .hint {
    font-size: var(--text-hint);
    color: var(--faint);
  }
  .hint.bad-text {
    color: var(--danger);
  }
  /* An input paired with the action that fills it in. The button keeps its
     intrinsic width so the field still grows with the dialog. */
  .with-action {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .with-action input {
    flex: 1;
    min-width: 0;
  }
  .with-action button {
    flex: none;
    white-space: nowrap;
  }
  .field input,
  .field select {
    width: 100%;
    background: var(--bg-surface-alt);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 7px 10px;
    font-size: var(--text-body);
  }
  .field input:focus-visible,
  .field select:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  .field input.bad {
    border-color: var(--danger);
  }
  .field input.readonly {
    color: var(--text-muted);
    background: var(--bg-surface);
  }
  .file-name {
    font-family: var(--font-mono);
    font-size: var(--text-small);
    color: var(--text-accent);
    word-break: break-all;
  }

  .ssh,
  .dsn {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .ssh legend,
  .dsn legend {
    padding: 0 var(--space-1);
    font-size: var(--text-hint);
    font-weight: 600;
    color: var(--text-muted);
  }
  .ssh .note,
  .dsn .note {
    margin: 0;
    font-size: var(--text-hint);
    color: var(--faint);
  }
  /* The mode switch is a control, not a call to action — it must not compete
     with Save for attention, so it reads as a link. */
  .linkish {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    font-size: var(--text-hint);
    color: var(--text-accent);
    text-decoration: underline;
    cursor: pointer;
  }
  .linkish:hover {
    color: var(--accent-hover);
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-body);
    color: var(--text);
  }
  .check input {
    width: auto;
    margin: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-1);
  }
  .primary {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
    border: none;
    border-radius: var(--radius-widget);
    padding: 7px 20px;
    cursor: pointer;
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ghost {
    background: var(--bg-surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    padding: 6px 14px;
    font-size: var(--text-small);
    font-weight: 600;
    cursor: pointer;
  }
  .ghost:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .ghost.danger {
    color: var(--danger);
  }
  .ghost.danger:hover:not(:disabled) {
    border-color: var(--danger);
  }
</style>
