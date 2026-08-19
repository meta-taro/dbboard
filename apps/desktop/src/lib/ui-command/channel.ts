/**
 * The window's end of the command channel (ADR-0109): take each `ui:command`,
 * hand it to whoever claimed the verb, and answer it.
 *
 * Written against injected functions rather than importing the Tauri bindings
 * so the rule that matters here — *every* command gets exactly one answer,
 * including the ones that fail — can be tested without a window.
 */
import type { UiCommand, UiCommandEvent } from '$lib/api';

/** Detaches the subscription. Mirrors Tauri's `UnlistenFn`. */
export type Detach = () => void;

export interface UiCommandChannel {
  subscribe: (handler: (event: UiCommandEvent) => void) => Promise<Detach>;
  dispatch: (command: UiCommand) => Promise<string | null>;
  report: (
    seq: number,
    ok: boolean,
    error: string | null,
    detail: string | null,
  ) => Promise<void>;
}

/** The message an agent is given when a handler throws something odd. */
function reason(error: unknown): string {
  if (error instanceof Error) return error.message;
  const text = String(error);
  return text === '' ? 'the window gave no reason' : text;
}

/**
 * Start answering commands. Resolves to the detach function, or `null` when
 * the subscription itself could not be set up — outside Tauri (a browser
 * `pnpm dev`) there is no event source, and that must not break the page.
 */
export async function attachUiCommands(
  channel: UiCommandChannel,
): Promise<Detach | null> {
  try {
    return await channel.subscribe((event) => {
      void answer(channel, event);
    });
  } catch {
    return null;
  }
}

async function answer(
  channel: UiCommandChannel,
  event: UiCommandEvent,
): Promise<void> {
  let ok = false;
  let detail: string | null = null;
  let error: string | null = null;
  try {
    detail = await channel.dispatch(event.command);
    ok = true;
  } catch (e) {
    error = reason(e);
  }
  // A failure to report is the one thing there is nowhere to report to. The
  // caller times out — which is the honest outcome — and swallowing it here
  // keeps an unanswerable command from taking the window's event loop with it.
  try {
    await channel.report(event.seq, ok, error, detail);
  } catch {
    /* nothing to do: the caller will time out */
  }
}
