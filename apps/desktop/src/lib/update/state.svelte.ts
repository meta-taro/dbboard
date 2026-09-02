// Whether a newer release is waiting, shared between the startup check that
// finds it, the notice card that offers it, and the status bar.
//
// Dismissing the card used to throw the update away for the rest of the
// session, which left no way back to it short of restarting the app. Now
// dismissing only hides the card: the availability itself survives, and the
// status bar keeps a quiet chip that brings the card back.
import type { AvailableUpdate, StalledUpdate } from './notice';

class UpdateState {
  /** The newer release, or null when there is none (or the check failed). */
  available = $state<AvailableUpdate | null>(null);

  /** The card has been closed. Kept apart from `available` so closing it is
   *  not the same as deciding there is no update. */
  dismissed = $state(false);

  /** An update that was started last run and did not land, reported once
   *  by the backend at startup. Separate from `available`: the card has
   *  something to say even when the check finds nothing new (offline, or
   *  the same release already offered). */
  stalled = $state<StalledUpdate | null>(null);

  get showNotice(): boolean {
    return (this.available !== null || this.stalled !== null) && !this.dismissed;
  }

  /** Record the result of an update check. */
  set(update: AvailableUpdate | null): void {
    this.available = update;
    this.dismissed = false;
  }

  /** Record a previous run's update that never completed. */
  setStalled(stalled: StalledUpdate | null): void {
    this.stalled = stalled;
    if (stalled !== null) this.dismissed = false;
  }

  dismiss(): void {
    this.dismissed = true;
  }

  /** Bring the dismissed card back — the status bar chip. */
  reopen(): void {
    this.dismissed = false;
  }
}

export const updateState = new UpdateState();
