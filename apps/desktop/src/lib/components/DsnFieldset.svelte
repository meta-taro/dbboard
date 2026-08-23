<!-- The host/port/user/password/database half of the connection form, plus
     the URL escape hatch and the TLS select that spans both.
     Split out of ConnectionManager.svelte along with SshFieldset, for the
     same reason: the file was 1,617 lines against an 800-line limit. The
     four derived values below are used nowhere else, so they came too.
     Styling comes from $lib/styles/connection-dialog.css. -->
<script lang="ts">
  import { i18n } from '$lib/i18n/i18n.svelte';
  import type { MessageKey } from '$lib/i18n/messages';
  import {
    DSN_FIELDS,
    SSL_MODES,
    defaultPort,
    schemeFor,
    sslModeFromUrl,
    withSslMode,
    type DsnField,
    type SslMode,
  } from '$lib/connections/dsn';
  import type { ConnectionForm, EditorMode, FormField } from '$lib/connections/draft';

  interface Props {
    /** The draft being edited. Mutated in place — it is the parent's `$state`. */
    form: ConnectionForm;
    /** Fields the last validate rejected. `url` lives here, not in `invalidDsn`. */
    invalid: FormField[];
    /** The parts-mode fields the last validate rejected. */
    invalidDsn: DsnField[];
    /** Add and edit differ over secrets: add always asks, edit keeps unless told. */
    editorMode: EditorMode;
    /** Write one form field. The parent's, because other fields use it too. */
    setField: (field: FormField, value: string) => void;
    /** Switch between URL and parts. The parent's, because it clears the
     *  highlights the other side left behind. */
    setUrlMode: (useUrl: boolean) => void;
  }

  let { form, invalid, invalidDsn, editorMode, setField, setUrlMode }: Props = $props();

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

  // The TLS select drives two different stores depending on the entry mode: in
  // parts mode it is its own field, in URL mode it is a view of the query
  // string the user typed. Reading it back out of the URL (rather than keeping
  // a shadow copy) means a hand-written `?ssl-mode=…` is never contradicted by
  // what the select shows.
  const sslValue = $derived(form.use_url ? sslModeFromUrl(form.url) : form.db_ssl);

  // On edit, a blank URL means "keep the stored credential" — there is no URL
  // here to rewrite, so the select would silently do nothing.
  const sslLocked = $derived(form.use_url && form.url.trim().length === 0);

  const sslHint = $derived<MessageKey>(
    sslLocked
      ? 'conn-dsn-ssl-locked-hint'
      : form.ssh_enabled
        ? 'conn-dsn-ssl-tunnel-hint'
        : 'conn-dsn-ssl-hint',
  );

  function onSslChange(e: Event & { currentTarget: HTMLSelectElement }) {
    const mode = e.currentTarget.value as SslMode;
    if (form.use_url) {
      setField('url', withSslMode(form.kind, form.url, mode));
    } else {
      form.db_ssl = mode;
    }
  }

  // A live example of the URL the parts would compose, so the escape hatch
  // shows the exact shape the backend parses for *this* kind.
  const urlExample = $derived(
    `${schemeFor(form.kind)}://user:password@host:${defaultPort(form.kind)}/database`,
  );
</script>

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
        {#if f === 'db_password' && editorMode === 'edit'}
          <span class="hint">{i18n.t('conn-dsn-edit-password-hint')}</span>
        {/if}
      </label>
    {/each}
  {/if}

  <!-- Outside the mode branch on purpose. TLS is a property of the
       connection, not of how its credential was typed, and the edit
       form opens in URL mode — so keeping this on the parts side
       only would hide it from exactly the people who need it. -->
  <label class="field">
    <span class="label">{i18n.t('conn-dsn-ssl')}</span>
    <select disabled={sslLocked} value={sslValue} onchange={onSslChange}>
      {#each SSL_MODES as m (m)}
        <option value={m}>{i18n.t(SSL_MODE_LABEL[m])}</option>
      {/each}
    </select>
    <span class="hint">{i18n.t(sslHint)}</span>
  </label>

  <button type="button" class="linkish" onclick={() => setUrlMode(!form.use_url)}>
    {form.use_url ? i18n.t('conn-dsn-use-fields') : i18n.t('conn-dsn-use-url')}
  </button>
</fieldset>
