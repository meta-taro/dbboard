// Structured host/port/user/password/database inputs for the URL-bearing
// engines, composed into the `mysql://…` / `postgres://…` DSN the backend
// adapters parse.
//
// Why this exists: the form used to ask for the DSN as one string. Every other
// desktop client (HeidiSQL, DBeaver, TablePlus) asks for the five parts, so a
// maintainer arriving with a working HeidiSQL session had to hand-assemble a
// URL — and hand-assembly is exactly where a password containing `@` or `/`
// silently repoints the connection at the wrong host. Composing here means the
// percent-encoding is done once, in a tested function.

import type { ConnectionKind } from './draft';

export type DsnField = 'db_host' | 'db_port' | 'db_user' | 'db_password' | 'db_name';

// Display order. Deliberately host → port → user → password → database, the
// order HeidiSQL and DBeaver both use, so muscle memory transfers.
export const DSN_FIELDS: readonly DsnField[] = [
  'db_host',
  'db_port',
  'db_user',
  'db_password',
  'db_name',
] as const;

/** Transport security for the database connection.
 *
 *  Only two choices, deliberately. sqlx also has `preferred`/`prefer`, which
 *  tries TLS and silently continues in plaintext when the server says no —
 *  the adapters refuse to ship that (`harden_ssl_mode` rewrites it), because a
 *  connection the user believes is encrypted and is not is worse than one they
 *  knowingly turned off. `verify_ca`/`verify_full` need a CA file the form has
 *  nowhere to put yet; a connection URL typed by hand can still ask for them. */
export type SslMode = 'require' | 'disable';

export const SSL_MODES: readonly SslMode[] = ['require', 'disable'] as const;

export interface DsnParts {
  db_host: string;
  db_port: string; // string in the form; blank means "the kind default"
  db_user: string;
  db_password: string;
  db_name: string;
  db_ssl: SslMode;
}

export function emptyDsnParts(): DsnParts {
  return {
    db_host: '',
    db_port: '',
    db_user: '',
    db_password: '',
    db_name: '',
    db_ssl: 'require',
  };
}

/** Kinds whose credential is a DSN, and so can be entered as parts. Turso
 *  (a file path or libSQL URL) and D1 (account/database ids) are not. */
export function usesDsnFields(kind: ConnectionKind): boolean {
  return kind !== 'turso' && kind !== 'd1';
}

export function defaultPort(kind: ConnectionKind): number {
  return kind === 'mysql' ? 3306 : 5432;
}

/** The URL scheme each adapter's parser accepts. Neon, Supabase and Aurora
 *  DSQL are all Postgres on the wire. */
export function schemeFor(kind: ConnectionKind): string {
  return kind === 'mysql' ? 'mysql' : 'postgres';
}

const trimmed = (v: string): string => v.trim();
const blank = (v: string): boolean => trimmed(v).length === 0;

// A bare IPv6 literal has to be bracketed or its colons read as a port
// separator. An address the user already bracketed is left alone.
function hostAuthority(raw: string): string {
  const host = trimmed(raw);
  if (host.startsWith('[')) return host;
  return host.includes(':') ? `[${host}]` : host;
}

/** The `?…` sqlx needs to be told to skip TLS, or `''` to say nothing.
 *
 *  Nothing is emitted for `require`: the adapters already harden sqlx's
 *  fall-back-to-plaintext default up to Required, so the safe case composes
 *  byte-for-byte the URL it composed before this option existed. MySQL and
 *  Postgres disagree on both the parameter name and the value spelling, and
 *  sqlx rejects a wrong one outright, so neither is guessed at the call site. */
function sslQuery(kind: ConnectionKind, mode: SslMode): string {
  if (mode !== 'disable') return '';
  return schemeFor(kind) === 'mysql' ? '?ssl-mode=disabled' : '?sslmode=disable';
}

export function composeDsn(kind: ConnectionKind, parts: DsnParts): string {
  const port = blank(parts.db_port)
    ? defaultPort(kind)
    : Number.parseInt(trimmed(parts.db_port), 10);
  const user = encodeURIComponent(trimmed(parts.db_user));
  // A blank password is not the same as an empty one: omit the whole section
  // rather than emit a trailing `:`.
  const auth = blank(parts.db_password)
    ? user
    : `${user}:${encodeURIComponent(parts.db_password)}`;
  const database = encodeURIComponent(trimmed(parts.db_name));
  const query = sslQuery(kind, parts.db_ssl);
  return `${schemeFor(kind)}://${auth}@${hostAuthority(parts.db_host)}:${port}/${database}${query}`;
}

/** Returns the invalid part fields (empty ⇒ valid). The password is optional:
 *  a MySQL account may legitimately have none. */
export function validateDsn(parts: DsnParts): DsnField[] {
  const bad: DsnField[] = [];
  if (blank(parts.db_host)) bad.push('db_host');
  if (!blank(parts.db_port)) {
    const n = Number.parseInt(trimmed(parts.db_port), 10);
    if (!Number.isFinite(n) || n < 1 || n > 65535) bad.push('db_port');
  }
  if (blank(parts.db_user)) bad.push('db_user');
  if (blank(parts.db_name)) bad.push('db_name');
  return bad;
}
