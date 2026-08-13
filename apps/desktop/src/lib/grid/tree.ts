// Turning a document into rows a table can show.
//
// A document database stores trees, and a grid cell is one line. Serialising
// the tree into that line is what produces `{"customer":{"name":"…","address":
// {"city":"…"}}}` — every value present and none of them findable. This module
// flattens the tree into an indented row list instead, so the shape survives
// into a viewer that is still, structurally, a list.
//
// Kept free of Svelte and the DOM: the flattening is where the subtle cases
// live (ordering, empty containers, collapsed subtrees), and those are worth
// testing without a component around them.

export type NodeKind = 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null';

export interface TreeNode {
  /** Dotted path from the root — `lines.0.sku`. Identifies the node for
   *  collapse state, and is stable across re-renders. */
  path: string;
  /** Nesting level; the root document's own fields are 0. */
  depth: number;
  /** The field name, or the index for an array entry. */
  label: string;
  kind: NodeKind;
  /** What the row shows to the right of the label: the value for a leaf, the
   *  size for a container (`{3}`, `[2]`). Language-neutral on purpose — this is
   *  data, not prose, and it must read the same in every locale. */
  preview: string;
  /** Whether opening this node would reveal anything. */
  hasChildren: boolean;
  /** True when this node has children and they are currently hidden. */
  collapsed: boolean;
}

/**
 * Flatten `value` into display rows, omitting the descendants of any path in
 * `collapsed`. The collapsed container itself stays: a subtree the user closed
 * must remain visible, or closing it would look like deleting it.
 */
export function flattenDocument(value: unknown, collapsed: ReadonlySet<string>): TreeNode[] {
  const out: TreeNode[] = [];
  walk(value, '', '', 0, collapsed, out);
  return out;
}

/** The collapse set with `path` flipped. Returns a new set; the caller's stays
 *  untouched so Svelte sees a changed reference. */
export function toggled(collapsed: ReadonlySet<string>, path: string): Set<string> {
  const next = new Set(collapsed);
  if (!next.delete(path)) next.add(path);
  return next;
}

/** Every path that has children — what "collapse all" needs. */
export function allContainerPaths(value: unknown): Set<string> {
  const paths = new Set<string>();
  collect(value, '', paths);
  return paths;
}

function walk(
  value: unknown,
  path: string,
  label: string,
  depth: number,
  collapsed: ReadonlySet<string>,
  out: TreeNode[],
): void {
  const entries = childEntries(value);

  // The root object contributes its fields directly rather than a row of its
  // own: a document is the thing being shown, not a field inside something.
  if (depth === 0 && path === '' && entries !== null) {
    for (const [key, child] of entries) {
      walk(child, key, key, 0, collapsed, out);
    }
    return;
  }

  const hasChildren = entries !== null && entries.length > 0;
  const isCollapsed = hasChildren && collapsed.has(path);
  out.push({
    path,
    depth,
    label,
    kind: kindOf(value),
    preview: previewOf(value, entries),
    hasChildren,
    collapsed: isCollapsed,
  });

  if (!hasChildren || isCollapsed) return;
  for (const [key, child] of entries) {
    walk(child, path === '' ? key : `${path}.${key}`, key, depth + 1, collapsed, out);
  }
}

function collect(value: unknown, path: string, into: Set<string>): void {
  const entries = childEntries(value);
  if (entries === null) return;
  if (path !== '' && entries.length > 0) into.add(path);
  for (const [key, child] of entries) {
    collect(child, path === '' ? key : `${path}.${key}`, into);
  }
}

/** `[key, value]` pairs for a container, or `null` for a leaf. Array indices
 *  become their decimal string, so paths and labels are built the same way for
 *  both container kinds. */
function childEntries(value: unknown): [string, unknown][] | null {
  if (Array.isArray(value)) return value.map((v, i) => [String(i), v]);
  if (typeof value === 'object' && value !== null) return Object.entries(value);
  return null;
}

function kindOf(value: unknown): NodeKind {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  switch (typeof value) {
    case 'object':
      return 'object';
    case 'number':
      return 'number';
    case 'boolean':
      return 'boolean';
    default:
      return 'string';
  }
}

function previewOf(value: unknown, entries: [string, unknown][] | null): string {
  if (entries === null) return value === null ? 'null' : String(value);
  const size = entries.length === 0 ? '' : String(entries.length);
  return Array.isArray(value) ? `[${size}]` : `{${size}}`;
}
