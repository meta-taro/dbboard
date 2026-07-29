// Pure form model for the connection editor. Keeping the validation and the
// DTO-shaping here (I/O-free) means the Svelte component only binds inputs and
// the Tauri command only receives an already-validated payload — mirroring the
// egui `ConnectionsView` split between form state and `ConnectionAdmin` drafts.
//
// The `kind` discriminator values are snake_case to match the backend's
// `#[serde(tag = "kind", rename_all = "snake_case")]` DTOs (src-tauri/lib.rs).

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

export interface ConnectionForm {
  id: string;
  name: string;
  kind: ConnectionKind;
  path: string; // turso
  account_id: string; // d1
  database_id: string; // d1
  base_url: string; // d1 (optional)
  token: string; // d1 secret
  url: string; // postgres / neon / supabase / aurora_dsql secret
}

export type EditorMode = 'add' | 'edit';

// Non-secret editable fields the backend returns for the edit form. Secrets
// (D1 token, the Postgres-family URL) are never included — the form leaves
// them blank, which the backend reads as "keep the stored secret".
export type EditFields =
  | { kind: 'turso'; path: string }
  | { kind: 'd1'; account_id: string; database_id: string; base_url: string | null }
  | { kind: 'postgres' | 'mysql' | 'neon' | 'supabase' | 'aurora_dsql' };

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
  };
}

const blank = (v: string): boolean => v.trim().length === 0;

// Seed the edit form from an existing connection's non-secret fields. Secret
// inputs stay blank so an untouched save keeps the stored secret.
export function formForEdit(id: string, name: string, fields: EditFields): ConnectionForm {
  const base: ConnectionForm = { ...emptyForm(), id, name, kind: fields.kind };
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
export function requiredFields(kind: ConnectionKind, mode: EditorMode): FormField[] {
  const common: FormField[] = mode === 'add' ? ['id', 'name'] : ['name'];
  const secrets = new Set(secretFields(kind));
  const kindFields = fieldsForKind(kind).filter(
    (f) => f !== 'base_url' && !(mode === 'edit' && secrets.has(f)),
  );
  return [...common, ...kindFields];
}

// Returns the fields that fail validation (blank required). Empty ⇒ valid.
export function validate(form: ConnectionForm, mode: EditorMode): FormField[] {
  return requiredFields(form.kind, mode).filter((f) => blank(form[f]));
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
      return { kind: form.kind, url: form.url };
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
      return { kind: form.kind, url: form.url };
  }
}
