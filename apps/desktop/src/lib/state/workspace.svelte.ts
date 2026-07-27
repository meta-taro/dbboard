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
  tableKey,
  type ConnectionView,
  type TableInfo,
} from '$lib/api';

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

  async selectConnection(id: string): Promise<void> {
    this.connectionId = id;
    this.selectedTable = null;
    this.tables = [];
    this.error = '';
    await this.#refreshTables();
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
