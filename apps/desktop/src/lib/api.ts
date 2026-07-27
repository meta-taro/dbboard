// Typed wrappers over the Tauri command surface (src-tauri/src/lib.rs). These
// mirror the McpService return shapes; keeping them here means the Svelte
// components never touch `invoke` string names directly.
import { invoke } from '@tauri-apps/api/core';

export interface ConnectionView {
  id: string;
  name: string;
  kind: string;
}

export interface Column {
  name: string;
  declared_type: string | null;
}

// A row is a positional list of cells aligned to `columns`. dbboard-core maps
// each cell to a native JSON scalar (Null→null, Integer/Real→number,
// Text→string); blobs are the one tagged shape (`{"$blob":"<base64>"}`).
export type Cell = string | number | boolean | null | { $blob: string };

// Render a cell for the results grid: NULL is explicit, blobs are summarised.
export function displayCell(cell: Cell): string {
  if (cell === null) return 'NULL';
  if (typeof cell === 'object' && '$blob' in cell) return '<blob>';
  return String(cell);
}

export interface QueryOutput {
  columns: Column[];
  rows: Cell[][];
  row_count: number;
  truncated: boolean;
}

export const listConnections = (): Promise<ConnectionView[]> =>
  invoke('list_connections');

export const listTables = (connectionId: string): Promise<string[]> =>
  invoke('list_tables', { connectionId });

export const runReadQuery = (
  connectionId: string,
  sql: string,
  maxRows?: number,
): Promise<QueryOutput> =>
  invoke('run_read_query', { connectionId, sql, maxRows: maxRows ?? null });
