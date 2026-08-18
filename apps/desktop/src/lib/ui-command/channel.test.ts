import { describe, it, expect, vi } from 'vitest';
import { attachUiCommands, type UiCommandChannel } from './channel';
import type { UiCommand, UiCommandEvent } from '$lib/api';

type Report = [number, boolean, string | null, string | null];

/** A channel whose subscription we can fire by hand. */
function harness(dispatch: UiCommandChannel['dispatch']) {
  let emit: ((event: UiCommandEvent) => void) | null = null;
  const reports: Report[] = [];
  const channel: UiCommandChannel = {
    subscribe: async (handler) => {
      emit = handler;
      return () => {
        emit = null;
      };
    },
    dispatch,
    report: async (seq, ok, error, detail) => {
      reports.push([seq, ok, error, detail]);
    },
  };
  return {
    channel,
    reports,
    send(seq: number, command: UiCommand) {
      emit?.({ seq, command });
    },
  };
}

/** Let the answer's promise chain settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('attachUiCommands', () => {
  it('answers a command it carried out, with the number it came in on', async () => {
    const h = harness(async () => '3 rows');
    await attachUiCommands(h.channel);
    h.send(7, { kind: 'run_query' });
    await settle();
    expect(h.reports).toEqual([[7, true, null, '3 rows']]);
  });

  it('answers a refusal with its reason rather than staying silent', async () => {
    // Silence here is the expensive failure: the caller waits the full
    // timeout and is told the app is not running, which sends whoever is
    // debugging it to the wrong place entirely.
    const h = harness(async () => {
      throw new Error('no connection is selected');
    });
    await attachUiCommands(h.channel);
    h.send(2, { kind: 'run_query' });
    await settle();
    expect(h.reports).toEqual([[2, false, 'no connection is selected', null]]);
  });

  it('still answers when a handler throws something that is not an Error', async () => {
    const h = harness(async () => {
      throw 'plain string';
    });
    await attachUiCommands(h.channel);
    h.send(1, { kind: 'open_ai_panel' });
    await settle();
    expect(h.reports).toEqual([[1, false, 'plain string', null]]);
  });

  it('answers every command, one each', async () => {
    const h = harness(async (c) => c.kind);
    await attachUiCommands(h.channel);
    h.send(1, { kind: 'run_query' });
    h.send(2, { kind: 'open_ai_panel' });
    await settle();
    expect(h.reports).toEqual([
      [1, true, null, 'run_query'],
      [2, true, null, 'open_ai_panel'],
    ]);
  });

  it('survives a report that fails', async () => {
    const channel: UiCommandChannel = {
      subscribe: async (handler) => {
        handler({ seq: 1, command: { kind: 'run_query' } });
        return () => {};
      },
      dispatch: async () => 'done',
      report: async () => {
        throw new Error('the shell went away');
      },
    };
    await expect(attachUiCommands(channel)).resolves.toBeTypeOf('function');
    await settle();
  });

  it('is a no-op outside Tauri rather than a broken page', async () => {
    // `pnpm dev` in a plain browser has no event source. The window must
    // still paint; it simply obeys nobody.
    const channel: UiCommandChannel = {
      subscribe: async () => {
        throw new Error('no Tauri IPC here');
      },
      dispatch: vi.fn(),
      report: vi.fn(),
    };
    await expect(attachUiCommands(channel)).resolves.toBeNull();
    expect(channel.dispatch).not.toHaveBeenCalled();
  });

  it('hands back the detach the subscription gave it', async () => {
    const detach = vi.fn();
    const channel: UiCommandChannel = {
      subscribe: async () => detach,
      dispatch: async () => null,
      report: async () => {},
    };
    const off = await attachUiCommands(channel);
    off?.();
    expect(detach).toHaveBeenCalledOnce();
  });
});
