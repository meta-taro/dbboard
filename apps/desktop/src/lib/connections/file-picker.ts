// Which connection-form fields hold a filesystem path, and how the native open
// dialog should be configured for each.
//
// Typing a path by hand is the wrong ask: it is long, easy to get subtly wrong
// (a stray quote, the wrong slash), and the user is looking at the file in
// Explorer while doing it. The dialog itself lives in the component; what the
// dialog should *say and show* lives here so it is testable.

import type { MessageKey } from '$lib/i18n/messages';

export type PathField = 'path' | 'ssh_key_path' | 'ssh_known_hosts';

export const PATH_FIELDS: readonly PathField[] = [
  'path',
  'ssh_key_path',
  'ssh_known_hosts',
] as const;

export function isPathField(field: string): field is PathField {
  return (PATH_FIELDS as readonly string[]).includes(field);
}

export interface PickerFilter {
  name: string;
  extensions: string[];
}

/** Extension filters for `field`'s open dialog. Empty means no filtering at
 *  all: an OpenSSH private key and a `known_hosts` file both have no
 *  extension, so any filter would hide the file the user opened the dialog to
 *  pick. Where filters do apply they always end with `*`, for the same
 *  reason — a database file named something unexpected must stay reachable. */
export function pickerFilters(field: PathField): PickerFilter[] {
  if (field !== 'path') return [];
  return [
    { name: 'SQLite', extensions: ['db', 'sqlite', 'sqlite3'] },
    { name: 'All files', extensions: ['*'] },
  ];
}

/** The dialog's title. Reusing the field's own label names what is being
 *  picked, rather than a generic "Open". */
export function pickerTitle(field: PathField): MessageKey {
  switch (field) {
    case 'path':
      return 'conn-field-path';
    case 'ssh_key_path':
      return 'conn-ssh-key-path';
    case 'ssh_known_hosts':
      return 'conn-ssh-known-hosts';
  }
}
