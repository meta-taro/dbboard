import { describe, expect, it } from 'vitest';

import { advanceTrail, firstTrail, pageState, type Cursor } from './pages';

describe('the page trail', () => {
  it('starts with the one cursor page one does not need', () => {
    expect(firstTrail()).toEqual([null]);
  });

  it('records the cursor that reaches the following page', () => {
    const trail = advanceTrail(firstTrail(), 0, [1]);
    expect(trail).toEqual([null, [1]]);
  });

  it('forgets the pages ahead when one is re-read', () => {
    // Walked to page three, then went back to page two and re-read it. The
    // cursor that used to reach page three came from rows that may since have
    // been deleted, so it is not the one to keep.
    let trail: Cursor[] = [null, [10], [20], [30]];
    trail = advanceTrail(trail, 1, [11]);
    expect(trail).toEqual([null, [10], [11]]);
  });

  it('is unchanged in length when the last page is re-read', () => {
    const trail = advanceTrail([null, [10]], 1, null);
    expect(trail).toEqual([null, [10], null]);
  });
});

describe('what the pager may offer', () => {
  const page = (has_more: boolean, next_cursor: Cursor) => ({
    has_more,
    next_cursor,
  });

  it('offers nothing at all before a browse has run', () => {
    expect(pageState(null, 0)).toEqual({
      canPrev: false,
      canNext: false,
      stranded: false,
      atEnd: false,
    });
  });

  it('offers Next while a cursor is on offer', () => {
    const state = pageState(page(true, [42]), 0);
    expect(state.canNext).toBe(true);
    expect(state.canPrev).toBe(false);
    expect(state.stranded).toBe(false);
  });

  it('offers Previous from the second page on', () => {
    expect(pageState(page(true, [42]), 1).canPrev).toBe(true);
  });

  it('calls the end of the table the end of the table', () => {
    const state = pageState(page(false, null), 3);
    expect(state.atEnd).toBe(true);
    expect(state.canNext).toBe(false);
    expect(state.stranded).toBe(false);
  });

  it('says the rest is unreachable rather than pretending it is not there', () => {
    // A table with no primary key: the rows exist, and there is no stable
    // order to resume from. Reporting `atEnd` here would be a lie, and
    // offering Next would be a button that cannot work.
    const state = pageState(page(true, null), 0);
    expect(state.stranded).toBe(true);
    expect(state.canNext).toBe(false);
    expect(state.atEnd).toBe(false);
  });
});
