<script lang="ts">
  // The Structure tab of a Redis-compatible connection (ADR-0166). A key
  // prefix has no columns to describe, so the tab lists the prefix's keys a
  // page at a time and shows the picked key's value in a layout for its type.
  //
  // Every command goes through the same read-only path a typed query does;
  // the commands themselves are built in `$lib/redis/keys`.
  import { runReadQuery, displayCell } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { workspace } from '$lib/state/workspace.svelte';
  import {
    PAGE,
    appendKeys,
    keyPage,
    metaCommands,
    scanCommand,
    ttlText,
    valueCommand,
    valueView,
    type KeyType,
    type ValueView,
  } from '$lib/redis/keys';

  /** Generous enough for one page of anything the browser asks for. */
  const MAX_ROWS = 1000;

  let keys = $state<string[]>([]);
  let next = $state<string | null>(null);
  let listing = $state(false);
  let listError = $state('');

  let picked = $state<string | null>(null);
  let type = $state<KeyType | null>(null);
  let ttl = $state<number | null>(null);
  let view = $state<ValueView | null>(null);
  let reading = $state(false);
  let valueError = $state('');

  const pattern = $derived(workspace.selectedTable?.name ?? null);

  let listSeq = 0;
  let valueSeq = 0;

  // A new prefix starts a new walk.
  $effect(() => {
    const p = pattern;
    keys = [];
    next = null;
    picked = null;
    view = null;
    listError = '';
    if (p) void loadPage(p, '0');
  });

  function run(command: string) {
    return runReadQuery(workspace.connectionId, command, MAX_ROWS, workspace.database);
  }

  async function loadPage(p: string, cursor: string) {
    const mine = ++listSeq;
    listing = true;
    try {
      const page = keyPage(await run(scanCommand(p, cursor)));
      if (mine !== listSeq) return;
      keys = appendKeys(keys, page.keys);
      next = page.next;
    } catch (e) {
      if (mine === listSeq) listError = String(e);
    } finally {
      if (mine === listSeq) listing = false;
    }
  }

  async function pick(key: string) {
    const mine = ++valueSeq;
    picked = key;
    type = null;
    ttl = null;
    view = null;
    valueError = '';
    reading = true;
    try {
      const meta = metaCommands(key);
      const [t, life] = await Promise.all([run(meta.type), run(meta.ttl)]);
      if (mine !== valueSeq) return;
      type = displayCell(t.rows[0]?.[0] ?? null);
      const seconds = life.rows[0]?.[0];
      ttl = typeof seconds === 'number' ? seconds : null;
      const command = valueCommand(key, type);
      if (command) {
        const result = await run(command);
        if (mine === valueSeq) view = valueView(type, result);
      }
    } catch (e) {
      if (mine === valueSeq) valueError = String(e);
    } finally {
      if (mine === valueSeq) reading = false;
    }
  }

  function life(seconds: number): string {
    const t = ttlText(seconds);
    return i18n.t(t.key, t.value ? { value: t.value } : undefined);
  }
</script>

{#if !pattern}
  <p class="muted pad">{i18n.t('redis-keys-pick-prefix')}</p>
{:else}
  <div class="panel">
    <section class="keys" aria-label={i18n.t('redis-keys-title', { pattern })}>
      <h2 class="heading mono">{i18n.t('redis-keys-title', { pattern })}</h2>
      {#if listError}
        <p class="error">{listError}</p>
      {/if}
      <ul class="list">
        {#each keys as key (key)}
          <li>
            <button
              type="button"
              class="key mono"
              class:selected={key === picked}
              aria-current={key === picked ? 'true' : undefined}
              onclick={() => pick(key)}
              title={key}
            >
              {key}
            </button>
          </li>
        {/each}
      </ul>
      {#if listing}
        <p class="muted">{i18n.t('redis-loading')}</p>
      {:else if keys.length === 0 && !next && !listError}
        <p class="muted">{i18n.t('redis-keys-empty', { pattern })}</p>
      {/if}
      {#if next && !listing}
        <button type="button" class="ghost more" onclick={() => pattern && next && loadPage(pattern, next)}>
          {i18n.t('redis-keys-more')}
        </button>
      {/if}
    </section>

    <section class="value">
      {#if !picked}
        <p class="muted">{i18n.t('redis-keys-pick')}</p>
      {:else}
        <h2 class="heading mono key-name">{picked}</h2>
        <dl class="meta">
          <dt>{i18n.t('redis-key-type')}</dt>
          <dd class="mono">{type ?? '—'}</dd>
          <dt>{i18n.t('redis-key-ttl')}</dt>
          <dd>{ttl === null ? '—' : life(ttl)}</dd>
        </dl>
        {#if valueError}
          <p class="error">{valueError}</p>
        {:else if reading}
          <p class="muted">{i18n.t('redis-loading')}</p>
        {:else if view?.kind === 'text'}
          <pre class="text mono">{view.text}</pre>
        {:else if view?.kind === 'table'}
          <div class="scroll">
            <table class="grid">
              <thead>
                <tr>{#each view.columns as c (c)}<th>{c}</th>{/each}</tr>
              </thead>
              <tbody>
                {#each view.rows as row, r (r)}
                  <tr>{#each row as cell, i (i)}<td class="mono">{cell}</td>{/each}</tr>
                {/each}
              </tbody>
            </table>
          </div>
          {#if view.rows.length >= PAGE}
            <p class="muted">{i18n.t('redis-value-first-page', { count: PAGE })}</p>
          {/if}
        {:else if type}
          <p class="muted">{i18n.t('redis-value-unreadable', { type })}</p>
        {/if}
      {/if}
    </section>
  </div>
{/if}

<style>
  .panel {
    display: flex;
    height: 100%;
    min-height: 0;
  }
  .pad {
    padding: var(--space-4);
  }
  .keys {
    flex: none;
    width: 320px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
    padding: var(--space-3);
    gap: var(--space-2);
  }
  .heading {
    margin: 0;
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .key-name {
    font-size: var(--text-heading);
    color: var(--text);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .key {
    width: 100%;
    text-align: left;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    font-size: var(--text-small);
    padding: 4px 8px;
    border-radius: var(--radius-widget);
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .key:hover {
    background: var(--bg-surface-alt);
  }
  .key.selected {
    color: var(--text-accent);
    background: var(--accent-weak);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .ghost {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-small);
    padding: 4px 10px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .ghost:hover {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .more {
    align-self: flex-start;
  }
  .value {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
  }
  .meta {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px var(--space-3);
    margin: 0;
    font-size: var(--text-small);
  }
  .meta dt {
    color: var(--text-muted);
  }
  .meta dd {
    margin: 0;
  }
  .text {
    margin: 0;
    padding: var(--space-3);
    background: var(--bg-code);
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    overflow: auto;
    font-size: var(--text-small);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
  }
  .grid {
    border-collapse: collapse;
    width: 100%;
    font-size: var(--text-small);
  }
  th,
  td {
    text-align: left;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--bg-surface-alt);
    color: var(--text-muted);
    font-weight: 600;
  }
  td {
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .muted {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
  }
</style>
