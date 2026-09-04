// Keyset page trail for a browsed table (ADR-0145).
//
// The backend holds nothing between pages: a cursor is the previous page's
// last row's key values, not a database cursor. That is what makes Previous
// this side's job — stepping back is re-running an earlier page from a cursor
// we still have, so the trail of them lives here.
//
// `trail[i]` is the cursor that *fetches* page `i`. `trail[0]` is always
// null: page one needs none.

import type { Cell, QueryOutput } from '$lib/api';

export type Cursor = Cell[] | null;

/** The trail a fresh browse starts from. */
export function firstTrail(): Cursor[] {
  return [null];
}

/** Record the cursor a page handed back, as the way to reach the page after it.
 *
 *  Everything past `index` is dropped. Arriving at a page from Previous
 *  invalidates the trail that was ahead of us: rows may have been inserted or
 *  deleted since, so the same page can hand back a different next cursor. */
export function advanceTrail(
  trail: Cursor[],
  index: number,
  next: Cursor,
): Cursor[] {
  return [...trail.slice(0, index + 1), next];
}

/** What the pager may offer, given the page on screen. */
export interface PageState {
  canPrev: boolean;
  canNext: boolean;
  /** More rows exist and none of them is reachable: the table has no primary
   *  key, so there is no stable order to resume from. The pair looks
   *  contradictory and is not — "there is more" and "here is how to reach it"
   *  are separate answers (ADR-0145). */
  stranded: boolean;
  /** This is the last page, and it is the last page because the table ended. */
  atEnd: boolean;
}

export function pageState(
  result: Pick<QueryOutput, 'has_more' | 'next_cursor'> | null,
  index: number,
): PageState {
  const hasMore = result?.has_more ?? false;
  const cursor = result?.next_cursor ?? null;
  return {
    canPrev: result !== null && index > 0,
    canNext: result !== null && cursor !== null,
    stranded: result !== null && hasMore && cursor === null,
    atEnd: result !== null && !hasMore,
  };
}
