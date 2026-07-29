// Typed wrappers over the Tauri command surface (src-tauri/src/lib.rs). These
// mirror the McpService return shapes; keeping them here means the Svelte
// components never touch `invoke` string names directly.
//
// The Rust structs derive `Serialize` with default (snake_case) field names,
// so the interfaces below keep snake_case to match the JSON on the wire.
// Command *arguments*, by contrast, follow Tauri's camelCase IPC convention.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { EditFields } from '$lib/connections/draft';
import type { CellEdit, KeyColumn } from '$lib/grid/edit';
import type { DumpPlan, DumpOutcome, DumpProgress } from '$lib/backup/plan';
import type {
  RestorePlan,
  RestoreOutcome,
  RestoreProgress,
  OnError,
} from '$lib/restore/plan';
import type {
  AiStatus,
  AiChunk,
  AiOutcome,
  AiProviderView,
} from '$lib/ai/panel';

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

// Set (or clear) a table's local note (ADR-0045). `table` is the
// schema-qualified key from `tableKey()`. A blank `note` deletes it — the
// backend trims and treats empty as delete, so pass raw editor text.
export const setTableNote = (
  connectionId: string,
  table: string,
  note: string,
): Promise<void> => invoke('set_table_note', { connectionId, table, note });

// Set (or clear) one column's local note (ADR-0045). Same key/blank-deletes
// semantics as `setTableNote`.
export const setColumnNote = (
  connectionId: string,
  table: string,
  column: string,
  note: string,
): Promise<void> =>
  invoke('set_column_note', { connectionId, table, column, note });

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

// Apply one row's staged edits as a single UPDATE (ADR-0042) — the app's first
// DB write path, deliberately NOT exposed to MCP agents. `schema` is the
// table's schema (null on SQLite/libSQL); `key` carries the row's primary-key
// columns with their ORIGINAL values, `edits` the changed cells. Resolves only
// when exactly one row changed; otherwise rejects with the backend's message
// (0 rows = the row was changed/deleted under us, >1 = a non-unique key).
export const updateRow = (
  connectionId: string,
  table: TableInfo,
  key: KeyColumn[],
  edits: CellEdit[],
): Promise<void> =>
  invoke('update_row', {
    connectionId,
    schema: table.schema,
    table: table.name,
    key,
    edits,
  });

// Absolute path of the connections.toml this app reads — shown in the
// first-run empty state and the connection manager's footer.
export const configPath = (): Promise<string> => invoke('config_path');

// Write a UTF-8 text file to a user-chosen `path` (ADR-0035 result export).
// The caller builds the delimited body (BOM-prefixed for the `.csv` form via
// `toDelimitedFile`) and picks `path` with the native save dialog; the write
// happens in Rust so the file lands at the chosen path.
export const saveTextFile = (path: string, contents: string): Promise<void> =>
  invoke('save_text_file', { path, contents });

// --- Logical backup / dump (write-to-file path, ADR-0049/0050) ----------
//
// The dump reads the whole database and writes SQL to a user-chosen file. It
// is deliberately NOT an MCP tool — external agents stay read-only; only the
// desktop app can trigger it (mirroring inline cell editing, ADR-0063).

// Preflight: count every table so the UI can size the dump and warn before a
// large backup (`DumpPlan` is not serialisable across IPC, so this flat DTO
// stands in). Reads only.
export const planDump = (connectionId: string): Promise<DumpPlan> =>
  invoke('plan_dump', { connectionId });

// Run the dump to a user-chosen `path`, streaming `dump:progress` events. The
// backend re-plans internally. Resolves with the outcome (including any
// per-table failures/truncations and whether it was cancelled); rejects only
// when the output file cannot be opened or written.
export const runDump = (
  connectionId: string,
  path: string,
): Promise<DumpOutcome> => invoke('run_dump', { connectionId, path });

// Request cancellation of the in-flight dump. The run stops at its next
// table/page checkpoint and resolves with a `cancelled` outcome, keeping the
// partial file.
export const cancelDump = (): Promise<void> => invoke('cancel_dump');

// Subscribe to dump progress for the duration of one run. Call the returned
// unlisten when the run ends (or the component unmounts) to detach.
export const onDumpProgress = (
  handler: (progress: DumpProgress) => void,
): Promise<UnlistenFn> =>
  listen<DumpProgress>('dump:progress', (event) => handler(event.payload));

// --- Logical restore / import (write-into-DB path, ADR-0051) ------------
//
// A restore reads a user-chosen `.sql` file and applies it to the target
// database. Like the dump it is deliberately NOT an MCP tool — external agents
// stay read-only; only the desktop app can trigger this write surface.

// Preflight: read and classify the `.sql` file at `path` and list the target's
// existing tables, so the UI can size the restore and decide whether the
// empty-target confirmation is needed (`RestorePlan` is not serialisable across
// IPC, so this flat DTO stands in). Reads only.
export const planRestore = (
  connectionId: string,
  path: string,
): Promise<RestorePlan> => invoke('plan_restore', { connectionId, path });

// Apply the `.sql` file at `path` to `connectionId`, streaming
// `restore:progress` events. The backend re-reads and re-plans internally.
// `confirmed` must be true to write into a non-empty target; `onError`
// (`"stop"` | `"continue"`) only affects the per-statement (non-atomic) path.
// Resolves with the outcome (including per-statement failures and whether it
// was cancelled); rejects only when the file cannot be read or the run errors.
export const runRestore = (
  connectionId: string,
  path: string,
  confirmed: boolean,
  onError: OnError,
): Promise<RestoreOutcome> =>
  invoke('run_restore', { connectionId, path, confirmed, onError });

// Request cancellation of the in-flight restore. On the per-statement path the
// run stops at the next statement boundary and resolves with a `cancelled`
// outcome; on the atomic path the flag is only observed before the batch starts.
export const cancelRestore = (): Promise<void> => invoke('cancel_restore');

// Subscribe to restore progress for the duration of one run. Call the returned
// unlisten when the run ends (or the component unmounts) to detach.
export const onRestoreProgress = (
  handler: (progress: RestoreProgress) => void,
): Promise<UnlistenFn> =>
  listen<RestoreProgress>('restore:progress', (event) =>
    handler(event.payload),
  );

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

// --- AI assistant (ADR-0052) --------------------------------------------
//
// The assistant explains SQL and drafts queries from a description. It is
// deliberately NOT an MCP tool — external agents stay read-only. The guardrail
// is enforced in Rust (src-tauri/src/ai.rs): Explain sends only the SQL text;
// Suggest additionally sends table/column *names*; row data never leaves. The
// API key lives only in the OS keyring, never in TOML or the WebView.

// Whether the assistant is usable right now (a provider is active), its label,
// and whether provider management is available on this host. Cheap read; call
// on every panel open.
export const aiStatus = (): Promise<AiStatus> => invoke('ai_status');

// Explain the SQL the user typed. `connectionId` only supplies the dialect tag
// (optional). Streams `ai:chunk` events; resolves with the whole answer.
export const aiExplain = (
  sql: string,
  connectionId?: string | null,
): Promise<AiOutcome> =>
  invoke('ai_explain', { connectionId: connectionId ?? null, sql });

// Draft SQL from a natural-language prompt. The backend attaches the target's
// table/column names; with `includeDetails` it also fans out `describe_table`
// for column metadata (still no row data). Streams `ai:chunk` events.
export const aiSuggest = (
  connectionId: string,
  prompt: string,
  includeDetails: boolean,
): Promise<AiOutcome> =>
  invoke('ai_suggest', { connectionId, prompt, includeDetails });

// Request cancellation of the in-flight AI request. The stream stops at its
// next event and resolves with a `cancelled` outcome holding the partial text.
export const cancelAi = (): Promise<void> => invoke('cancel_ai');

// Subscribe to streaming deltas for the duration of one request. Call the
// returned unlisten when the request ends (or the component unmounts).
export const onAiChunk = (
  handler: (chunk: AiChunk) => void,
): Promise<UnlistenFn> =>
  listen<AiChunk>('ai:chunk', (event) => handler(event.payload));

// List every configured provider (id / name / kind / model / active). Never
// includes the api key. Rejects when provider storage is unavailable.
export const listAiProviders = (): Promise<AiProviderView[]> =>
  invoke('list_ai_providers');

// Add a provider. `kind` is a tagged `AiKindInput` from `$lib/ai/panel`'s
// `buildAddKindInput`; the key is written to the keyring, the rest to
// `ai-providers.toml`. Does not auto-activate — the caller uses `setActive`.
export const addAiProvider = (
  id: string,
  name: string,
  kind: Record<string, unknown>,
): Promise<void> => invoke('add_ai_provider', { id, name, kind });

// Edit a provider's name / model / key (the id and kind are immutable). A
// blank/absent `apiKey` keeps the stored secret. If the edited provider is
// active, the live slot is rebuilt so the change takes effect immediately.
export const updateAiProvider = (
  id: string,
  name: string,
  model?: string,
  apiKey?: string,
): Promise<void> =>
  invoke('update_ai_provider', {
    id,
    name,
    model: model ?? null,
    apiKey: apiKey ?? null,
  });

// Delete a provider and purge its keyring secret. If it was active, the live
// slot is cleared to match.
export const deleteAiProvider = (id: string): Promise<void> =>
  invoke('delete_ai_provider', { id });

// Activate a provider (or clear the active one with `id = null`). The provider
// is built first, so a bad key fails without leaving a broken active id.
export const setActiveAiProvider = (id: string | null): Promise<void> =>
  invoke('set_active_ai_provider', { id });
