// Whether the connection in view keeps history, and its newest commits
// (ADR-0165). Read by the shell, which shows the History tab only when it
// does, and by the panel, which lists the commits.
import { commitLog } from '$lib/api';
import type { Commit, CommitLogView } from './view';

type FetchLog = (connectionId: string, database: string | null) => Promise<CommitLogView>;

export class VersionedState {
  available = $state(false);
  commits = $state<Commit[]>([]);
  loading = $state(false);
  /** A failed refresh of a log that was already showing. The first check
   *  never sets this: detection failing means "no history", not a banner. */
  error = $state('');

  #fetch: FetchLog;
  #seq = 0;

  constructor(fetch: FetchLog = (id, db) => commitLog(id, db)) {
    this.#fetch = fetch;
  }

  /** Ask a connection (and database within it) whether it keeps history. */
  async load(connectionId: string, database: string | null): Promise<void> {
    const seq = ++this.#seq;
    // Drop the previous connection's answer at once: its commits drawn under
    // the next connection's name would be a wrong answer, not a stale one.
    this.available = false;
    this.commits = [];
    this.error = '';
    if (!connectionId) return;
    this.loading = true;
    try {
      const view = await this.#fetch(connectionId, database);
      if (seq !== this.#seq) return;
      this.available = view.available;
      this.commits = view.commits;
    } catch {
      if (seq !== this.#seq) return;
      this.available = false;
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }

  /** Re-read the log of a connection already known to keep history, after
   *  someone has committed to it from elsewhere. */
  async refresh(connectionId: string, database: string | null): Promise<void> {
    const seq = ++this.#seq;
    this.loading = true;
    try {
      const view = await this.#fetch(connectionId, database);
      if (seq !== this.#seq) return;
      this.available = view.available;
      this.commits = view.commits;
      this.error = '';
    } catch (e) {
      if (seq !== this.#seq) return;
      this.error = String(e);
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }
}

export const versioned = new VersionedState();
