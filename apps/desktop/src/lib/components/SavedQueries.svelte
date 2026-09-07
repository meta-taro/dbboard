<script lang="ts">
  // Saved queries (ADR-0147): the button, its popover, and the three calls
  // that back them. A component of its own rather than more of `QueryPanel`,
  // which is already near the 800-line ceiling and owns enough.
  import { deleteSavedQuery, listSavedQueries, saveQuery } from '$lib/api';
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { placePopover, type PopoverPlacement } from '$lib/layout/popover';
  import { byRecency, defaultQueryName, type SavedQueryView } from '$lib/queries/saved';

  interface Props {
    // Null while no connection is chosen: the button is disabled, because a
    // saved query belongs to the database whose tables it names.
    connectionId: string | null;
    // The editor's current text — what "Save" would keep.
    sql: string;
    // Put a saved statement back in the editor.
    onLoad: (sql: string) => void;
  }
  let { connectionId, sql, onLoad }: Props = $props();

  const POP_HEIGHT = 320;
  const POP_WIDTH = 460;

  let open = $state(false);
  let queries = $state<SavedQueryView[]>([]);
  let name = $state('');
  let error = $state('');
  // The name whose save was refused because it is taken. Set means the
  // popover is showing "replace it?" — the confirmation the backend demands
  // before it will overwrite (ADR-0147).
  let clashing = $state<string | null>(null);
  let busy = $state(false);

  let btn = $state<HTMLButtonElement | null>(null);
  let pop = $state<PopoverPlacement | null>(null);
  let width = $state(POP_WIDTH);

  const sorted = $derived(byRecency(queries));
  const canSave = $derived(sql.trim() !== '' && name.trim() !== '' && !busy);

  // Reload whenever the connection changes, so switching databases never
  // shows another one's queries — even for the instant before a fetch lands.
  $effect(() => {
    const id = connectionId;
    queries = [];
    if (!id) return;
    void listSavedQueries(id).then(
      (list) => {
        // Ignore a reply that arrived after the connection moved on.
        if (id === connectionId) queries = list;
      },
      () => {
        if (id === connectionId) error = i18n.t('saved-load-failed');
      },
    );
  });

  function place() {
    if (!btn) return;
    width = Math.min(POP_WIDTH, window.innerWidth * 0.6);
    pop = placePopover(
      btn.getBoundingClientRect(),
      { width: window.innerWidth, height: window.innerHeight },
      { width, preferredHeight: POP_HEIGHT },
    );
  }

  function toggle() {
    open = !open;
    if (open) {
      place();
      // Propose a name on every opening rather than once: the statement in
      // the editor is usually not the one it was last time.
      name = defaultQueryName(sql, queries);
      error = '';
      clashing = null;
    }
  }

  const style = $derived(
    pop === null
      ? ''
      : `left:${pop.left}px;` +
        (pop.top === null ? `bottom:${pop.bottom}px;` : `top:${pop.top}px;`) +
        `width:${width}px;max-height:${pop.maxHeight}px`,
  );

  async function reload() {
    if (!connectionId) return;
    queries = await listSavedQueries(connectionId);
  }

  async function save(overwrite: boolean) {
    if (!connectionId || !canSave) return;
    busy = true;
    error = '';
    try {
      await saveQuery(connectionId, name.trim(), sql, overwrite);
      clashing = null;
      await reload();
      name = defaultQueryName(sql, queries);
    } catch (e) {
      // The backend answers a taken name with this exact string so the two
      // cases stay distinguishable: one is a question for the operator, the
      // other is a failure to report.
      if (String(e) === 'duplicate-name') {
        clashing = name.trim();
      } else {
        error = i18n.t('saved-save-failed');
      }
    } finally {
      busy = false;
    }
  }

  async function remove(entry: SavedQueryView) {
    if (!connectionId) return;
    if (!confirm(i18n.t('saved-delete-confirm', { name: entry.name }))) return;
    busy = true;
    try {
      await deleteSavedQuery(connectionId, entry.name);
      await reload();
    } catch {
      error = i18n.t('saved-delete-failed');
    } finally {
      busy = false;
    }
  }

  function load(entry: SavedQueryView) {
    onLoad(entry.sql);
    open = false;
  }
</script>

<div class="saved">
  <button
    type="button"
    class="chip"
    bind:this={btn}
    onclick={toggle}
    disabled={!connectionId}
    aria-expanded={open}
    title={i18n.t('saved-heading')}
  >
    ★ {i18n.t('saved-title', { count: queries.length })}
  </button>

  {#if open}
    <div class="saved-pop" role="menu" style={style}>
      <div class="saved-head">
        <span class="saved-title">{i18n.t('saved-heading')}</span>
      </div>

      <div class="saved-save">
        <input
          type="text"
          bind:value={name}
          placeholder={i18n.t('saved-name-placeholder')}
          aria-label={i18n.t('saved-name-placeholder')}
        />
        {#if clashing === null}
          <button type="button" class="primary" onclick={() => save(false)} disabled={!canSave}>
            {i18n.t('saved-save')}
          </button>
        {:else}
          <button type="button" class="primary" onclick={() => save(true)} disabled={busy}>
            {i18n.t('saved-replace')}
          </button>
        {/if}
      </div>

      {#if clashing !== null}
        <p class="saved-clash" role="alert">
          {i18n.t('saved-name-taken', { name: clashing })}
        </p>
      {/if}
      {#if error}<p class="saved-error" role="alert">{error}</p>{/if}

      {#if queries.length === 0}
        <p class="saved-empty">{i18n.t('saved-empty')}</p>
      {:else}
        <ul class="saved-list">
          {#each sorted as entry (entry.name)}
            <li>
              <button
                type="button"
                class="saved-item"
                onclick={() => load(entry)}
                title={entry.sql}
              >
                <span class="saved-item-name">{entry.name}</span>
                <span class="saved-item-sql">{entry.sql}</span>
              </button>
              <button
                type="button"
                class="saved-remove"
                onclick={() => remove(entry)}
                title={i18n.t('saved-delete')}
                aria-label={i18n.t('saved-delete')}
              >
                ✕
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>

<style>
  .saved {
    position: relative;
  }
  .saved-pop {
    position: fixed;
    display: flex;
    flex-direction: column;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-window);
    box-shadow: var(--shadow-popover);
    overflow: hidden;
    z-index: 20;
  }
  .saved-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }
  .saved-title {
    font-weight: 600;
    font-size: 0.85rem;
  }
  .saved-save {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }
  .saved-save input {
    flex: 1;
    min-width: 0;
  }
  .saved-clash,
  .saved-error,
  .saved-empty {
    margin: 0;
    padding: 0.5rem 0.75rem;
    font-size: 0.8rem;
    color: var(--text-muted);
  }
  .saved-clash,
  .saved-error {
    color: var(--danger, #c33);
  }
  .saved-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
  }
  .saved-list li {
    display: flex;
    align-items: stretch;
    border-bottom: 1px solid var(--border-subtle, var(--border));
  }
  .saved-item {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    align-items: flex-start;
    padding: 0.4rem 0.75rem;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
  }
  .saved-item:hover {
    background: var(--bg-hover);
  }
  .saved-item-name {
    font-weight: 600;
    font-size: 0.85rem;
  }
  .saved-item-sql {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-muted);
  }
  .saved-remove {
    background: none;
    border: none;
    padding: 0 0.6rem;
    color: var(--text-muted);
    cursor: pointer;
  }
  .saved-remove:hover {
    color: var(--danger, #c33);
  }
</style>
