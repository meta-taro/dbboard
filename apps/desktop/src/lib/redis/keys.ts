// The key browser of a Redis-compatible connection (ADR-0166): which command
// reads each kind of key, and how its reply is laid out on screen.
//
// Pure, so the rules are tested without a component. Every command built here
// is a read the adapter's allowlist accepts, bounded to one page: the browser
// never asks for a whole collection, so it never meets the size check that
// refuses one.
import { displayCell, type Cell, type QueryOutput } from '$lib/api';
import type { MessageKey } from '$lib/i18n/messages';

/** Keys per page of the list, and items per page of a value. */
export const PAGE = 100;

/** What `TYPE` answers. Modules add their own (`ReJSON-RL`, …), which the
 *  browser shows by name but does not read. */
export type KeyType = 'string' | 'hash' | 'list' | 'set' | 'zset' | 'stream' | (string & {});

export interface KeyPage {
  keys: string[];
  /** The cursor to read the next page from, or null when the walk is over. */
  next: string | null;
}

export type ValueView =
  | { kind: 'text'; text: string }
  | { kind: 'table'; columns: string[]; rows: string[][] };

/** One page of keys under `pattern`, from `cursor`. */
export function scanCommand(pattern: string, cursor: string): string {
  return `SCAN ${cursor} MATCH ${word(pattern)} COUNT ${PAGE}`;
}

/** The keys in a `SCAN` result and where to go on from. */
export function keyPage(result: QueryOutput): KeyPage {
  const keyAt = column(result, 'key');
  const cursorAt = column(result, 'next_cursor');
  const keys = result.rows
    .map((row) => row[keyAt])
    .filter((cell): cell is string => typeof cell === 'string');
  const cursor = result.rows[0]?.[cursorAt];
  // `0` ends a walk. `partial` is a cluster walk that stopped early: there is
  // no one cursor to continue it from, so there is no next page either.
  const next = typeof cursor === 'string' && /^[1-9]\d*$/.test(cursor) ? cursor : null;
  return { keys, next };
}

/** `existing` followed by the keys of a new page it does not already hold.
 *  `SCAN` may return a key more than once over one walk, and the list is
 *  keyed by key. */
export function appendKeys(existing: string[], page: string[]): string[] {
  return [...new Set([...existing, ...page])];
}

/** The two reads that head a key's view: its type, and its time to live. */
export function metaCommands(key: string): { type: string; ttl: string } {
  return { type: `TYPE ${quote(key)}`, ttl: `TTL ${quote(key)}` };
}

/** The first page of a key's value, or null for a type the browser does not
 *  read (a module's type, or `none` for a key that has just expired). */
export function valueCommand(key: string, type: KeyType): string | null {
  const k = quote(key);
  switch (type) {
    case 'string':
      return `GET ${k}`;
    case 'hash':
      return `HSCAN ${k} 0 COUNT ${PAGE}`;
    case 'list':
      return `LRANGE ${k} 0 ${PAGE - 1}`;
    case 'set':
      return `SSCAN ${k} 0 COUNT ${PAGE}`;
    case 'zset':
      return `ZRANGE ${k} 0 ${PAGE - 1} WITHSCORES`;
    // Newest first: a stream is usually read to see what just happened.
    case 'stream':
      return `XREVRANGE ${k} + - COUNT ${PAGE}`;
    default:
      return null;
  }
}

/** A value's reply, laid out for its type. */
export function valueView(type: KeyType, result: QueryOutput): ValueView {
  if (type === 'string') {
    return { kind: 'text', text: displayCell(result.rows[0]?.[0] ?? null) };
  }
  // The scan cursor is how to fetch more, not part of the value.
  const keep = result.columns
    .map((c, i) => ({ name: c.name, i }))
    .filter((c) => c.name !== 'next_cursor');
  const rows = result.rows
    // An empty scan page carries one row with nothing in it but the cursor.
    .filter((row) => keep.some((c) => row[c.i] !== null))
    .map((row) => keep.map((c) => text(row[c.i])));
  if (type === 'list') {
    return {
      kind: 'table',
      columns: ['index', 'value'],
      rows: rows.map((row, i) => [String(i), ...row]),
    };
  }
  return { kind: 'table', columns: keep.map((c) => c.name), rows };
}

/** How long a key has left, as a message to show. */
export function ttlText(seconds: number): { key: MessageKey; value?: string } {
  if (seconds === -1) return { key: 'redis-ttl-none' };
  if (seconds < 0) return { key: 'redis-ttl-gone' };
  return { key: 'redis-ttl-in', value: duration(seconds) };
}

function duration(s: number): string {
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

function column(result: QueryOutput, name: string): number {
  return result.columns.findIndex((c) => c.name === name);
}

function text(cell: Cell | undefined): string {
  return displayCell(cell ?? null);
}

/** A key, always quoted: keys are data, and a key that happens to look like
 *  an option must still reach the server as a key. */
function quote(value: string): string {
  return `"${value.replace(/[\\"]/g, (c) => `\\${c}`)}"`;
}

/** A pattern, quoted only when the command line would split or misread it. */
function word(value: string): string {
  return value !== '' && /^[^\s"'\\]+$/.test(value) ? value : quote(value);
}
