// The pure decisions behind the connection list's two issue-#213 actions:
// duplicating a connection, and repairing one that points at another
// connection's saved-secret slot. Tauri-free so the rules are unit-testable;
// the component only renders what these return.
//
// Both actions exist because a slot name is minted once, from the id, when the
// connection is added — there has never been a way to change one. That left no
// supported route to a second connection sharing one credential, and no route
// out of the state a hand-edited `connections.toml` produces.
import type { ForeignRef } from '$lib/api';

// The known secret fields a slot can be minted for, and the message key that
// names each one. Kept as a table rather than folded into the key so an
// unknown field falls back instead of asking for a message that does not
// exist — a slot from a future version must not render a raw key.
const SECRET_LABELS: Record<string, string> = {
  url: 'conn-repair-secret-url',
  token: 'conn-repair-secret-token',
  service_account: 'conn-repair-secret-service_account',
  secret_key: 'conn-repair-secret-secret_key',
  ssh_password: 'conn-repair-secret-ssh_password',
  ssh_passphrase: 'conn-repair-secret-ssh_passphrase',
};

export interface CopyForm {
  id: string;
  name: string;
}

export type CopyField = 'id' | 'name';

// The foreign slot this connection carries, if any. The list calls this once
// per row, so absence — the normal case — is the cheap path.
export function foreignRefFor(refs: ForeignRef[], id: string): ForeignRef | undefined {
  return refs.find((r) => r.id === id);
}

// A free id to pre-fill the duplicate dialog with. Counts past anything taken:
// proposing an id the backend would immediately refuse reads as the app
// suggesting something it does not accept.
export function suggestCopyId(id: string, existing: string[]): string {
  const taken = new Set(existing);
  const base = `${id}-copy`;
  if (!taken.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base}-${n}`;
    if (!taken.has(candidate)) return candidate;
  }
}

// Which fields of the duplicate dialog are not usable. The id collision is
// checked here so the dialog can flag the field, instead of letting the
// backend's `DuplicateId` arrive as a banner detached from the input.
export function validateCopy(form: CopyForm, existing: string[]): CopyField[] {
  const bad: CopyField[] = [];
  const id = form.id.trim();
  // Trimmed on both sides because the backend stores the trimmed id, so
  // ' prod ' and 'prod' are the same collision.
  if (id === '' || existing.includes(id)) bad.push('id');
  if (form.name.trim() === '') bad.push('name');
  return bad;
}

// The message key labelling the one value the repair dialog asks for. Read out
// of the slot name, because "the secret" leaves an operator guessing between a
// connection URL, an API token and an SSH password.
export function secretLabelKey(keyRef: string): string {
  const rest = keyRef.startsWith('dbboard.') ? keyRef.slice('dbboard.'.length) : '';
  // Split from the right: an id may contain dots, a field name never does —
  // the same rule the backend's `split_ref` follows.
  const dot = rest.lastIndexOf('.');
  const field = dot > 0 ? rest.slice(dot + 1) : '';
  return SECRET_LABELS[field] ?? 'conn-repair-secret';
}
