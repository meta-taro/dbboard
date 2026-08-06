import { describe, expect, it } from 'vitest';

import { displayCell, type Cell } from './api';

describe('displayCell', () => {
  it('spells NULL out rather than showing a blank', () => {
    expect(displayCell(null)).toBe('NULL');
  });

  it('summarises a blob instead of dumping base64 into the grid', () => {
    expect(displayCell({ $blob: 'AP8M' })).toBe('<blob>');
  });

  it('renders a document as compact JSON, never as [object Object]', () => {
    const cell: Cell = { $json: { a: 1, b: ['x', null] } };
    expect(displayCell(cell)).toBe('{"a":1,"b":["x",null]}');
  });

  it('renders a document array as JSON', () => {
    expect(displayCell({ $json: [1, 2] })).toBe('[1,2]');
  });

  it('keeps a text cell that merely looks like JSON distinguishable from a document', () => {
    // The tag is the whole point: without it these two would render alike and
    // the grid could not tell prose from a parsed document.
    expect(displayCell('{"a":1}')).toBe('{"a":1}');
    expect(displayCell({ $json: { a: 1 } })).toBe('{"a":1}');
  });

  it('passes scalars through unchanged', () => {
    expect(displayCell(42)).toBe('42');
    expect(displayCell('hi')).toBe('hi');
    expect(displayCell(true)).toBe('true');
  });
});
