import { describe, it, expect } from 'vitest';
import {
  CONNECTION_COLORS,
  CONNECTION_TAG_MAX_CHARS,
  colorVar,
  isConnectionColor,
  isConnectionTag,
  markFor,
  markNeedsTag,
} from './marks';

// The colour half. The list is closed on purpose (ADR-0126): a name this build
// cannot render would paint an invisible swatch, which reads as "unmarked" on
// exactly the connection someone bothered to mark.
describe('the colour of a mark', () => {
  it('offers the spectrum, not the alphabet', () => {
    // Sorted by name, red would sit next to purple — the two marks hardest to
    // tell apart at swatch size.
    expect([...CONNECTION_COLORS]).toEqual([
      'red',
      'orange',
      'yellow',
      'green',
      'teal',
      'blue',
      'purple',
      'pink',
    ]);
  });

  it('accepts every name it offers', () => {
    for (const name of CONNECTION_COLORS) expect(isConnectionColor(name)).toBe(true);
  });

  it('rejects a name from a newer build, and a blank one', () => {
    expect(isConnectionColor('chartreuse')).toBe(false);
    expect(isConnectionColor('RED')).toBe(false);
    expect(isConnectionColor('')).toBe(false);
  });

  it('resolves to the theme-aware custom property, never a hex value', () => {
    expect(colorVar('red')).toBe('var(--conn-red)');
  });
});

// The tag half — the one that survives a greyscale screenshot and a
// colour-blind reader, which is why it exists at all (ADR-0126).
describe('the tag of a mark', () => {
  it('accepts a tag up to the limit', () => {
    expect(isConnectionTag('prod')).toBe(true);
    expect(isConnectionTag('x'.repeat(CONNECTION_TAG_MAX_CHARS))).toBe(true);
  });

  it('refuses one character past it', () => {
    expect(isConnectionTag('x'.repeat(CONNECTION_TAG_MAX_CHARS + 1))).toBe(false);
  });

  it('measures characters, so a Japanese tag fits', () => {
    // Twelve kanji are thirty-six bytes and still narrower than twelve latin
    // letters, so bytes would refuse a tag that fits the row perfectly.
    expect(isConnectionTag('本番環境専用接続先設定値')).toBe(true);
  });

  it('counts an emoji once, the way the backend does', () => {
    // `.length` would count two UTF-16 code units, so this side would refuse a
    // tag the backend accepts — and the operator would be stopped mid-word by
    // a limit that is not the real one.
    expect(isConnectionTag('🚨'.repeat(CONNECTION_TAG_MAX_CHARS))).toBe(true);
  });

  it('treats an empty tag as fitting; blank is unmarked, not too long', () => {
    expect(isConnectionTag('')).toBe(true);
  });
});

// What a row actually renders. Kept out of the components because both the
// sidebar and the connection manager render the same mark, and a rule about
// which half survives is not a thing a `{#if}` in two files can hold straight.
describe('reading a mark for a row', () => {
  it('an unmarked connection has nothing to render', () => {
    expect(markFor({}, 'a')).toBe(null);
    expect(markFor({ b: { color: 'red', tag: 'prod' } }, 'a')).toBe(null);
  });

  it('renders both halves when both are set', () => {
    expect(markFor({ a: { color: 'red', tag: 'prod' } }, 'a')).toEqual({
      color: 'red',
      tag: 'prod',
    });
  });

  it('a tag with no colour renders on its own', () => {
    // The tag is the half that carries the meaning, so it never needs the
    // swatch to be legible.
    expect(markFor({ a: { color: null, tag: 'prod' } }, 'a')).toEqual({
      color: null,
      tag: 'prod',
    });
  });

  it('drops a colour this build cannot render, and keeps the tag', () => {
    // A newer build's palette would resolve to `var(--conn-chartreuse)`, which
    // paints nothing — an invisible swatch reads as "unmarked" on exactly the
    // connection somebody bothered to mark.
    expect(markFor({ a: { color: 'chartreuse', tag: 'prod' } }, 'a')).toEqual({
      color: null,
      tag: 'prod',
    });
  });

  it('falls back to the colour name when a hand-edited config gave no tag', () => {
    // The form refuses to save this (see `markNeedsTag`), so it only arrives
    // from someone editing connections.toml. "red" is a poor mark — that is
    // the whole reason the tag exists — but it beats a bare swatch, which a
    // greyscale screenshot and a screen reader both render as nothing at all.
    expect(markFor({ a: { color: 'red', tag: null } }, 'a')).toEqual({
      color: 'red',
      tag: 'red',
    });
  });

  it('a mark with nothing left in it is no mark', () => {
    expect(markFor({ a: { color: null, tag: null } }, 'a')).toBe(null);
    expect(markFor({ a: { color: 'chartreuse', tag: null } }, 'a')).toBe(null);
    expect(markFor({ a: { color: null, tag: '   ' } }, 'a')).toBe(null);
  });
});

// The form's half of the same rule: a colour may not be saved on its own.
describe('a colour needs a tag', () => {
  it('a colour with no tag is refused', () => {
    expect(markNeedsTag('red', '')).toBe(true);
    expect(markNeedsTag('red', '  ')).toBe(true);
  });

  it('a colour with a tag is fine', () => {
    expect(markNeedsTag('red', 'prod')).toBe(false);
  });

  it('no colour asks for nothing — a tag alone, or no mark at all', () => {
    expect(markNeedsTag('', '')).toBe(false);
    expect(markNeedsTag('', 'prod')).toBe(false);
  });
});
