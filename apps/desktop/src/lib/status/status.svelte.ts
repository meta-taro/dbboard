// How the last statement went, for the status bar at the bottom of the window.
//
// It lives outside the query panel because the bar is part of the app shell
// (`+layout.svelte`) and stays visible while you are on another tab — the one
// number here, how long the statement took, is measured nowhere else in the
// app and disappears the moment you look away from the result.
class RunStatus {
  /** A statement is in flight. */
  running = $state(false);

  /** The last finished statement, or null before the first one. */
  last = $state<{ elapsedMs: number; failed: boolean } | null>(null);

  #startedAt = 0;

  /** Mark the start of a statement and take the clock reading it will be
   *  measured against. */
  begin(): void {
    this.running = true;
    this.#startedAt = performance.now();
  }

  /** Mark the end of a statement. A failed one is still timed: a query that
   *  died after thirty seconds is a different problem from one that was
   *  rejected instantly.
   *
   *  Ignored when nothing is running, so a caller that stops the clock the
   *  moment the query returns and also has a `catch` around the bookkeeping
   *  after it cannot overwrite a good measurement with a later failure. */
  end(failed: boolean): void {
    if (!this.running) return;
    this.running = false;
    this.last = { elapsedMs: performance.now() - this.#startedAt, failed };
  }
}

export const runStatus = new RunStatus();
