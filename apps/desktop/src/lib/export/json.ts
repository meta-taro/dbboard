// JSON serialization of a result grid, for the "Save As" export (ADR-0146).
//
// Why this is not `toDelimited` with different punctuation: the delimited
// export flattens every cell to a string because a spreadsheet cell has
// nowhere else to put one — NULL becomes `''`, a document becomes its JSON
// *text*, a blob becomes `<blob>`. That is the right trade for something a
// person opens in Excel and the wrong one for something a program reads back.
// This path keeps the types the engine returned.
//
// Kept free of Svelte and the DOM so it is unit-testable in isolation.
import { isDocument, type Cell, type Column } from '$lib/api';

/**
 * The object keys the export uses for `columns`, in column order.
 *
 * A JSON object cannot hold two members of the same name, and a result set
 * can: `SELECT a.id, b.id` is ordinary SQL. Writing both under `id` would drop
 * a column silently — the file would parse, and be wrong. So repeats are
 * numbered by their own occurrence (`id`, `id:2`, `id:3`), which leaves the
 * common case — every name distinct — spelled exactly as the engine reported
 * it. An unnamed column (an expression in SQLite) is keyed by its 1-based
 * position instead of the empty string, which is a legal JSON key that no
 * consumer can address.
 */
export function columnKeys(columns: Column[]): string[] {
  const seen = new Map<string, number>();
  return columns.map((c, i) => {
    const name = c.name === '' ? `column:${i + 1}` : c.name;
    const count = (seen.get(name) ?? 0) + 1;
    seen.set(name, count);
    return count === 1 ? name : `${name}:${count}`;
  });
}

/**
 * The value a cell contributes to the JSON export.
 *
 * The rule for the two tagged shapes is one rule, not two exceptions: a tag
 * exists to carry type information the transport cannot express by itself.
 * JSON *can* express a document, so `$json` is stripped and the document nests
 * natively. JSON cannot express bytes, so `$blob` stays — dropping it would
 * leave base64 that reads as ordinary text with nothing to say it is not.
 */
export function jsonValue(cell: Cell): unknown {
  if (isDocument(cell)) return cell.$json;
  return cell;
}

/**
 * Serialize columns + rows as a JSON array of row objects.
 *
 * An array of objects rather than the grid's own shape (`columns` beside
 * `rows`-as-arrays) because the file's job is to be read by the next tool, and
 * that is the shape `jq`, a dataframe loader and a hand-written script all
 * expect. The grid shape is what the API contract already offers to anything
 * that wants column metadata (`docs/api-contract.md`).
 *
 * Indented, and terminated with a newline: this is a file a person names in a
 * save dialog, so some of them will open it in an editor, and every text file
 * ends with a newline. No byte-order mark — the CSV export leads with one so
 * Excel detects UTF-8 (ADR-0035), and JSON parsers reject it.
 */
export function toJsonFile(columns: Column[], rows: Cell[][]): string {
  const keys = columnKeys(columns);
  const objects = rows.map((row) => {
    const out: Record<string, unknown> = {};
    keys.forEach((key, i) => {
      out[key] = jsonValue(row[i]);
    });
    return out;
  });
  return `${JSON.stringify(objects, null, 2)}\n`;
}
