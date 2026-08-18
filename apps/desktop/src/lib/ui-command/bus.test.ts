import { describe, it, expect } from 'vitest';
import { UiCommandBus } from './bus';

describe('UiCommandBus', () => {
  it('hands a command to the part of the window that claimed it', async () => {
    const bus = new UiCommandBus();
    const seen: string[] = [];
    bus.on('set_editor_sql', async (c) => {
      seen.push(c.sql);
      return 'editor updated';
    });

    const detail = await bus.dispatch({
      kind: 'set_editor_sql',
      sql: "SELECT '日本語';",
    });

    expect(seen).toEqual(["SELECT '日本語';"]);
    expect(detail).toBe('editor updated');
  });

  it('refuses a verb nobody claimed, naming it', async () => {
    // Not theoretical: the AI panel and the query panel register separately,
    // so a build where one of them failed to mount would otherwise leave the
    // caller waiting the full timeout for an answer that is never coming.
    const bus = new UiCommandBus();
    await expect(bus.dispatch({ kind: 'run_query' })).rejects.toThrow(
      /run_query/,
    );
  });

  it('passes a handler failure through as the refusal reason', async () => {
    const bus = new UiCommandBus();
    bus.on('run_query', async () => {
      throw new Error('no connection is selected');
    });
    await expect(bus.dispatch({ kind: 'run_query' })).rejects.toThrow(
      'no connection is selected',
    );
  });

  it('lets a handler answer with no detail', async () => {
    const bus = new UiCommandBus();
    bus.on('open_ai_panel', async () => null);
    await expect(bus.dispatch({ kind: 'open_ai_panel' })).resolves.toBeNull();
  });

  it('keeps the verbs apart', async () => {
    const bus = new UiCommandBus();
    bus.on('run_query', async () => 'ran');
    bus.on('open_ai_panel', async () => 'opened');
    expect(await bus.dispatch({ kind: 'run_query' })).toBe('ran');
    expect(await bus.dispatch({ kind: 'open_ai_panel' })).toBe('opened');
  });

  it('sends the command to the handler that is currently registered', async () => {
    const bus = new UiCommandBus();
    bus.on('run_query', async () => 'first');
    bus.on('run_query', async () => 'second');
    expect(await bus.dispatch({ kind: 'run_query' })).toBe('second');
  });

  it('an unregister only removes its own handler', async () => {
    // A component that re-registers on remount would otherwise tear down the
    // live handler when its previous cleanup ran, and the window would go
    // deaf to that verb with nothing in the log to say why.
    const bus = new UiCommandBus();
    const off = bus.on('run_query', async () => 'first');
    bus.on('run_query', async () => 'second');
    off();
    expect(await bus.dispatch({ kind: 'run_query' })).toBe('second');
  });

  it('stops calling a handler once it unregisters', async () => {
    const bus = new UiCommandBus();
    const off = bus.on('run_query', async () => 'ran');
    off();
    await expect(bus.dispatch({ kind: 'run_query' })).rejects.toThrow(
      /run_query/,
    );
  });
});
