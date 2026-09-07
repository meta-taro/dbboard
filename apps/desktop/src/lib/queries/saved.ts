// Pure helpers for saved queries (ADR-0147). Storage lives in Rust
// (`saved-queries.toml`); what is here is the naming and ordering the list on
// screen needs, kept free of Svelte and Tauri so it is unit-testable.

/** One saved query, as `list_saved_queries` returns it. */
export interface SavedQueryView {
  name: string;
  sql: string;
  /** Epoch milliseconds when it was last written. */
  saved_at: number;
}

/** Longest name proposed automatically. Past this the tail is elided. */
const NAME_MAX = 60;

/** What an unnameable statement is called. The store refuses a blank name. */
const FALLBACK_NAME = 'Untitled query';

/**
 * The name the Save box opens with: the statement's first line, whitespace
 * collapsed and shortened, stepped past any name already taken.
 *
 * The first line rather than a generated summary because the operator has to
 * recognise it a week later, and what they will recognise is what they typed.
 * The `(2)` step matters more than it looks: saving twice from one statement
 * — a tweak, then another — is ordinary, and proposing the taken name would
 * send them into the overwrite confirmation every single time.
 */
export function defaultQueryName(sql: string, existing: SavedQueryView[]): string {
  const firstLine = sql.split('\n')[0]?.trim() ?? '';
  const collapsed = firstLine.replace(/\s+/g, ' ');
  const base =
    collapsed === ''
      ? FALLBACK_NAME
      : collapsed.length > NAME_MAX
        ? `${collapsed.slice(0, NAME_MAX - 1).trimEnd()}…`
        : collapsed;

  const taken = new Set(existing.map((e) => e.name));
  if (!taken.has(base)) return base;
  let n = 2;
  while (taken.has(`${base} (${n})`)) n += 1;
  return `${base} (${n})`;
}

/**
 * A copy ordered most-recently-saved first, ties broken by name.
 *
 * The file keeps insertion order — that is what makes its diffs readable —
 * and the list a person looks at wants the opposite. The name tiebreak is not
 * theoretical tidiness: two saves in the same millisecond would otherwise
 * order differently on each render, and a list that reshuffles itself under
 * the cursor is its own bug report.
 */
export function byRecency(list: SavedQueryView[]): SavedQueryView[] {
  return [...list].sort(
    (a, b) => b.saved_at - a.saved_at || a.name.localeCompare(b.name),
  );
}
