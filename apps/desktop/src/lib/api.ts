// Typed wrappers over the Tauri command surface (src-tauri/src/lib.rs). These
// mirror the McpService return shapes; keeping them here means the Svelte
// components never touch `invoke` string names directly.
//
// The Rust structs derive `Serialize` with default (snake_case) field names,
// so the interfaces below keep snake_case to match the JSON on the wire.
// Command *arguments*, by contrast, follow Tauri's camelCase IPC convention.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
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
import type { AvailableUpdate, DownloadEvent } from '$lib/update/notice';

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
  // Tables the sweep could list but not introspect (a denied PRAGMA, a revoked
  // grant). Their edges are missing from `relationships` — reported rather
  // than swallowed, because "no foreign keys" and "we could not look" differ.
  unreadable_tables: TableInfo[];
}

export interface Column {
  name: string;
  declared_type: string | null;
}

// A row is a positional list of cells aligned to `columns`. dbboard-core maps
// each cell to a native JSON scalar (Null→null, Integer/Real→number,
// Text→string); blobs and documents are the two tagged shapes
// (`{"$blob":"<base64>"}`, `{"$json":<tree>}` — see docs/api-contract.md).
export type Cell = string | number | boolean | null | { $blob: string } | { $json: unknown };

// Whether a cell is a document from a document store (ADR-0091). The tag is
// what keeps it apart from a Text cell that merely looks like JSON.
export function isDocument(cell: Cell): cell is { $json: unknown } {
  return typeof cell === 'object' && cell !== null && '$json' in cell;
}

// Render a cell for the results grid: NULL is explicit, blobs are summarised,
// documents show their JSON so a tree never reaches the grid as
// "[object Object]".
export function displayCell(cell: Cell): string {
  if (cell === null) return 'NULL';
  if (typeof cell === 'object' && '$blob' in cell) return '<blob>';
  if (isDocument(cell)) return JSON.stringify(cell.$json);
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

// Ask the SSH server for its host-key fingerprint (`SHA256:…`) so the form can
// offer it for pinning. Opens a connection to `host`, but never authenticates —
// see the command's doc comment.
export const probeSshHostKey = (host: string, port: number): Promise<string> =>
  invoke('probe_ssh_host_key', { host, port });

// `ssh` is a tagged SshInput (or null for no tunnel) / SshEditInput, shaped by
// `$lib/connections/draft`. The tunnel fronts the connection; its secrets ride
// inline on add and keep-or-overwrite on edit (ADR-0069).
//
// `mcpWrite` is the MCP write gate (ADR-0087) — a permission, not a secret, so
// it rides in plain and comes back out on edit. `mcpAlias` is the agent-facing
// name (ADR-0088); blank means none, and the backend trims it.
export const addConnection = (
  id: string,
  name: string,
  kind: Record<string, unknown>,
  ssh: Record<string, unknown> | null,
  mcpWrite: boolean,
  mcpAlias: string,
): Promise<void> =>
  invoke('add_connection', { id, name, kind, ssh, mcpWrite, mcpAlias });

// `keepPassword` is the structured-input counterpart of a blank secret: the
// form rebuilt the DSN from the parts it was shown, which never included the
// password, so the backend grafts the stored one back on (ADR-0080).
export const updateConnection = (
  id: string,
  name: string,
  kind: Record<string, unknown>,
  ssh: Record<string, unknown>,
  keepPassword: boolean,
  mcpWrite: boolean,
  mcpAlias: string,
): Promise<void> =>
  invoke('update_connection', {
    id,
    name,
    kind,
    ssh,
    keepPassword,
    mcpWrite,
    mcpAlias,
  });

// Copy a connection into a new one that owns its own keychain slots, seeded
// with the source's secret values (issue #213). The copy drops the MCP alias
// (it is a unique handle) and leaves MCP writes off (nobody approved writes to
// the copy's database yet).
//
// Rejects a source that itself points at another connection's slot: reading
// that slot to seed a third entry is the thing the import guard refuses. Repair
// it first with `repairConnectionRef`.
export const duplicateConnection = (
  id: string,
  newId: string,
  newName: string,
): Promise<void> => invoke('duplicate_connection', { id, newId, newName });

// Re-point one of `id`'s keychain slots at a slot of its own and store
// `secret` there (issue #213).
//
// The secret is asked for rather than copied out of the slot being abandoned,
// because that value belongs to another connection. That slot is left alone
// for the same reason — this stops referencing it, nothing more.
export const repairConnectionRef = (
  id: string,
  keyRef: string,
  secret: string,
): Promise<void> => invoke('repair_connection_ref', { id, keyRef, secret });

// Every connection pointing at a keychain slot minted for a different one
// (issue #194). Shown in the connection list rather than only at export time
// (issue #213): it is also the reason such a connection cannot be duplicated.
export const foreignConnectionRefs = (): Promise<ForeignRef[]> =>
  invoke('foreign_connection_refs');

export const deleteConnection = (id: string): Promise<void> =>
  invoke('delete_connection', { id });

// Drop the cached adapter and dial again. The backend already re-checks an
// idle adapter before handing it out, so this is not needed to *recover* — it
// is needed to recover now, without guessing whether the next click will work.
// Resolves only once the fresh connection has answered a ping.
export const reconnectConnection = (id: string): Promise<void> =>
  invoke('reconnect_connection', { id });

// An entry refused because a keychain slot it names belongs to a different
// connection (ADR-0038). Both sides of the collision travel because neither
// alone is actionable: the refused id is absent from the store afterwards, so
// an operator told only the id sees a broken import rather than a deliberate
// refusal (ADR-0112). No field here is a secret value — `key_ref` is the slot
// name, not its contents.
export interface RefusedEntry {
  id: string;
  key_ref: string;
  owner: string;
}

// The five lists partition the bundle's entries (ADR-0038, ADR-0105,
// ADR-0112). `overwritten` is only ever non-empty when the import asked for
// it. The three not-imported reasons stay apart because only
// `skipped_existing` means "already present", and only `skipped_existing` is
// resolved by re-importing with overwrite on.
// Mirrors the backend `ImportReportDto`.
export interface ImportReport {
  imported: string[];
  overwritten: string[];
  skipped_existing: string[];
  duplicate_in_bundle: string[];
  refused: RefusedEntry[];
}

// An entry in the local store whose keychain slot was minted for a different
// connection (issue #194). A ref only ever comes from `dbboard.{id}.{field}`
// and an id cannot be renamed, so this state is not reachable through the app
// — it means a hand-edited `connections.toml` or an import predating ADR-0038.
// Mirrors the backend `ForeignRefDto`; no field is a secret value.
export interface ForeignRef {
  id: string;
  key_ref: string;
  owner: string;
}

// Mirrors the backend `ExportReportDto`. `foreign_refs` is a warning about the
// bundle that was just written, not a reason it was not written: the export
// succeeds either way.
export interface ExportReport {
  exported: number;
  foreign_refs: ForeignRef[];
}

// Encrypt connections to a passphrase-protected `.dbbx` bundle at `path`
// (chosen via the native save dialog). `ids` names which to include; omit it
// to export the whole store. An empty array is rejected by the backend rather
// than read as "all" — the two readings are opposites.
export const exportConnections = (
  path: string,
  passphrase: string,
  ids?: string[],
): Promise<ExportReport> =>
  invoke('export_connections', { path, passphrase, ids });

// Decrypt and merge a `.dbbx` bundle at `path` into the local store. With
// `overwrite`, an incoming id that already exists replaces it; without, that
// entry is skipped and reported.
export const importConnections = (
  path: string,
  passphrase: string,
  overwrite = false,
): Promise<ImportReport> =>
  invoke('import_connections', { path, passphrase, overwrite });

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

// --- UI language (ADR-0041) ---------------------------------------------
//
// The chosen language lives in `ui-settings.toml`, not only in localStorage,
// so an MCP client can read it and switch it while the window is open. The
// shell watches that file and emits `ui:locale` when it changes.

// The persisted UI language and the codes the build ships. `locale` is null
// when nothing has been chosen — the caller then falls back to the OS
// language, so null is a state, not a missing value.
export interface UiLocale {
  locale: string | null;
  supported: string[];
}

export const getUiLocale = (): Promise<UiLocale> => invoke('get_ui_locale');

// Persist the UI language. Rejects on a code this build cannot display, so a
// typo surfaces instead of leaving the window in a language nobody asked for.
export const setUiLocale = (locale: string): Promise<void> =>
  invoke('set_ui_locale', { locale });

// Subscribe to language changes made outside this window (an MCP client, or a
// hand edit of `ui-settings.toml`). A null payload means the choice was
// cleared and the OS language applies again.
export const onUiLocale = (
  handler: (locale: string | null) => void,
): Promise<UnlistenFn> =>
  listen<string | null>('ui:locale', (event) => handler(event.payload));

// --- UI commands (ADR-0109) ---------------------------------------------
//
// The language channel above lets an agent *change a setting*. This one lets
// it work the window: type into the editor, run what is there, open the AI
// panel. The shell watches `ui-command.toml` and emits `ui:command`; whoever
// carried the instruction out answers with `reportUiCommandResult`, and the
// caller is blocked until that answer arrives.

/** An instruction from an MCP client. The verbs the shell can send. */
export type UiCommand =
  | { kind: 'set_editor_sql'; sql: string }
  | { kind: 'run_query' }
  | { kind: 'open_ai_panel' }
  | { kind: 'open_ai_settings' };

/** A command and the number its answer must carry back. */
export interface UiCommandEvent {
  seq: number;
  command: UiCommand;
}

export const onUiCommand = (
  handler: (event: UiCommandEvent) => void,
): Promise<UnlistenFn> =>
  listen<UiCommandEvent>('ui:command', (event) => handler(event.payload));

// Answer one command. Called when the work has *finished* — reporting on
// start would let an agent read the previous result as this one's.
export const reportUiCommandResult = (
  seq: number,
  ok: boolean,
  error: string | null,
  detail: string | null,
): Promise<void> =>
  invoke('report_ui_command_result', { seq, ok, error, detail });

// --- Auto-update (ADR-0067) ---------------------------------------------
//
// The updater plugin fetches the signed `latest.json` from the GitHub release,
// verifies the minisign signature against the embedded pubkey, then downloads
// and installs the newer bundle. The egui client only *informs* (ADR-0040);
// here we go one step further and install in-place, then relaunch.
//
// `check()` returns a stateful `Update` handle that `downloadAndInstall` must
// act on, so we stash it module-side rather than surfacing the plugin type to
// the components — they see only our flat `AvailableUpdate` DTO.

let pendingUpdate: Update | null = null;

// Whether the startup check is disabled via DBBOARD_NO_UPDATE_CHECK (opt-out
// parity with the egui client, ADR-0040). Cheap; call once before checking.
export const updateOptOut = (): Promise<boolean> => invoke('update_opt_out');

// Check the release endpoint for a newer signed bundle. Resolves to the mapped
// update (version + notes) or null when already current. Applies the same
// strictly-newer guard the egui client uses, so a same/older endpoint entry
// never surfaces a phantom notice. Best-effort: a transport/verify failure
// rejects and the caller swallows it (an update check must never break launch).
export const checkForUpdate = async (): Promise<AvailableUpdate | null> => {
  const update = await check();
  pendingUpdate = update;
  if (!update) return null;
  // Defensive: the plugin already gates on version, but a misconfigured
  // endpoint could still hand back a non-newer build — never nag for one.
  const { isNewer } = await import('$lib/update/notice');
  if (!isNewer(update.currentVersion, update.version)) {
    pendingUpdate = null;
    return null;
  }
  return {
    version: update.version,
    currentVersion: update.currentVersion,
    notes: update.body ?? '',
    date: update.date ?? null,
  };
};

// Download and install the update found by the last `checkForUpdate`, reporting
// progress through `onEvent`. Rejects if no update is pending or the transfer
// fails. On success the app is installed but still running — call `relaunchApp`.
export const installUpdate = async (
  onEvent: (event: DownloadEvent) => void,
): Promise<void> => {
  if (!pendingUpdate) throw new Error('no pending update to install');
  await pendingUpdate.downloadAndInstall((event) =>
    onEvent(event as DownloadEvent),
  );
};

// Relaunch the app so the freshly installed bundle takes over (ADR-0067).
export const relaunchApp = (): Promise<void> => relaunch();
