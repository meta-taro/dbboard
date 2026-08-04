import { describe, expect, it } from 'vitest';
import type { Cell, Column } from '$lib/api';
import { UTF8_BOM, toDelimited, toDelimitedFile } from './format';

const col = (name: string): Column => ({ name, declared_type: null });

describe('toDelimited', () => {
  it('writes a header row then one CRLF-separated record per row (CSV)', () => {
    const cols = [col('id'), col('name')];
    const rows: Cell[][] = [
      [1, 'Alpha'],
      [2, 'Beta'],
    ];
    expect(toDelimited(cols, rows, ',')).toBe('id,name\r\n1,Alpha\r\n2,Beta');
  });

  it('quotes a CSV field carrying the delimiter, a quote, or a newline', () => {
    const cols = [col('note')];
    const rows: Cell[][] = [['a,b'], ['say "hi"'], ['line1\nline2']];
    expect(toDelimited(cols, rows, ',')).toBe(
      'note\r\n"a,b"\r\n"say ""hi"""\r\n"line1\nline2"',
    );
  });

  it('renders NULL as an empty field, not the word NULL', () => {
    const cols = [col('a'), col('b')];
    const rows: Cell[][] = [[null, 7]];
    expect(toDelimited(cols, rows, ',')).toBe('a,b\r\n,7');
  });

  it('summarises a blob rather than emitting its bytes', () => {
    const cols = [col('data')];
    const rows: Cell[][] = [[{ $blob: 'AAAA' }]];
    expect(toDelimited(cols, rows, ',')).toBe('data\r\n<blob>');
  });

  it('collapses embedded tabs/newlines in a TSV field to a space', () => {
    const cols = [col('note')];
    const rows: Cell[][] = [['a\tb'], ['x\ny']];
    expect(toDelimited(cols, rows, '\t')).toBe('note\na b\nx y');
  });

  it('emits the header alone when there are no rows', () => {
    expect(toDelimited([col('id'), col('name')], [], ',')).toBe('id,name');
  });
});

describe('toDelimitedFile', () => {
  it('prefixes the UTF-8 BOM so Excel auto-detects UTF-8, body unchanged', () => {
    const cols = [col('id')];
    const rows: Cell[][] = [[1]];
    const withBom = toDelimitedFile(cols, rows, ',');
    expect(withBom.startsWith(UTF8_BOM)).toBe(true);
    expect(withBom.slice(UTF8_BOM.length)).toBe(toDelimited(cols, rows, ','));
  });

  it('BOM is exactly U+FEFF (EF BB BF once UTF-8 encoded)', () => {
    expect(UTF8_BOM).toBe('﻿');
    expect(new TextEncoder().encode(UTF8_BOM)).toEqual(
      new Uint8Array([0xef, 0xbb, 0xbf]),
    );
  });
});
