/**
 * Routing for the instructions an MCP client sends this window (ADR-0109).
 *
 * The verbs land in different places: the query panel owns the editor and the
 * Run button, the shell owns the AI panel. A single listener in the shell that
 * reached into both would have to know their internals, so instead each part
 * claims the verbs it can carry out and the listener just forwards.
 *
 * Handlers are async and answer with a short detail — or throw, which becomes
 * the reason the agent is told the window refused. Both are reported only when
 * the work has finished; see `reportUiCommandResult`.
 */
import type { UiCommand } from '$lib/api';

/** What a verb's handler must be: do the work, then say what happened. */
export type UiCommandHandler<K extends UiCommand['kind']> = (
  command: Extract<UiCommand, { kind: K }>,
) => Promise<string | null>;

type AnyHandler = (command: UiCommand) => Promise<string | null>;

export class UiCommandBus {
  #handlers = new Map<UiCommand['kind'], AnyHandler>();

  /**
   * Claim a verb. Returns the function that gives it up again — which removes
   * *this* handler only, never whichever one is registered by then. A
   * component that remounts registers before its old cleanup runs, and a
   * blind `delete` there would leave the window deaf to the verb with nothing
   * to show for it.
   */
  on<K extends UiCommand['kind']>(
    kind: K,
    handler: UiCommandHandler<K>,
  ): () => void {
    const entry = handler as AnyHandler;
    this.#handlers.set(kind, entry);
    return () => {
      if (this.#handlers.get(kind) === entry) this.#handlers.delete(kind);
    };
  }

  /** Carry out one command, or reject with the reason it could not be. */
  async dispatch(command: UiCommand): Promise<string | null> {
    const handler = this.#handlers.get(command.kind);
    if (!handler) {
      throw new Error(
        `this dbboard window cannot do "${command.kind}" — the part that ` +
          `handles it is not open`,
      );
    }
    return await handler(command);
  }
}

/** The window's bus. One per process; the shell forwards `ui:command` here. */
export const uiCommands = new UiCommandBus();
