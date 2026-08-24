// The identity marks a connection can carry: a colour and a short tag
// (issue #192).
//
// A connection's colour is stored as one of these *names*, never as a hex
// value: the name is what the operator can say out loud, and it lets each
// theme choose its own value — the light and dark pairs live in
// `$lib/styles/tokens.css` as `--conn-<name>`, and DESIGN.md documents them.
//
// This list is duplicated in `dbboard-config`'s `mark.rs`, because that crate
// has to reject a colour no build can render for callers that are not this
// form. `crates/dbboard-config/tests/mark_drift.rs` fails when the two lists
// stop matching, which is the only moment the mismatch is cheap to fix.
//
// Order is the order the picker offers, and it is the spectrum rather than the
// alphabet: a picker sorted by name puts red next to purple, and the two marks
// hardest to tell apart end up adjacent.
export const CONNECTION_COLORS = [
  'red',
  'orange',
  'yellow',
  'green',
  'teal',
  'blue',
  'purple',
  'pink',
] as const;

export type ConnectionColor = (typeof CONNECTION_COLORS)[number];

// Whether `name` is a colour this build can render. Used to drop a mark set by
// a newer build rather than emitting a `--conn-<unknown>` custom property that
// resolves to nothing and paints an invisible swatch.
export function isConnectionColor(name: string): name is ConnectionColor {
  return (CONNECTION_COLORS as readonly string[]).includes(name);
}

// The CSS custom property holding this colour's value for the active theme.
export function colorVar(name: ConnectionColor): string {
  return `var(--conn-${name})`;
}

// How many characters a tag may carry. Mirrors `CONNECTION_TAG_MAX_CHARS` in
// `dbboard-config`'s `mark.rs`, which is what actually rejects an over-long
// one; this copy exists so the input can stop the operator at the limit
// instead of letting them type past it and lose the tail on save.
// `mark_drift.rs` fails when the two numbers stop matching.
export const CONNECTION_TAG_MAX_CHARS = 12;

// Whether `tag` fits. Counts characters via the iterator rather than `.length`,
// which counts UTF-16 code units: an emoji would otherwise cost two, and a tag
// of six emoji would be refused by this side and accepted by the backend.
export function isConnectionTag(tag: string): boolean {
  return [...tag].length <= CONNECTION_TAG_MAX_CHARS;
}

// A mark as a row renders it: a colour this build can paint, and the words
// beside it. Both may not be absent — a `ConnectionMarkView` always shows
// something (ADR-0126).
export interface ConnectionMarkView {
  color: ConnectionColor | null;
  tag: string;
}

// What the row at `id` should render, or `null` for an unmarked connection.
//
// Two rules, and they are the reason this is not an inline `{#if}` in each of
// the two components that render a mark:
//
//  - A colour name this build does not know is dropped rather than passed to
//    CSS, which would paint nothing and read as unmarked.
//  - The tag is what carries the meaning, so a mark that has lost it falls
//    back to the colour name. That is a poor mark — it says the row is red,
//    not that it is production — but the form refuses to produce one
//    (`markNeedsTag`), so it only arrives from a hand-edited config, where a
//    bare swatch would leave a greyscale screenshot and a screen reader with
//    nothing at all.
export function markFor(
  marks: Record<string, { color: string | null; tag: string | null }>,
  id: string,
): ConnectionMarkView | null {
  const mark = marks[id];
  if (!mark) return null;
  const color =
    mark.color && isConnectionColor(mark.color) ? mark.color : null;
  const tag = (mark.tag ?? '').trim() || color || '';
  return tag ? { color, tag } : null;
}

// Whether the form is holding a colour with no tag — the one combination it
// will not save. Colour alone fails for a colour-blind reader and in a
// greyscale screenshot, and the tag costs four keystrokes (ADR-0126).
export function markNeedsTag(color: string, tag: string): boolean {
  return color.trim().length > 0 && tag.trim().length === 0;
}
