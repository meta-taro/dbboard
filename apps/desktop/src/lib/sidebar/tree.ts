// The table list as a tree: tables under the database (MySQL) or schema
// (Postgres) they live in (docs/every-database.md).
//
// A connection can now see more than one database, and a flat list of every
// table on a server, each prefixed with where it lives, is harder to read
// than what it replaced. Grouping is the whole fix. It is kept pure so the
// ordering and the one-group rule are tested without a component.

import type { TableInfo } from '$lib/api';

export interface TableGroup {
  /** The database or schema, or `null` for engines that have neither. */
  name: string | null;
  tables: TableInfo[];
}

const byName = (a: string, b: string) => a.localeCompare(b);

/**
 * Tables grouped by `schema`, groups and tables each in name order. An
 * unqualified group comes first. A caller that gets one group back should
 * draw it flat: a parent with a single child is a click with nothing behind
 * it.
 */
export function groupTables(tables: TableInfo[]): TableGroup[] {
  const groups = new Map<string | null, TableInfo[]>();
  for (const table of tables) {
    const list = groups.get(table.schema);
    if (list) list.push(table);
    else groups.set(table.schema, [table]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => (a === null ? -1 : b === null ? 1 : byName(a, b)))
    .map(([name, list]) => ({ name, tables: list.sort((x, y) => byName(x.name, y.name)) }));
}
