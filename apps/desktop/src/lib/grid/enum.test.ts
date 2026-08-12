import { describe, it, expect } from 'vitest';
import { enumVariants, enumColumns } from './enum';

describe('enumVariants', () => {
  it('reads the variants of a MySQL ENUM declaration', () => {
    expect(enumVariants("enum('draft','sent','paid')")).toEqual([
      'draft',
      'sent',
      'paid',
    ]);
  });

  it('accepts the keyword in any case, and stray spacing', () => {
    expect(enumVariants("ENUM('x','y')")).toEqual(['x', 'y']);
    expect(enumVariants("Enum ('x')")).toEqual(['x']);
  });

  it('is not fooled by a comma inside a value', () => {
    expect(enumVariants("enum('a,b','c')")).toEqual(['a,b', 'c']);
  });

  // MySQL doubles a quote inside a value, and also accepts backslash escapes.
  // Getting these wrong would silently split one variant into two.
  it('unescapes quotes', () => {
    expect(enumVariants("enum('a''b')")).toEqual(["a'b"]);
    expect(enumVariants("enum('a\\'b')")).toEqual(["a'b"]);
    expect(enumVariants("enum('a\\\\b')")).toEqual(['a\\b']);
  });

  // '' is a legal ENUM member in MySQL and is distinct from NULL.
  it('keeps an empty-string member', () => {
    expect(enumVariants("enum('','x')")).toEqual(['', 'x']);
  });

  it('is null for a column that is not an enum', () => {
    expect(enumVariants(null)).toBeNull();
    expect(enumVariants('varchar(255)')).toBeNull();
    expect(enumVariants('int unsigned')).toBeNull();
    expect(enumVariants('')).toBeNull();
  });

  // A SET column holds several members at once. Offering a single-select would
  // quietly drop every value but one, so it is left as free text.
  it('is null for SET, which is not single-valued', () => {
    expect(enumVariants("set('a','b')")).toBeNull();
  });

  // A column type we cannot parse must fall back to the text editor rather
  // than present an empty dropdown that can only write the wrong thing.
  it('is null when the declaration does not parse', () => {
    expect(enumVariants("enum('a'")).toBeNull();
    expect(enumVariants('enum()')).toBeNull();
    expect(enumVariants('enum(a,b)')).toBeNull();
  });
});

describe('enumColumns', () => {
  it('maps only the enum columns of a schema', () => {
    expect(
      enumColumns([
        { name: 'id', declared_type: 'int' },
        { name: 'status', declared_type: "enum('open','closed')" },
        { name: 'note', declared_type: null },
        { name: 'tags', declared_type: "set('a','b')" },
      ]),
    ).toEqual({ status: ['open', 'closed'] });
  });

  it('is empty when the table has no enum column', () => {
    expect(enumColumns([{ name: 'id', declared_type: 'int' }])).toEqual({});
  });
});
