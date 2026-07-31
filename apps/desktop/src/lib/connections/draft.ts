// Pure form model for the connection editor. Keeping the validation and the
// DTO-shaping here (I/O-free) means the Svelte component only binds inputs and
// the Tauri command only receives an already-validated payload — mirroring the
// egui `ConnectionsView` split between form state and `ConnectionAdmin` drafts.
//
// The `kind` discriminator values are snake_case to match the backend's
// `#[serde(tag = "kind", rename_all = "snake_case")]` DTOs (src-tauri/lib.rs).

import {
  composeDsn,
  emptyDsnParts,
  usesDsnFields,
  validateDsn,
  type DsnField,
  type DsnParts,
} from './dsn';

export type ConnectionKind =
  | 'turso'
  | 'd1'
  | 'postgres'
  | 'mysql'
  | 'neon'
  | 'supabase'
  | 'aurora_dsql';

// Order shown in the kind picker. `turso` first: it's the zero-credential
// local/libSQL case and the friendliest default.
export const CONNECTION_KINDS: readonly ConnectionKind[] = [
  'turso',
  'd1',
  'postgres',
  'mysql',
  'neon',
  'supabase',
  'aurora_dsql',
] as const;

// Every field the union of all kinds can carry. A given kind uses only a
// subset; unused fields stay blank and are ignored by the payload builders.
export type FormField =
  | 'id'
  | 'name'
  | 'path'
  | 'account_id'
  | 'database_id'
  | 'base_url'
  | 'token'
  | 'url';

// Kinds that can front an SSH tunnel (ADR-0069): the TCP/URL-bearing engines.
// Turso (embedded/libSQL HTTP) and D1 (HTTP) have no host:port to forward, so
// the tunnel section is hidden for them — matching the backend's
// `ConnectionKind::supports_ssh_tunnel`.
export const SSH_TUNNELABLE_KINDS: readonly ConnectionKind[] = [
  'postgres',
  'mysql',
  'neon',
  'supabase',
  'aurora_dsql',
] as const;

export function supportsSshTunnel(kind: ConnectionKind): boolean {
  return SSH_TUNNELABLE_KINDS.includes(kind);
}

// Backend display slugs (hyphenated — `kind_label` in dbboard-mcp) whose
// entries are owned by `connections.toml` and have no in-app form. The list
// panel takes these from `ConnectionView.kind`, a different namespace from the
// underscored `ConnectionKind` above, which is why this is a plain string.
const TOML_ONLY_KIND_SLUGS: readonly string[] = ['aurora-dsql-iam'];

/** Whether the edit form can open this connection at all. The backend refuses
 *  the same set; checking here turns "click, then get a red error" into a
 *  disabled button that says why up front. Unknown slugs are treated as
 *  editable so a newly added backend kind is never silently locked out. */
export function isEditableInApp(kindSlug: string): boolean {
  return !TOML_ONLY_KIND_SLUGS.includes(kindSlug);
}

export type SshAuthMethod = 'key' | 'password';
export type SshHostKeyPolicy = 'fingerprint' | 'known_hosts';

// SSH form fields whose blank-ness `validateSsh` reports. Distinct from
// `FormField` because the tunnel section is conditional and its required set
// depends on the chosen auth method and host-key policy.
export type SshFormField =
  | 'ssh_host'
  | 'ssh_port'
  | 'ssh_user'
  | 'ssh_key_path'
  | 'ssh_passphrase'
  | 'ssh_password'
  | 'ssh_fingerprint'
  | 'ssh_known_hosts';

export interface ConnectionForm extends DsnParts {
  id: string;
  name: string;
  kind: ConnectionKind;
  path: string; // turso
  account_id: string; // d1
  database_id: string; // d1
  base_url: string; // d1 (optional)
  token: string; // d1 secret
  url: string; // postgres / neon / supabase / aurora_dsql secret
  // How the DSN is being supplied. `false` (the default on add) means the
  // structured `db_*` parts, which is what every other client asks for; `true`
  // means the raw URL, kept as the escape hatch for what a form can't express
  // (`?sslmode=`, unix sockets, multi-host). An edit starts in URL mode because
  // the stored secret is never returned, so a blank URL means "keep it".
  use_url: boolean;
  // SSH tunnel (ADR-0069). Only meaningful for `SSH_TUNNELABLE_KINDS`; ignored
  // by the payload builders otherwise. `port` is a string in the form and
  // parsed to a number on submit; the passphrase/password are secrets (blank on
  // edit means "keep the stored one").
  ssh_enabled: boolean;
  ssh_host: string;
  ssh_port: string;
  ssh_user: string;
  ssh_auth_method: SshAuthMethod;
  ssh_key_path: string;
  ssh_key_encrypted: boolean; // edit prefill: does the stored key have a passphrase?
  ssh_passphrase: string; // secret
  ssh_password: string; // secret
  ssh_host_key_policy: SshHostKeyPolicy;
  ssh_fingerprint: string;
  ssh_known_hosts: string;
  // Edit-prefill provenance: whether the stored tunnel actually had a
  // key-passphrase / password to *keep*. A blank secret only means "keep" when
  // one of these is true; otherwise the field is required (mirrors the backend's
  // `apply_update_ssh`, which rejects a keep with nothing to keep). Always false
  // on add. Not sent to the backend.
  ssh_had_key_passphrase: boolean;
  ssh_had_password: boolean;
}

// Non-secret SSH prefill the backend returns alongside the edit fields. Secrets
// (passphrase/password) are never sent back (ADR-0016); `encrypted` tells the
// form whether a stored passphrase exists.
export type SshAuthPrefill =
  | { method: 'key'; key_path: string; encrypted: boolean }
  | { method: 'password' };
export type SshHostKeyPrefill =
  | { policy: 'fingerprint'; fingerprint: string }
  | { policy: 'known_hosts'; known_hosts: string };
export interface SshPrefill {
  host: string;
  port: number;
  user: string;
  auth: SshAuthPrefill;
  host_key: SshHostKeyPrefill;
}

export const DEFAULT_SSH_PORT = 22;

export type EditorMode = 'add' | 'edit';

// Non-secret editable fields the backend returns for the edit form. Secrets
// (D1 token, the Postgres-family URL) are never included — the form leaves
// them blank, which the backend reads as "keep the stored secret". The `ssh`
// block is the tunnel prefill (`null` when no tunnel is configured); the
// backend flattens the `kind` discriminator to the top level, so it sits beside
// these kind fields (matching `EditFieldsResponse` in src-tauri/lib.rs).
export type EditFields = (
  | { kind: 'turso'; path: string }
  | { kind: 'd1'; account_id: string; database_id: string; base_url: string | null }
  | { kind: 'postgres' | 'mysql' | 'neon' | 'supabase' | 'aurora_dsql' }
) & { ssh?: SshPrefill | null };

export function emptyForm(): ConnectionForm {
  return {
    id: '',
    name: '',
    kind: 'turso',
    path: '',
    account_id: '',
    database_id: '',
    base_url: '',
    token: '',
    url: '',
    use_url: false,
    ...emptyDsnParts(),
    ssh_enabled: false,
    ssh_host: '',
    ssh_port: String(DEFAULT_SSH_PORT),
    ssh_user: '',
    ssh_auth_method: 'key',
    ssh_key_path: '',
    ssh_key_encrypted: false,
    ssh_passphrase: '',
    ssh_password: '',
    ssh_host_key_policy: 'fingerprint',
    ssh_fingerprint: '',
    ssh_known_hosts: '',
    ssh_had_key_passphrase: false,
    ssh_had_password: false,
  };
}

const blank = (v: string): boolean => v.trim().length === 0;

// Apply the tunnel prefill (if any) onto a base form. Secrets stay blank so an
// untouched save keeps the stored passphrase/password.
function applySshPrefill(base: ConnectionForm, ssh: SshPrefill | null | undefined): ConnectionForm {
  if (!ssh) return base;
  const next: ConnectionForm = {
    ...base,
    ssh_enabled: true,
    ssh_host: ssh.host,
    ssh_port: String(ssh.port),
    ssh_user: ssh.user,
  };
  if (ssh.auth.method === 'key') {
    next.ssh_auth_method = 'key';
    next.ssh_key_path = ssh.auth.key_path;
    next.ssh_key_encrypted = ssh.auth.encrypted;
    // A stored passphrase exists to keep only for an encrypted key.
    next.ssh_had_key_passphrase = ssh.auth.encrypted;
  } else {
    next.ssh_auth_method = 'password';
    next.ssh_had_password = true;
  }
  if (ssh.host_key.policy === 'fingerprint') {
    next.ssh_host_key_policy = 'fingerprint';
    next.ssh_fingerprint = ssh.host_key.fingerprint;
  } else {
    next.ssh_host_key_policy = 'known_hosts';
    next.ssh_known_hosts = ssh.host_key.known_hosts;
  }
  return next;
}

// Seed the edit form from an existing connection's non-secret fields. Secret
// inputs stay blank so an untouched save keeps the stored secret.
export function formForEdit(id: string, name: string, fields: EditFields): ConnectionForm {
  const base: ConnectionForm = applySshPrefill(
    // URL mode on edit: the stored DSN is a secret the backend never sends
    // back, so a blank URL has to keep it. The structured parts cannot express
    // "keep" — a half-filled set would compose a wrong DSN — so switching to
    // them is an explicit, full replacement.
    { ...emptyForm(), id, name, kind: fields.kind, use_url: usesDsnFields(fields.kind) },
    fields.ssh,
  );
  switch (fields.kind) {
    case 'turso':
      return { ...base, path: fields.path };
    case 'd1':
      return {
        ...base,
        account_id: fields.account_id,
        database_id: fields.database_id,
        base_url: fields.base_url ?? '',
      };
    default:
      // Postgres-family: only name + the secret url are editable.
      return base;
  }
}

// The kind-specific fields shown in the form, in display order. `id`/`name`
// are common and handled separately.
export function fieldsForKind(kind: ConnectionKind): FormField[] {
  switch (kind) {
    case 'turso':
      return ['path'];
    case 'd1':
      return ['account_id', 'database_id', 'base_url', 'token'];
    case 'postgres':
    case 'mysql':
    case 'neon':
    case 'supabase':
    case 'aurora_dsql':
      return ['url'];
  }
}

// Which of a kind's fields hold secret material (masked input; on edit a blank
// value means "keep the stored secret", never "clear it").
export function secretFields(kind: ConnectionKind): FormField[] {
  switch (kind) {
    case 'd1':
      return ['token'];
    case 'postgres':
    case 'mysql':
    case 'neon':
    case 'supabase':
    case 'aurora_dsql':
      return ['url'];
    case 'turso':
      return [];
  }
}

// Fields that must be non-blank to submit. `base_url` is always optional. On
// edit, secret fields drop out of the required set: a blank secret keeps the
// stored one (the existing value is never sent back to the form).
export function requiredFields(
  kind: ConnectionKind,
  mode: EditorMode,
  useUrl = true,
): FormField[] {
  const common: FormField[] = mode === 'add' ? ['id', 'name'] : ['name'];
  const secrets = new Set(secretFields(kind));
  const kindFields = fieldsForKind(kind).filter(
    (f) =>
      f !== 'base_url' &&
      !(mode === 'edit' && secrets.has(f)) &&
      // In fields mode the URL is composed, never typed, so it is not an input.
      !(f === 'url' && !useUrl),
  );
  return [...common, ...kindFields];
}

// Returns the fields that fail validation (blank required). Empty ⇒ valid.
export function validate(form: ConnectionForm, mode: EditorMode): FormField[] {
  return requiredFields(form.kind, mode, form.use_url).filter((f) => blank(form[f]));
}

// The DSN parts that fail validation, or empty when the form isn't using them.
// Reported separately from `validate` because the two field sets are disjoint
// and the component highlights them in different sections.
export function validateDsnFields(form: ConnectionForm): DsnField[] {
  if (!usesDsnFields(form.kind) || form.use_url) return [];
  return validateDsn(form);
}

// Parse the form's port string, defaulting a blank/invalid value to 22. The
// caller has already validated the range via `validateSsh`; this is the
// coercion used when building the payload.
export function parseSshPort(raw: string): number {
  const n = Number.parseInt(raw.trim(), 10);
  return Number.isFinite(n) && n >= 1 && n <= 65535 ? n : DEFAULT_SSH_PORT;
}

// Validate the SSH section. Returns the invalid tunnel fields (empty ⇒ valid).
// No-op unless the kind supports a tunnel and the toggle is on. Host-key
// verification is mandatory, so the chosen policy's field is always required.
// Secrets are required only on `add` (on `edit`, a blank keeps the stored one;
// an unencrypted key never needs a passphrase).
export function validateSsh(form: ConnectionForm, mode: EditorMode): SshFormField[] {
  if (!supportsSshTunnel(form.kind) || !form.ssh_enabled) return [];
  const bad: SshFormField[] = [];
  if (blank(form.ssh_host)) bad.push('ssh_host');
  if (blank(form.ssh_user)) bad.push('ssh_user');
  // A non-blank port must be a valid TCP port; blank is allowed (defaults 22).
  if (!blank(form.ssh_port)) {
    const n = Number.parseInt(form.ssh_port.trim(), 10);
    if (!Number.isFinite(n) || n < 1 || n > 65535) bad.push('ssh_port');
  }
  if (form.ssh_auth_method === 'key') {
    if (blank(form.ssh_key_path)) bad.push('ssh_key_path');
    // On add the encrypted checkbox is hidden and a blank passphrase means an
    // unencrypted key, so nothing is required here. On edit, a blank passphrase
    // means "keep" only when the stored key was already encrypted; if the user
    // freshly marks the key encrypted (or switches auth to an encrypted key)
    // there is nothing to keep, so the passphrase is required — matching the
    // backend, which would otherwise reject the save.
    if (
      mode === 'edit' &&
      form.ssh_key_encrypted &&
      blank(form.ssh_passphrase) &&
      !form.ssh_had_key_passphrase
    ) {
      bad.push('ssh_passphrase');
    }
  } else if (blank(form.ssh_password) && (mode === 'add' || !form.ssh_had_password)) {
    // A blank password means "keep" only when a stored password exists (edit of
    // an already password-authed tunnel); otherwise it is required.
    bad.push('ssh_password');
  }
  if (form.ssh_host_key_policy === 'fingerprint') {
    if (blank(form.ssh_fingerprint)) bad.push('ssh_fingerprint');
  } else if (blank(form.ssh_known_hosts)) {
    bad.push('ssh_known_hosts');
  }
  return bad;
}

// Shared host-key payload fragment for both add and edit.
function buildHostKey(form: ConnectionForm): Record<string, unknown> {
  return form.ssh_host_key_policy === 'fingerprint'
    ? { policy: 'fingerprint', fingerprint: form.ssh_fingerprint }
    : { policy: 'known_hosts', known_hosts: form.ssh_known_hosts };
}

// The `ssh` object the `add_connection` command expects (a tagged SshInput), or
// `null` when the kind can't tunnel or the toggle is off. Secrets are inline; a
// blank key passphrase is sent as `null`, meaning the key is unencrypted.
export function buildSshInput(form: ConnectionForm): Record<string, unknown> | null {
  if (!supportsSshTunnel(form.kind) || !form.ssh_enabled) return null;
  const auth =
    form.ssh_auth_method === 'key'
      ? {
          method: 'key',
          key_path: form.ssh_key_path,
          passphrase: blank(form.ssh_passphrase) ? null : form.ssh_passphrase,
        }
      : { method: 'password', password: form.ssh_password };
  return {
    host: form.ssh_host,
    port: parseSshPort(form.ssh_port),
    user: form.ssh_user,
    auth,
    host_key: buildHostKey(form),
  };
}

// The `ssh` object the `update_connection` command expects (a tagged
// SshEditInput). The desktop form always knows the toggle state, so it sends
// `set` (tunnel on) or `disable` (tunnel off) — never `keep`, which exists for
// callers with no tunnel UI. Secrets are keep-or-overwrite: a blank passphrase/
// password is sent as `null`, which the backend reads as "keep the stored one";
// `encrypted` distinguishes an unencrypted key from one whose passphrase is kept.
export function buildSshEditInput(form: ConnectionForm): Record<string, unknown> {
  if (!supportsSshTunnel(form.kind) || !form.ssh_enabled) return { action: 'disable' };
  const auth =
    form.ssh_auth_method === 'key'
      ? {
          method: 'key',
          key_path: form.ssh_key_path,
          encrypted: form.ssh_key_encrypted,
          passphrase: blank(form.ssh_passphrase) ? null : form.ssh_passphrase,
        }
      : {
          method: 'password',
          password: blank(form.ssh_password) ? null : form.ssh_password,
        };
  return {
    action: 'set',
    host: form.ssh_host,
    port: parseSshPort(form.ssh_port),
    user: form.ssh_user,
    auth,
    host_key: buildHostKey(form),
  };
}

// The DSN to send for a URL-bearing kind: the raw URL as typed, or one composed
// from the structured parts. Shared by add and edit — in fields mode an edit is
// a full replacement of the stored secret, never a keep.
function dsnFor(form: ConnectionForm): string {
  return form.use_url ? form.url : composeDsn(form.kind, form);
}

// The `kind` object the `add_connection` command expects (a tagged KindInput).
// Non-secret optional fields are trimmed to undefined when blank so the
// backend's `none_if_blank` sees them absent.
export function buildKindInput(form: ConnectionForm): Record<string, unknown> {
  switch (form.kind) {
    case 'turso':
      return { kind: 'turso', path: form.path };
    case 'd1':
      return {
        kind: 'd1',
        account_id: form.account_id,
        database_id: form.database_id,
        base_url: blank(form.base_url) ? null : form.base_url,
        token: form.token,
      };
    case 'postgres':
    case 'mysql':
    case 'neon':
    case 'supabase':
    case 'aurora_dsql':
      return { kind: form.kind, url: dsnFor(form) };
  }
}

// The `kind` object the `update_connection` command expects (a KindEditInput).
// Secret fields are sent verbatim: the backend treats a blank string as "keep
// the stored secret", so we never need to omit them.
export function buildKindEditInput(form: ConnectionForm): Record<string, unknown> {
  switch (form.kind) {
    case 'turso':
      return { kind: 'turso', path: form.path };
    case 'd1':
      return {
        kind: 'd1',
        account_id: form.account_id,
        database_id: form.database_id,
        base_url: blank(form.base_url) ? null : form.base_url,
        token: form.token,
      };
    case 'postgres':
    case 'mysql':
    case 'neon':
    case 'supabase':
    case 'aurora_dsql':
      return { kind: form.kind, url: dsnFor(form) };
  }
}
