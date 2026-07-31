import { describe, it, expect } from 'vitest';
import { tableMenuActions, BROWSE_ROWS } from './menu';

const orders = { schema: 'public', name: 'orders' };

describe('tableMenuActions', () => {
  // Parity with the egui build's table right-click menu, which has always
  // offered "select all rows" *and* "count rows"; the desktop shell shipped
  // without the count. Locking the id list keeps a future edit from silently
  // dropping one again.
  it('offers structure, browse, count and copy — in that order', () => {
    expect(tableMenuActions(orders, 'postgres').map((a) => a.id)).toEqual([
      'open-structure',
      'select-top',
      'count-rows',
      'copy-name',
    ]);
  });

  it('browses a bounded first page', () => {
    const browse = tableMenuActions(orders, 'postgres').find(
      (a) => a.id === 'select-top',
    );
    expect(browse).toEqual({
      id: 'select-top',
      n: BROWSE_ROWS,
      sql: `SELECT * FROM "public"."orders" LIMIT ${BROWSE_ROWS};`,
    });
  });

  it('counts every row', () => {
    expect(
      tableMenuActions(orders, 'postgres').find((a) => a.id === 'count-rows'),
    ).toEqual({
      id: 'count-rows',
      sql: 'SELECT COUNT(*) FROM "public"."orders";',
    });
  });

  // The generated SQL follows the connection's dialect: MySQL rejects the
  // ANSI double quote unless the server runs with ANSI_QUOTES.
  it('generates back-quoted SQL for a MySQL connection', () => {
    const actions = tableMenuActions({ schema: 'shop', name: 'orders' }, 'mysql');
    expect(actions.find((a) => a.id === 'select-top')?.sql).toBe(
      `SELECT * FROM \`shop\`.\`orders\` LIMIT ${BROWSE_ROWS};`,
    );
    expect(actions.find((a) => a.id === 'count-rows')?.sql).toBe(
      'SELECT COUNT(*) FROM `shop`.`orders`;',
    );
  });

  // "Copy name" copies the display key, not a quoted SQL fragment — it is for
  // pasting into a chat or a note, not into an editor.
  it('copies the qualified display key, unquoted', () => {
    expect(
      tableMenuActions(orders, 'postgres').find((a) => a.id === 'copy-name'),
    ).toEqual({ id: 'copy-name', text: 'public.orders' });
    expect(
      tableMenuActions({ schema: null, name: 'orders' }, 'turso').find(
        (a) => a.id === 'copy-name',
      ),
    ).toEqual({ id: 'copy-name', text: 'orders' });
  });

  // The sidebar renders before `list_connections` resolves; an undefined kind
  // must still produce a usable (ANSI) menu rather than throw.
  it('still builds a menu when the connection kind is unknown', () => {
    expect(tableMenuActions(orders, undefined).map((a) => a.id)).toEqual([
      'open-structure',
      'select-top',
      'count-rows',
      'copy-name',
    ]);
  });
});
