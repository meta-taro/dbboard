import { describe, it, expect } from 'vitest';
import { flattenDocument, toggled, allContainerPaths, type TreeNode } from './tree';

const NONE = new Set<string>();

/** `path:label:kind` — enough to read an expectation at a glance. */
function shape(nodes: TreeNode[]): string[] {
  return nodes.map((n) => `${'  '.repeat(n.depth)}${n.label}:${n.kind}=${n.preview}`);
}

describe('flattenDocument', () => {
  it('lists an object as its fields, in insertion order', () => {
    expect(shape(flattenDocument({ b: 1, a: 'x' }, NONE))).toEqual([
      'b:number=1',
      'a:string=x',
    ]);
  });

  it('indents a nested object under its key', () => {
    const doc = { customer: { name: 'Sample Customer', tier: 'standard' } };
    expect(shape(flattenDocument(doc, NONE))).toEqual([
      'customer:object={2}',
      '  name:string=Sample Customer',
      '  tier:string=standard',
    ]);
  });

  it('numbers array entries by index', () => {
    const doc = { lines: [{ sku: 'SKU-1000' }, { sku: 'SKU-1003' }] };
    expect(shape(flattenDocument(doc, NONE))).toEqual([
      'lines:array=[2]',
      '  0:object={1}',
      '    sku:string=SKU-1000',
      '  1:object={1}',
      '    sku:string=SKU-1003',
    ]);
  });

  it('keeps going past the second level', () => {
    const doc = { customer: { address: { city: 'Springfield' } } };
    const nodes = flattenDocument(doc, NONE);
    expect(nodes.at(-1)).toMatchObject({ label: 'city', depth: 2, preview: 'Springfield' });
  });

  it('renders each scalar as itself, null included', () => {
    const doc = { s: 'text', n: 1.5, t: true, f: false, z: null };
    expect(shape(flattenDocument(doc, NONE))).toEqual([
      's:string=text',
      'n:number=1.5',
      't:boolean=true',
      'f:boolean=false',
      'z:null=null',
    ]);
  });

  it('marks an empty container as having nothing to open', () => {
    const nodes = flattenDocument({ lines: [], meta: {} }, NONE);
    expect(shape(nodes)).toEqual(['lines:array=[]', 'meta:object={}']);
    expect(nodes.every((n) => !n.hasChildren)).toBe(true);
  });

  it('gives every node a path that identifies it uniquely', () => {
    const doc = { lines: [{ sku: 'a' }, { sku: 'b' }], sku: 'c' };
    const paths = flattenDocument(doc, NONE).map((n) => n.path);
    expect(paths).toEqual(['lines', 'lines.0', 'lines.0.sku', 'lines.1', 'lines.1.sku', 'sku']);
    expect(new Set(paths).size).toBe(paths.length);
  });

  it('hides the descendants of a collapsed container but keeps the container', () => {
    const doc = { customer: { name: 'x', address: { city: 'y' } }, total: 1 };
    const nodes = flattenDocument(doc, new Set(['customer']));
    expect(shape(nodes)).toEqual(['customer:object={2}', 'total:number=1']);
    expect(nodes[0].collapsed).toBe(true);
  });

  it('collapses one branch without touching its sibling', () => {
    const doc = { a: { x: 1 }, b: { y: 2 } };
    expect(shape(flattenDocument(doc, new Set(['b'])))).toEqual([
      'a:object={1}',
      '  x:number=1',
      'b:object={1}',
    ]);
  });

  // A cell holds whatever the adapter put there; a bare scalar is not expected
  // but must not blow up the viewer.
  it('survives a document that is not an object', () => {
    expect(shape(flattenDocument('bare', NONE))).toEqual([':string=bare']);
    expect(shape(flattenDocument(null, NONE))).toEqual([':null=null']);
  });
});

describe('toggled', () => {
  it('opens a collapsed path and closes an open one', () => {
    expect([...toggled(NONE, 'a')]).toEqual(['a']);
    expect([...toggled(new Set(['a']), 'a')]).toEqual([]);
  });

  it('leaves the other paths alone and does not mutate the input', () => {
    const before = new Set(['a', 'b']);
    const after = toggled(before, 'b');
    expect([...after]).toEqual(['a']);
    expect([...before]).toEqual(['a', 'b']);
  });
});

describe('allContainerPaths', () => {
  it('is every path that can be opened, nested ones included', () => {
    const doc = { customer: { address: { city: 'y' } }, lines: [{ sku: 'a' }], total: 1 };
    expect([...allContainerPaths(doc)]).toEqual([
      'customer',
      'customer.address',
      'lines',
      'lines.0',
    ]);
  });

  it('skips empty containers, which have nothing to collapse', () => {
    expect([...allContainerPaths({ a: {}, b: [] })]).toEqual([]);
  });
});
