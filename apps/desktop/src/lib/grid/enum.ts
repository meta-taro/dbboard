// Reading the members of an ENUM column out of its declared type.
//
// MySQL reports an enum column as `enum('draft','sent','paid')` in
// `information_schema.columns.column_type`, which is the only place the member
// list appears — the result-set metadata says just `ENUM`. So an enum dropdown
// has to come from the table schema (`describeTable`), and this module turns
// that one string into the list of choices.
//
// Kept pure and separate because the escaping is where this goes wrong: a
// comma or a quote inside a member must not split it into two, and a
// declaration we cannot parse must produce nothing at all rather than a
// half-read list — an editor offering the wrong members is worse than the
// plain text box it replaces.

/** MySQL's backslash escapes inside a string literal. Anything else after a
 *  backslash stands for itself (`\%` is `%`). */
const ESCAPES: Record<string, string> = {
  '0': '\0',
  b: '\b',
  n: '\n',
  r: '\r',
  t: '\t',
  Z: '\x1a',
};

/**
 * The members of an ENUM column, in declaration order, or null when the type
 * is not a single-valued enum or cannot be parsed.
 *
 * SET is deliberately excluded: it holds several members at once, and a
 * single-select would silently drop all but one.
 */
export function enumVariants(declaredType: string | null): string[] | null {
  if (!declaredType) return null;
  const head = /^\s*enum\s*\(/i.exec(declaredType);
  if (!head) return null;

  const s = declaredType;
  let i = head[0].length;
  const members: string[] = [];

  for (;;) {
    i = skipSpace(s, i);
    if (s[i] !== "'") return null;
    const literal = readLiteral(s, i);
    if (literal === null) return null;
    members.push(literal.value);

    i = skipSpace(s, literal.next);
    if (s[i] === ',') {
      i++;
      continue;
    }
    if (s[i] === ')') return members.length > 0 ? members : null;
    // Anything else means we have lost the plot mid-list.
    return null;
  }
}

/**
 * What the dropdown offers for `draft`, given a column's declared members.
 *
 * A draft outside the members — a value written before the type was narrowed,
 * or the empty draft a NULL starts from — is kept at the head of the list.
 * Without it, merely opening the editor on such a row would rewrite the value
 * to the first member, and the operator would have no way back to what was
 * there. `members` is left untouched: it belongs to the schema, not the row.
 */
export function enumOptions(members: readonly string[], draft: string): string[] {
  return members.includes(draft) ? [...members] : [draft, ...members];
}

function skipSpace(s: string, i: number): number {
  while (i < s.length && /\s/.test(s[i])) i++;
  return i;
}

/** Read one `'...'` literal starting at `i`, returning its unescaped value and
 *  the index just past the closing quote. Null when it never closes. */
function readLiteral(s: string, i: number): { value: string; next: number } | null {
  let value = '';
  let at = i + 1; // past the opening quote
  while (at < s.length) {
    const ch = s[at];
    if (ch === '\\') {
      const escaped = s[at + 1];
      if (escaped === undefined) return null;
      value += ESCAPES[escaped] ?? escaped;
      at += 2;
      continue;
    }
    if (ch === "'") {
      // A doubled quote is one literal quote, not the end of the string.
      if (s[at + 1] === "'") {
        value += "'";
        at += 2;
        continue;
      }
      return { value, next: at + 1 };
    }
    value += ch;
    at++;
  }
  return null;
}

/**
 * Enum members by column name, for every enum column of a table schema.
 * Columns that are not enums are simply absent, so a lookup miss means "edit
 * this as text".
 */
export function enumColumns(
  columns: readonly { name: string; declared_type: string | null }[],
): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const col of columns) {
    const variants = enumVariants(col.declared_type);
    if (variants) out[col.name] = variants;
  }
  return out;
}
