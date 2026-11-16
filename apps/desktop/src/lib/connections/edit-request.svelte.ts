// A request, from anywhere in the window, to open one connection's edit form.
// The connection manager is owned by the sidebar; the error banner that
// needs it (#331) is not, so the two meet here rather than through props.

class EditRequest {
  /** The connection whose form should open, or null when nothing is asked. */
  id = $state<string | null>(null);

  open(id: string): void {
    this.id = id;
  }

  /** Take the request, so it is acted on once. */
  take(): string | null {
    const id = this.id;
    this.id = null;
    return id;
  }
}

export const editRequest = new EditRequest();
