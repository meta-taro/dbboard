// Changes agents leave on their branches for a person to approve (ADR-0176).
// Shapes mirror `dbboard_core::PendingChange` and the service's
// `ReviewAnswer`; the logic here decides what the screen does with an answer.
import type { MessageKey } from '$lib/i18n/messages';

export interface TableChangeCount {
  table: string;
  changed: number;
}

export interface PendingChange {
  /** Sent back with a decision; never shown. */
  branch: string;
  /** The commit the person is looking at. */
  head: string;
  /** Where the branch left `main`. `null` when it shares nothing with it. */
  base: string | null;
  authors: string[];
  why: string;
  first_at: string;
  last_at: string;
  tables: TableChangeCount[];
  main_moved: boolean;
  conflicts: boolean;
}

export interface PendingView {
  connection_id: string;
  available: boolean;
  changes: PendingChange[];
}

export type ReviewRefusal =
  | { kind: 'moved' }
  | { kind: 'decided' }
  | { kind: 'not_waiting' }
  | { kind: 'conflict'; keys: string[]; more: number }
  | { kind: 'no_permission'; message: string }
  | { kind: 'no_auth' };

export type ReviewAnswer =
  | { outcome: 'approved'; merge: string }
  | { outcome: 'rejected' }
  | { outcome: 'refused'; refusal: ReviewRefusal };

/** What the screen does after a refusal. */
export type FollowUp =
  /** Read the change's diff again: the agent wrote more. */
  | 'reload-diff'
  /** Read the whole list again. */
  | 'reload-list'
  /** Keep everything as it is; the message says what to fix. */
  | 'stay';

export interface RefusalPlan {
  messageKey: MessageKey;
  followUp: FollowUp;
}

/** Each refusal leads somewhere different (ADR-0176's table). */
export function planFor(refusal: ReviewRefusal): RefusalPlan {
  switch (refusal.kind) {
    case 'moved':
      return { messageKey: 'review-refused-moved', followUp: 'reload-diff' };
    case 'decided':
      return { messageKey: 'review-refused-decided', followUp: 'reload-list' };
    case 'not_waiting':
      return { messageKey: 'review-refused-not-waiting', followUp: 'reload-list' };
    case 'conflict':
      return { messageKey: 'review-refused-conflict', followUp: 'stay' };
    case 'no_permission':
      return { messageKey: 'review-refused-no-permission', followUp: 'stay' };
    case 'no_auth':
      return { messageKey: 'review-refused-no-auth', followUp: 'stay' };
  }
}

/** The key pattern the history views read a table by (ADR-0170). */
export function tablePattern(table: string): string {
  return `${table}/*`;
}

/** The total number of keys a change touches, for the list. */
export function changedKeys(change: PendingChange): number {
  return change.tables.reduce((sum, t) => sum + t.changed, 0);
}
