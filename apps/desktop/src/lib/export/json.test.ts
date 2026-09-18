import { describe, expect, it } from 'vitest';

import type { Cell, Column } from '$lib/api';
import { columnKeys, toJsonFile } from './json';

const col = (name: string): Column => ({ name, declared_type: null });

describe('columnKeys', () => {
  it('uses the column names the engine reported', () => {
    expect(columnKeys([col('id'), col('name')])).toEqual(['id', 'name']);
  });

  it('disambiguates a repeated name instead of losing a column', () => {
    // `SELECT a.id, b.id FROM a JOIN b` is ordinary SQL, and an object cannot
    // hold two `id` keys: the second would overwrite the first and the export
    // would silently carry one column fewer than the grid shows. The first
    // occurrence keeps the bare name so the common case reads unchanged.
    expect(columnKeys([col('id'), col('name'), col('id')])).toEqual([
      'id',
      'name',
      'id:2',
    ]);
  });

  it('numbers each repeat by its own occurrence, not by position', () => {
    expect(columnKeys([col('x'), col('x'), col('y'), col('x')])).toEqual([
      'x',
      'x:2',
      'y',
      'x:3',
    ]);
  });

  it('names an unnamed column by its position', () => {
    // SQLite gives an expression no name. `""` as a JSON key is legal but
    // unusable from every consumer that addresses fields by identifier.
    expect(columnKeys([col('id'), col('')])).toEqual(['id', 'column:2']);
  });
});

describe('toJsonFile', () => {
  it('writes an array of objects, one per row', () => {
    const text = toJsonFile([col('id'), col('name')], [
      [1, 'alice'],
      [2, 'bob'],
    ]);
    expect(JSON.parse(text)).toEqual([
      { id: 1, name: 'alice' },
      { id: 2, name: 'bob' },
    ]);
  });

  it('keeps NULL as null, not as the empty string the CSV export writes', () => {
    // This is the reason JSON export exists next to CSV. `exportValue` flattens
    // NULL to `''` because a spreadsheet cell has nowhere else to put it; JSON
    // does, and a consumer that cannot tell NULL from an empty string cannot
    // reconstruct the row.
    const text = toJsonFile([col('note')], [[null]]);
    expect(JSON.parse(text)).toEqual([{ note: null }]);
  });

  it('keeps numbers and booleans as themselves', () => {
    const text = toJsonFile([col('n'), col('ok')], [[42, true]]);
    expect(JSON.parse(text)).toEqual([{ n: 42, ok: true }]);
  });

  it('unwraps a document into real JSON', () => {
    // `$json` is a transport tag that tells the frontend "this cell is a
    // document, not prose". JSON can express a document natively, so the tag
    // carries nothing here and would only force every consumer to strip it.
    const text = toJsonFile([col('addr')], [[{ $json: { city: 'Nara' } }]]);
    expect(JSON.parse(text)).toEqual([{ addr: { city: 'Nara' } }]);
  });

  it('keeps a blob tagged, because JSON cannot express bytes', () => {
    // The opposite case, and the reason the rule is not "strip every tag":
    // dropping `$blob` would leave base64 that reads as ordinary text, with
    // nothing to say it is not.
    const text = toJsonFile([col('avatar')], [[{ $blob: 'AQID' }]]);
    expect(JSON.parse(text)).toEqual([{ avatar: { $blob: 'AQID' } }]);
  });

  it('writes no byte-order mark', () => {
    // The CSV export deliberately leads with a BOM so Excel detects UTF-8
    // (ADR-0035). JSON goes the other way: `JSON.parse` and most parsers
    // reject a leading BOM outright, so the same habit would produce a file
    // nothing can read.
    const text = toJsonFile([col('id')], [[1]]);
    expect(text.startsWith('﻿')).toBe(false);
    expect(text.charCodeAt(0)).toBe('['.charCodeAt(0));
  });

  it('is indented and newline-terminated, because a person opens it too', () => {
    const text = toJsonFile([col('id')], [[1]]);
    expect(text).toBe('[\n  {\n    "id": 1\n  }\n]\n');
  });

  it('writes an empty array for an empty result', () => {
    expect(toJsonFile([col('id')], [])).toBe('[]\n');
  });

  it('carries a repeated column under its disambiguated key', () => {
    const rows: Cell[][] = [[1, 2]];
    const text = toJsonFile([col('id'), col('id')], rows);
    expect(JSON.parse(text)).toEqual([{ id: 1, 'id:2': 2 }]);
  });
});
