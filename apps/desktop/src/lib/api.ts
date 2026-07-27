// Typed wrappers over the Tauri command surface (src-tauri/src/lib.rs). These
// mirror the McpService return shapes; keeping them here means the Svelte
// components never touch `invoke` string names directly.
//
// The Rust structs derive `Serialize` with default (snake_case) field names,
// so the interfaces below keep snake_case to match the JSON on the wire.
// Command *arguments*, by contrast, follow Tauri's camelCase IPC convention.
import { invoke } from '@tauri-apps/api/core';
import type { EditFields } from '$lib/connections/draft';

export interface ConnectionView {
  id: string;
  name: string;
  kind: string;
}

// A table as `list_tables` returns it: `schema` is null on schemaless engines
// (SQLite/libSQL) and set on Postgres-family ones (e.g. "public"). Pass it back
// verbatim to `describeTable`/`listRelationships` — never reconstruct it.
export interface TableInfo {
  schema: string | null;
  name: string;
}

// Address a table for display/keying: "schema.name" when schema-qualified,
// bare name otherwise. Mirrors the backend's `schema.name` match key.
export function tableKey(t: TableInfo): string {
  return t.schema ? `${t.schema}.${t.name}` : t.name;
}

export interface ColumnInfo {
  name: string;
  declared_type: string | null;
  nullable: boolean;
  primary_key: boolean;
  ordinal: number;
  default_value: string | null;
}

export interface TableSchema {
  table: TableInfo;
  columns: ColumnInfo[];
  primary_key: string[];
}

export interface ColumnAnnotation {
  name: string;
  note: string | null;
}

export interface TableAnnotations {
  key: string;
  note: string | null;
  columns: ColumnAnnotation[];
}

export interface AnnotationsView {
  connection_id: string;
  tables: TableAnnotations[];
}

// One table (and/or one of its columns) matched by `search_schema`. The
// backend flags whether the table *name* itself matched; `matched_columns`
// carries the full column info for each column whose name matched (empty on a
// name-only hit).
export interface SchemaMatch {
  table: TableInfo;
  table_name_matched: boolean;
  matched_columns: ColumnInfo[];
}

export interface SchemaSearchView {
  connection_id: string;
  pattern: string;
  matches: SchemaMatch[];
  truncated: boolean;
}

// One foreign-key edge: child (`from`) columns point at parent (`to`) columns,
// aligned 1:1 in key order.
export interface Relationship {
  from_table: TableInfo;
  from_columns: string[];
  to_table: TableInfo;
  to_columns: string[];
  constraint_name: string | null;
}

export interface RelationshipView {
  connection_id: string;
  table: string | null;
  relationships: Relationship[];
  truncated: boolean;
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

export const listTables = (connectionId: string): Promise<TableInfo[]> =>
  invoke('list_tables', { connectionId });

export const describeTable = (
  connectionId: string,
  table: string,
  schema?: string | null,
): Promise<TableSchema> =>
  invoke('describe_table', { connectionId, schema: schema ?? null, table });

export const getAnnotations = (
  connectionId: string,
  table?: string | null,
  column?: string | null,
): Promise<AnnotationsView> =>
  invoke('get_annotations', {
    connectionId,
    table: table ?? null,
    column: column ?? null,
  });

export const searchSchema = (
  connectionId: string,
  pattern: string,
): Promise<SchemaSearchView> =>
  invoke('search_schema', { connectionId, pattern });

export const listRelationships = (
  connectionId: string,
  table?: string | null,
): Promise<RelationshipView> =>
  invoke('list_relationships', { connectionId, table: table ?? null });

export const runReadQuery = (
  connectionId: string,
  sql: string,
  maxRows?: number,
): Promise<QueryOutput> =>
  invoke('run_read_query', { connectionId, sql, maxRows: maxRows ?? null });

// Absolute path of the connections.toml this app reads — shown in the
// first-run empty state and the connection manager's footer.
export const configPath = (): Promise<string> => invoke('config_path');

// --- Connection management (write path, ADR-0062) -----------------------
//
// `kind` is a tagged object shaped by `$lib/connections/draft` — its snake_case
// discriminator matches the backend's KindInput/KindEditInput DTOs. These
// mutate `connections.toml` + the OS keyring; callers refresh the connection
// list from `listConnections` afterwards.

// Non-secret editable fields for the edit form (ADR-0016: secrets are never
// read back out of the keyring). The shape lives in the pure `draft` module.
export const connectionEditFields = (id: string): Promise<EditFields> =>
  invoke('connection_edit_fields', { id });

export const addConnection = (
  id: string,
  name: string,
  kind: Record<string, unknown>,
): Promise<void> => invoke('add_connection', { id, name, kind });

export const updateConnection = (
  id: string,
  name: string,
  kind: Record<string, unknown>,
): Promise<void> => invoke('update_connection', { id, name, kind });

export const deleteConnection = (id: string): Promise<void> =>
  invoke('delete_connection', { id });

// Additive, non-destructive import: ids already present are skipped, never
// overwritten (ADR-0038). Mirrors the backend `ImportReportDto`.
export interface ImportReport {
  imported: string[];
  skipped: string[];
}

// Encrypt all connections to a passphrase-protected `.dbbx` bundle at `path`
// (chosen via the native save dialog). Returns the exported connection count.
export const exportConnections = (
  path: string,
  passphrase: string,
): Promise<number> => invoke('export_connections', { path, passphrase });

// Decrypt and merge a `.dbbx` bundle at `path` into the local store.
export const importConnections = (
  path: string,
  passphrase: string,
): Promise<ImportReport> => invoke('import_connections', { path, passphrase });
