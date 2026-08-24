<!-- The SSH tunnel half of the connection form.
     Split out of ConnectionManager.svelte, which was 1,617 lines against an
     800-line limit. This is the largest self-contained piece: it owns the
     host-key probe outright, and everything else it needs arrives as a prop.
     Styling comes from $lib/styles/connection-dialog.css, which the dialog
     already loads — see the note at the top of that file. -->
<script lang="ts">
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { probeSshHostKey } from '$lib/api';
  import {
    canProbeHostKey,
    parseSshPort,
    type ConnectionForm,
    type EditorMode,
    type SshFormField,
  } from '$lib/connections/draft';

  interface Props {
    /** The draft being edited. Mutated in place — it is the parent's `$state`. */
    form: ConnectionForm;
    /** Which SSH fields failed the last validate, for the red outlines. */
    invalidSsh: SshFormField[];
    /** Add and edit differ over secrets: add always asks, edit keeps unless told. */
    editorMode: EditorMode;
    /** A save or delete is running; the whole form is inert. */
    busy: boolean;
    /** Fill a path field from the native open dialog. Shared with the other
     *  path fields on the form, so it stays with the parent. */
    browseFor: (field: 'ssh_key_path' | 'ssh_known_hosts') => void;
    /** Drop one field from the parent's invalid list once the user fixes it. */
    clearInvalid: (field: SshFormField) => void;
  }

  let { form, invalidSsh, editorMode, busy, browseFor, clearInvalid }: Props = $props();

  // Probe state, separate from the parent's `busy`/`error` so a failed lookup
  // does not read as a failed save and does not disable the rest of the form.
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
      clearInvalid('ssh_fingerprint');
    } catch (e) {
      probeError = String(e);
    } finally {
      probing = false;
    }
  }
</script>

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
