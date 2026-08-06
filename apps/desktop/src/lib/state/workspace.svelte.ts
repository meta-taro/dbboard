// Workspace state shared across the shell: which connection is selected, its
// tables, which table is focused, and the active main-area tab. The sidebar
// and the tab panels all read and drive this one instance, so selecting a
// table in the sidebar and reading it in the Structure tab need no prop
// threading.
//
// Query editor state (the SQL text, the last result) stays local to the Query
// panel — it is specific to that view and preserved by keeping the panel
// mounted, not by living here.
import {
  listConnections,
  listTables,
  reconnectConnection,
  tableKey,
  type ConnectionView,
  type TableInfo,
} from '$lib/api';
import { BROWSE_ROWS } from '$lib/sidebar/menu';
import { dialectForKind, selectTopN } from '$lib/sql/build';

export type MainTab = 'query' | 'structure';

class Workspace {
  connections = $state<ConnectionView[]>([]);
  connectionId = $state('');
  tables = $state<TableInfo[]>([]);
  selectedTable = $state<TableInfo | null>(null);
  activeTab = $state<MainTab>('query');

  /** Populated whenever a load fails; surfaced by the shell, cleared on the
   *  next successful action. */
  error = $state('');
  loadingTables = $state(false);
  reconnecting = $state(false);

  /** A request to load SQL into the query editor and run it — raised by the
   *  sidebar context menu ("Select top 100"), consumed by the Query panel.
   *  The seq lets the panel apply each request exactly once, even when the
   *  same SQL text is requested twice in a row. `table`, when present, marks
   *  the request as an editable browse of that table: the panel loads its
   *  primary key so the result grid can offer inline editing (ADR-0042). */
  queryRequest = $state<{ sql: string; seq: number; table?: TableInfo } | null>(
    null,
  );

  /** The currently selected connection, or undefined if none. */
  get connection(): ConnectionView | undefined {
    return this.connections.find((c) => c.id === this.connectionId);
  }

  /** Load connections and auto-select the first, then its tables. */
  async init(): Promise<void> {
    try {
      this.connections = await listConnections();
      if (this.connections.length > 0) {
        await this.selectConnection(this.connections[0].id);
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  /** Reload the connection list after an add/edit/delete. Preserves the
   *  current selection when it survives (re-selecting it so changed
   *  credentials take effect); otherwise falls back to the first connection,
   *  or clears everything when none remain. */
  async refreshConnections(): Promise<void> {
    try {
      this.connections = await listConnections();
      const survived = this.connections.some((c) => c.id === this.connectionId);
      if (survived) {
        await this.selectConnection(this.connectionId);
      } else if (this.connections.length > 0) {
        await this.selectConnection(this.connections[0].id);
      } else {
        this.connectionId = '';
        this.selectedTable = null;
        this.tables = [];
      }
    } catch (e) {
      this.error = String(e);
    }
  }

  async selectConnection(id: string): Promise<void> {
    this.connectionId = id;
    this.selectedTable = null;
    this.tables = [];
    this.error = '';
    await this.#refreshTables();
  }

  /** Throw away the live connection and open a new one, then reload the table
   *  list.
   *
   *  Needed because a connection can die in a way this process cannot see:
   *  through an SSH bastion the tunnel dies while the adapter holding it stays
   *  cached, and every call then fails with `expected to read 4 bytes, got 0
   *  bytes at EOF` until the app is restarted. The backend now heals that on
   *  its own once the connection has been idle a while, so this button is
   *  about not having to wait — and about not having to guess whether
   *  clicking again would have worked. */
  async reconnect(): Promise<void> {
    if (!this.connectionId || this.reconnecting) return;
    this.reconnecting = true;
    try {
      await reconnectConnection(this.connectionId);
      this.error = '';
      await this.#refreshTables();
    } catch (e) {
      this.error = String(e);
    } finally {
      this.reconnecting = false;
    }
  }

  /** Focus a table and jump to the Structure tab — clicking a table in the
   *  sidebar is a request to inspect it. */
  selectTable(table: TableInfo): void {
    this.selectedTable = table;
    this.activeTab = 'structure';
  }

  setTab(tab: MainTab): void {
    this.activeTab = tab;
  }

  /** Load SQL into the Query editor and switch to it; the panel runs it. Used
   *  for arbitrary generated SQL that is not tied to one editable table. */
  runInEditor(sql: string): void {
    const seq = (this.queryRequest?.seq ?? 0) + 1;
    this.queryRequest = { sql, seq };
    this.activeTab = 'query';
  }

  /** Browse a table's first rows in the Query editor as an *editable* result:
   *  a bounded `SELECT *` plus the table identity, so the panel can load the
   *  primary key and the grid can offer inline cell editing (ADR-0042). */
  browse(table: TableInfo): void {
    const seq = (this.queryRequest?.seq ?? 0) + 1;
    const sql = selectTopN(table, BROWSE_ROWS, dialectForKind(this.connection?.kind));
    this.queryRequest = { sql, seq, table };
    this.activeTab = 'query';
  }

  /** Stable key for a table, used for list keying and equality. */
  key(table: TableInfo): string {
    return tableKey(table);
  }

  async #refreshTables(): Promise<void> {
    if (!this.connectionId) return;
    this.loadingTables = true;
    try {
      this.tables = await listTables(this.connectionId);
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loadingTables = false;
    }
  }
}

export const workspace = new Workspace();
