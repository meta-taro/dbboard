// The one comparison the window is holding (ADR-0149).
//
// It lives outside the Compare panel because two surfaces read it: the panel
// draws the report, and the sidebar draws a marker on every table the report
// mentions. Two independent copies would drift for as long as it took the
// slower one to land — the same reason the connection marks live in
// `workspace` rather than in each renderer.
import type { SchemaDiff, TableInfo } from '$lib/api';
import { differingTables, sectionId, type DiffMark } from './diff';

class CompareState {
  /** The report on screen, or null before the first comparison. */
  diff = $state<SchemaDiff | null>(null);
  /** The two sides the report was built from, for labelling it. */
  leftId = $state('');
  rightId = $state('');

  /** A request to scroll the report to one table's section, raised by the
   *  sidebar and consumed by the panel. The seq lets the panel act on each
   *  request exactly once, even when the same table is clicked twice — the
   *  same shape `workspace.queryRequest` uses. */
  jump = $state<{ id: string; seq: number } | null>(null);
  #seq = 0;

  /** Which tables the report mentions, keyed as the sidebar keys its rows. */
  marks = $derived<Map<string, DiffMark>>(
    this.diff ? differingTables(this.diff) : new Map(),
  );

  /** Whether a report is on screen at all — what the sidebar checks before
   *  drawing markers, and the panel before offering to jump. */
  get active(): boolean {
    return this.diff !== null;
  }

  /** Hold a fresh report. */
  hold(diff: SchemaDiff, leftId: string, rightId: string) {
    this.diff = diff;
    this.leftId = leftId;
    this.rightId = rightId;
    this.jump = null;
  }

  /**
   * Ask the panel to scroll to `table`.
   *
   * Deliberately a no-op for a table the report does not mention: there is
   * nowhere to go, and scrolling somewhere arbitrary is worse than staying
   * put.
   */
  jumpTo(table: TableInfo) {
    if (!this.marks.has(keyOf(table))) return;
    this.#seq += 1;
    this.jump = { id: sectionId(table), seq: this.#seq };
  }

  /** Drop the report — on an explicit clear, or when a compared connection
   *  goes away. */
  clear() {
    this.diff = null;
    this.leftId = '';
    this.rightId = '';
    this.jump = null;
  }
}

function keyOf(table: TableInfo): string {
  return table.schema ? `${table.schema}.${table.name}` : table.name;
}

export const compare = new CompareState();
