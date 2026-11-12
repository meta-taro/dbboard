// The Review tab's state (ADR-0176): the waiting changes on the connection in
// view, the one the person picked, its diff table by table, and the outcome
// of their last decision.
import { approveChange, commitDiff, pendingChanges, rejectChange } from '$lib/api';
import type { CommitDiff } from '$lib/versioned/view';
import {
  planFor,
  tablePattern,
  type PendingChange,
  type PendingView,
  type RefusalPlan,
  type ReviewAnswer,
  type ReviewRefusal,
} from './view';

export interface ReviewBackend {
  pending(connectionId: string): Promise<PendingView>;
  diff(connectionId: string, table: string, from: string, to: string): Promise<CommitDiff>;
  approve(connectionId: string, branch: string, head: string, reason: string): Promise<ReviewAnswer>;
  reject(connectionId: string, branch: string, head: string, reason: string): Promise<ReviewAnswer>;
}

const tauriBackend: ReviewBackend = {
  pending: (id) => pendingChanges(id),
  diff: (id, table, from, to) => commitDiff(id, { schema: null, name: tablePattern(table) }, from, to),
  approve: (id, branch, head, reason) => approveChange(id, branch, head, reason),
  reject: (id, branch, head, reason) => rejectChange(id, branch, head, reason),
};

/** Umbrachyx's name for the state before any commit. */
const EMPTY = '-';

export interface TableDiff {
  table: string;
  diff: CommitDiff | null;
  error: string;
}

/** The last decision's outcome, as the screen shows it. */
export type Outcome =
  | { kind: 'approved'; merge: string }
  | { kind: 'rejected' }
  | { kind: 'refused'; refusal: ReviewRefusal; plan: RefusalPlan }
  | { kind: 'failed'; message: string };

export class ReviewState {
  available = $state(false);
  changes = $state<PendingChange[]>([]);
  loading = $state(false);
  error = $state('');
  selected = $state<PendingChange | null>(null);
  diffs = $state<TableDiff[]>([]);
  diffLoading = $state(false);
  deciding = $state(false);
  outcome = $state<Outcome | null>(null);

  #backend: ReviewBackend;
  #connectionId = '';
  #seq = 0;

  constructor(backend: ReviewBackend = tauriBackend) {
    this.#backend = backend;
  }

  /** Ask the connection in view whether it keeps a review queue. */
  async load(connectionId: string): Promise<void> {
    const seq = ++this.#seq;
    this.#connectionId = connectionId;
    this.available = false;
    this.changes = [];
    this.selected = null;
    this.diffs = [];
    this.outcome = null;
    this.error = '';
    if (!connectionId) return;
    this.loading = true;
    try {
      const view = await this.#backend.pending(connectionId);
      if (seq !== this.#seq) return;
      this.available = view.available;
      this.changes = view.changes;
    } catch {
      // Detection failing means "no queue", as with the history views.
      if (seq !== this.#seq) return;
      this.available = false;
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }

  /** Read the list again, keeping the picked change if it is still waiting. */
  async refresh(): Promise<void> {
    const seq = ++this.#seq;
    this.loading = true;
    try {
      const view = await this.#backend.pending(this.#connectionId);
      if (seq !== this.#seq) return;
      this.changes = view.changes;
      this.error = '';
      const kept = this.changes.find((c) => c.branch === this.selected?.branch) ?? null;
      if (!kept) {
        this.selected = null;
        this.diffs = [];
      } else if (kept.head !== this.selected?.head) {
        await this.select(kept);
      }
    } catch (e) {
      if (seq !== this.#seq) return;
      this.error = String(e);
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }

  /** Show one change: its diff, table by table, from its base to its head. */
  async select(change: PendingChange): Promise<void> {
    this.selected = change;
    this.outcome = null;
    this.diffs = [];
    // A branch that shares no commit with `main` has no base; Umbrachyx's
    // `-` names the empty state, so everything on the branch reads as added.
    const base = change.base ?? EMPTY;
    this.diffLoading = true;
    const results = await Promise.all(
      change.tables.map(async ({ table }): Promise<TableDiff> => {
        try {
          const diff = await this.#backend.diff(this.#connectionId, table, base, change.head);
          return { table, diff, error: '' };
        } catch (e) {
          return { table, diff: null, error: String(e) };
        }
      }),
    );
    if (this.selected?.head !== change.head) return;
    this.diffs = results;
    this.diffLoading = false;
  }

  /** Approve or reject the change in view, with the person's reason. */
  async decide(verb: 'approve' | 'reject', reason: string): Promise<void> {
    const change = this.selected;
    if (!change || this.deciding) return;
    this.deciding = true;
    try {
      const send = verb === 'approve' ? this.#backend.approve : this.#backend.reject;
      const answer = await send(this.#connectionId, change.branch, change.head, reason);
      await this.#settle(answer);
    } catch (e) {
      this.outcome = { kind: 'failed', message: String(e) };
    } finally {
      this.deciding = false;
    }
  }

  async #settle(answer: ReviewAnswer): Promise<void> {
    if (answer.outcome === 'approved' || answer.outcome === 'rejected') {
      const outcome: Outcome =
        answer.outcome === 'approved'
          ? { kind: 'approved', merge: answer.merge }
          : { kind: 'rejected' };
      this.selected = null;
      this.diffs = [];
      await this.refresh();
      this.outcome = outcome;
      return;
    }
    const plan = planFor(answer.refusal);
    const outcome: Outcome = { kind: 'refused', refusal: answer.refusal, plan };
    if (plan.followUp === 'reload-diff' || plan.followUp === 'reload-list') {
      await this.refresh();
    }
    this.outcome = outcome;
  }
}

export const review = new ReviewState();
