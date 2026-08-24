/** The two reads a mutating action has to redo, injected so they can be observed. */
export interface RefreshTargets {
  /** Re-read the connection list and re-select whatever survived. */
  connections: () => Promise<void>;
  /** Re-read which entries name another connection's secret slot. */
  foreignRefs: () => Promise<void>;
}

/**
 * Bring the connection panel back in step after something changed it.
 *
 * Adding, editing, deleting, duplicating, repairing and importing all invalidate
 * the same two reads, and every one of them used to spell the pair out at the
 * call site. Naming it is not only tidiness: the pair was factored out once
 * already and came back wrong (see `refresh.test.ts`), and a two-line helper
 * inside a 1,600-line component is a thing no test can reach.
 *
 * The list comes first and a failure there stops the badges — they annotate
 * rows, and decorating a list that did not load says the panel is current when
 * the caller is about to say it is not.
 */
export async function refreshConnectionList(targets: RefreshTargets): Promise<void> {
  await targets.connections();
  await targets.foreignRefs();
}
