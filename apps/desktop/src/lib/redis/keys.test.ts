import { describe, expect, it } from 'vitest';

import type { QueryOutput } from '$lib/api';
import {
  PAGE,
  appendKeys,
  keyPage,
  metaCommands,
  scanCommand,
  ttlText,
  valueCommand,
  valueView,
  type KeyType,
} from './keys';

const out = (columns: string[], rows: QueryOutput['rows']): QueryOutput => ({
  columns: columns.map((name) => ({ name, declared_type: null })),
  rows,
  row_count: rows.length,
  truncated: false,
  has_more: false,
  next_cursor: null,
});

describe('scanCommand', () => {
  it('reads one page of a prefix from a cursor', () => {
    expect(scanCommand('user:*', '0')).toBe(`SCAN 0 MATCH user:* COUNT ${PAGE}`);
    expect(scanCommand('user:*', '42')).toBe(`SCAN 42 MATCH user:* COUNT ${PAGE}`);
  });

  it('quotes a pattern the command line would split', () => {
    expect(scanCommand('my key:*', '0')).toBe(`SCAN 0 MATCH "my key:*" COUNT ${PAGE}`);
  });
});

describe('keyPage', () => {
  it('reads the keys and the cursor to go on from', () => {
    const page = keyPage(out(['key', 'next_cursor'], [['a', '7'], ['b', '7']]));
    expect(page).toEqual({ keys: ['a', 'b'], next: '7' });
  });

  it('says the walk is over at cursor 0', () => {
    expect(keyPage(out(['key', 'next_cursor'], [['a', '0']])).next).toBeNull();
  });

  it('keeps going past an empty page', () => {
    // An empty page mid-walk comes back as one row with no key.
    expect(keyPage(out(['key', 'next_cursor'], [[null, '42']]))).toEqual({
      keys: [],
      next: '42',
    });
  });

  it('has nowhere to go when there are no rows at all', () => {
    expect(keyPage(out(['key', 'next_cursor'], []))).toEqual({ keys: [], next: null });
  });

  it('stops a cluster walk that could not finish', () => {
    // A cluster has no single cursor to continue from (ADR-0166).
    expect(keyPage(out(['key', 'next_cursor'], [['a', 'partial']])).next).toBeNull();
  });
});

describe('valueCommand', () => {
  it.each<[KeyType, string]>([
    ['string', 'GET "user:1"'],
    ['hash', `HSCAN "user:1" 0 COUNT ${PAGE}`],
    ['list', `LRANGE "user:1" 0 ${PAGE - 1}`],
    ['set', `SSCAN "user:1" 0 COUNT ${PAGE}`],
    ['zset', `ZRANGE "user:1" 0 ${PAGE - 1} WITHSCORES`],
    ['stream', `XREVRANGE "user:1" + - COUNT ${PAGE}`],
  ])('reads a %s a bounded page at a time', (type, command) => {
    expect(valueCommand('user:1', type)).toBe(command);
  });

  it('has nothing to read for a type it does not know', () => {
    expect(valueCommand('k', 'ReJSON-RL')).toBeNull();
    expect(valueCommand('k', 'none')).toBeNull();
  });

  it('quotes a key with a space or a quote in it', () => {
    expect(valueCommand('say "hi" there', 'string')).toBe('GET "say \\"hi\\" there"');
  });
});

describe('valueView', () => {
  it('shows a string as text', () => {
    expect(valueView('string', out(['value'], [['dark-mode']]))).toEqual({
      kind: 'text',
      text: 'dark-mode',
    });
  });

  it('lays a hash out as field and value, without the scan cursor', () => {
    const view = valueView(
      'hash',
      out(['field', 'value', 'next_cursor'], [['name', 'Aiko', '0'], ['plan', 'pro', '0']]),
    );
    expect(view).toEqual({
      kind: 'table',
      columns: ['field', 'value'],
      rows: [
        ['name', 'Aiko'],
        ['plan', 'pro'],
      ],
    });
  });

  it('numbers the items of a list from zero', () => {
    const view = valueView('list', out(['value'], [['a'], ['b']]));
    expect(view).toEqual({
      kind: 'table',
      columns: ['index', 'value'],
      rows: [
        ['0', 'a'],
        ['1', 'b'],
      ],
    });
  });

  it('shows a sorted set as member and score', () => {
    const view = valueView('zset', out(['member', 'score'], [['ben', '30']]));
    expect(view).toEqual({ kind: 'table', columns: ['member', 'score'], rows: [['ben', '30']] });
  });

  it('shows stream entries with their fields as one cell', () => {
    const view = valueView('stream', out(['id', 'fields'], [['1-0', { $json: { event: 'paid' } }]]));
    expect(view).toEqual({
      kind: 'table',
      columns: ['id', 'fields'],
      rows: [['1-0', '{"event":"paid"}']],
    });
  });

  it('drops the empty row an empty scan page carries', () => {
    const view = valueView('set', out(['member', 'next_cursor'], [[null, '9']]));
    expect(view).toEqual({ kind: 'table', columns: ['member'], rows: [] });
  });
});

describe('ttlText', () => {
  it('says when a key never expires', () => {
    expect(ttlText(-1)).toEqual({ key: 'redis-ttl-none' });
  });

  it('says when the key is gone', () => {
    expect(ttlText(-2)).toEqual({ key: 'redis-ttl-gone' });
  });

  it('counts down in the largest unit that fits', () => {
    expect(ttlText(42)).toEqual({ key: 'redis-ttl-in', value: '42s' });
    expect(ttlText(3600)).toEqual({ key: 'redis-ttl-in', value: '1h 0m' });
    expect(ttlText(90061)).toEqual({ key: 'redis-ttl-in', value: '1d 1h' });
  });
});

describe('metaCommands', () => {
  it('asks for the type and the time to live of exactly that key', () => {
    expect(metaCommands('user:1')).toEqual({ type: 'TYPE "user:1"', ttl: 'TTL "user:1"' });
  });
});

describe('appendKeys', () => {
  // SCAN may return a key more than once over a walk (a rehash in between,
  // or a cluster walk), and the list is keyed by key.
  it('adds the new page once, keeping the order keys were first seen in', () => {
    expect(appendKeys(['a', 'b'], ['b', 'c', 'c'])).toEqual(['a', 'b', 'c']);
  });
});
