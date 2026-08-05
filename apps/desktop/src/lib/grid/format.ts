// Pure helpers for the result grid: cell comparison, multi-key sort, and
// delimited (CSV/TSV) export. Kept free of Svelte and the DOM so the logic is
// unit-testable in isolation (a runner is not yet wired up, but the seam is).
import { displayCell, isDocument, type Cell, type Column } from '$lib/api';

export type SortDir = 'asc' | 'desc';
export interface SortKey {
  col: number;
  dir: SortDir;
}

/**
 * Order two cells for sorting. NULL always sinks to the bottom (independent of
 * direction — the caller negates for `desc`, and NULLs staying last is the
 * conventional, least-surprising behaviour). Numbers compare numerically,
 * booleans as 0/1, everything else (text, blobs) by its displayed string with
 * a numeric-aware locale compare so "row2" < "row10".
 */
export function compareCell(a: Cell, b: Cell): number {
  const aNull = a === null;
  const bNull = b === null;
  if (aNull && bNull) return 0;
  if (aNull) return 1;
  if (bNull) return -1;

  if (typeof a === 'number' && typeof b === 'number') return a - b;
  if (typeof a === 'boolean' && typeof b === 'boolean') {
    return (a ? 1 : 0) - (b ? 1 : 0);
  }
  return displayCell(a).localeCompare(displayCell(b), undefined, {
    numeric: true,
    sensitivity: 'base',
  });
}

/**
 * Return row indices ordered by the sort keys, most significant first, with a
 * stable tiebreak on original position. Indices (not rows) are returned so the
 * caller keeps each row's identity for selection. An empty key list yields the
 * natural order.
 */
export function sortedIndices(rows: Cell[][], keys: SortKey[]): number[] {
  const order = rows.map((_, i) => i);
  if (keys.length === 0) return order;

  return order.sort((ia, ib) => {
    for (const k of keys) {
      const c = compareCell(rows[ia][k.col], rows[ib][k.col]);
      if (c !== 0) return k.dir === 'asc' ? c : -c;
    }
    return ia - ib; // stable
  });
}

/**
 * Advance the sort state for a header click.
 * - Plain click cycles the single active column: asc → desc → none. Clicking a
 *   different column starts fresh at asc.
 * - Additive (shift) click keeps existing keys and toggles this column's
 *   direction, appending it at asc if new — this is how multi-key sort is
 *   built.
 */
export function nextSortKeys(
  current: SortKey[],
  col: number,
  additive: boolean,
): SortKey[] {
  const existing = current.find((k) => k.col === col);

  if (additive) {
    if (existing) {
      return current.map((k) =>
        k.col === col ? { col, dir: flip(k.dir) } : k,
      );
    }
    return [...current, { col, dir: 'asc' }];
  }

  // Plain click.
  if (current.length === 1 && existing) {
    return existing.dir === 'asc' ? [{ col, dir: 'desc' }] : [];
  }
  return [{ col, dir: 'asc' }];
}

function flip(dir: SortDir): SortDir {
  return dir === 'asc' ? 'desc' : 'asc';
}

/** The value a cell contributes to an export: NULL is blank (spreadsheet
 *  convention), a blob is a placeholder, a document its JSON text, everything
 *  else its display text. */
export function exportValue(cell: Cell): string {
  if (cell === null) return '';
  if (typeof cell === 'object' && '$blob' in cell) return '<blob>';
  if (isDocument(cell)) return JSON.stringify(cell.$json);
  return String(cell);
}

/**
 * Serialize columns + rows to a delimited block. CSV (`,`) fields are
 * RFC-4180-quoted when they contain the delimiter, a quote, or a newline; TSV
 * (`\t`) fields have any tab/newline collapsed to a space so a paste into a
 * spreadsheet keeps its cell boundaries. Records are separated (no trailing
 * newline) — CSV joins with CRLF per RFC 4180, TSV with a bare LF, the byte
 * layout ADR-0035 fixed for dbboard's exports.
 */
export function toDelimited(
  columns: Column[],
  rows: Cell[][],
  sep: ',' | '\t',
): string {
  const escape = sep === ',' ? escapeCsv : escapeTsv;
  const newline = sep === ',' ? '\r\n' : '\n';
  const header = columns.map((c) => escape(c.name)).join(sep);
  const body = rows.map((r) => r.map((cell) => escape(exportValue(cell))).join(sep));
  return [header, ...body].join(newline);
}

/**
 * UTF-8 byte-order mark. Excel on Windows assumes the system ANSI code page
 * (Shift-JIS on Japanese Windows) for a BOM-less CSV and renders UTF-8 text as
 * mojibake; a leading BOM makes it auto-detect UTF-8. Harmless to BOM-aware
 * parsers and to the spreadsheet's own re-save (ADR-0035).
 */
export const UTF8_BOM = '﻿';

/**
 * `toDelimited` with a leading UTF-8 BOM — the form to *write to a file* a user
 * will open in Excel. The clipboard path (copy) deliberately stays BOM-less:
 * the clipboard carries Unicode natively, and a BOM would show up as a stray
 * glyph when pasting into a plain-text target (ADR-0035).
 */
export function toDelimitedFile(
  columns: Column[],
  rows: Cell[][],
  sep: ',' | '\t',
): string {
  return UTF8_BOM + toDelimited(columns, rows, sep);
}

function escapeCsv(field: string): string {
  if (/[",\r\n]/.test(field)) {
    return `"${field.replace(/"/g, '""')}"`;
  }
  return field;
}

function escapeTsv(field: string): string {
  return field.replace(/[\t\r\n]+/g, ' ');
}
