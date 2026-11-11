<!--
  The Review tab (ADR-0176): changes agents left on their branches, what each
  one changes, and a person's approve or reject. The only place dbboard
  writes to Umbrachyx, and never reachable by an agent.
-->
<script lang="ts">
  import { i18n } from '$lib/i18n/i18n.svelte';
  import { review } from '$lib/review/review.svelte';
  import { changedKeys, tablePattern, type PendingChange } from '$lib/review/view';
  import { diffRows, shortHash } from '$lib/versioned/view';

  let reason = $state('');

  const kindMark = { added: '+', modified: '~', removed: '−' } as const;

  function when(iso: string): string {
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(i18n.locale);
  }

  async function pick(change: PendingChange): Promise<void> {
    reason = '';
    await review.select(change);
  }

  async function decide(verb: 'approve' | 'reject'): Promise<void> {
    await review.decide(verb, reason);
    if (review.outcome?.kind === 'approved' || review.outcome?.kind === 'rejected') reason = '';
  }

  const canDecide = $derived(
    review.selected !== null && reason.trim().length > 0 && !review.deciding && !review.diffLoading,
  );
</script>

<div class="panel">
  <div class="bar">
    <p class="person-only">{i18n.t('review-person-only')}</p>
    <button type="button" class="ghost" onclick={() => review.refresh()} disabled={review.loading}>
      {i18n.t('review-refresh')}
    </button>
  </div>

  {#if review.error}
    <p class="error" role="alert">{review.error}</p>
  {/if}

  <div class="body">
    <ul class="changes" aria-label={i18n.t('tab-review')}>
      {#if review.loading && review.changes.length === 0}
        <li class="muted">{i18n.t('review-loading')}</li>
      {:else if review.changes.length === 0}
        <li class="muted">{i18n.t('review-empty')}</li>
      {/if}
      {#each review.changes as change (change.branch)}
        <li>
          <button
            type="button"
            class="change"
            class:selected={review.selected?.branch === change.branch}
            aria-current={review.selected?.branch === change.branch ? 'true' : undefined}
            onclick={() => pick(change)}
          >
            <span class="why">{change.why}</span>
            <span class="meta">
              {change.authors.join(', ')} · {i18n.t('review-key-count', { count: changedKeys(change) })}
            </span>
            <span class="meta">{when(change.last_at)}</span>
            {#if change.conflicts}
              <span class="flag danger"><span aria-hidden="true">!</span> {i18n.t('review-conflicts')}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>

    <section class="detail">
      {#if !review.selected}
        <p class="muted">{i18n.t('review-pick')}</p>
      {:else}
        {@const change = review.selected}
        <dl class="facts">
          <dt>{i18n.t('review-by')}</dt>
          <dd>{change.authors.join(', ')}</dd>
          <dt>{i18n.t('review-why')}</dt>
          <dd>{change.why}</dd>
          <dt>{i18n.t('review-last-written')}</dt>
          <dd>{when(change.last_at)} <span class="mono muted-inline">{shortHash(change.head)}</span></dd>
        </dl>
        {#if change.main_moved}
          <p class="note">{i18n.t('review-main-moved')}</p>
        {/if}
        {#if change.conflicts}
          <p class="note danger">{i18n.t('review-conflicts')}</p>
        {/if}

        <div class="diffs">
          {#if review.diffLoading}
            <p class="muted">{i18n.t('review-loading')}</p>
          {/if}
          {#each review.diffs as part (part.table)}
            <h3 class="mono">{tablePattern(part.table)}</h3>
            {#if part.error}
              <p class="error">{part.error}</p>
            {:else if part.diff}
              {@const rows = diffRows(part.diff)}
              {#if rows.length === 0}
                <p class="muted">{i18n.t('review-no-changes-in-table', { table: tablePattern(part.table) })}</p>
              {:else}
                <table class="grid">
                  <thead>
                    <tr>
                      <th class="kind-col"></th>
                      {#each part.diff.columns as col (col.name)}<th>{col.name}</th>{/each}
                    </tr>
                  </thead>
                  <tbody>
                    {#each rows as row, r (r)}
                      <tr class="row-{row.kind}">
                        <td class="kind-col">
                          <span class="badge {row.kind}">
                            <span aria-hidden="true">{kindMark[row.kind]}</span>
                            {i18n.t(`versioned-kind-${row.kind}`)}
                          </span>
                        </td>
                        {#each row.cells as cell, i (i)}
                          <td class="mono" class:changed={cell.changed}>
                            {#if cell.before !== null}
                              <span class="before">{cell.before}</span><span class="arrow" aria-hidden="true">→</span>
                            {/if}{cell.text}
                          </td>
                        {/each}
                      </tr>
                    {/each}
                  </tbody>
                </table>
              {/if}
            {/if}
          {/each}
        </div>

        <div class="decide">
          <label class="reason">
            <span class="label">{i18n.t('review-reason')}</span>
            <input
              type="text"
              bind:value={reason}
              placeholder={i18n.t('review-reason-placeholder')}
              disabled={review.deciding}
            />
          </label>
          <button type="button" class="ghost danger" disabled={!canDecide} onclick={() => decide('reject')}>
            {i18n.t('review-reject')}
          </button>
          <button type="button" class="primary" disabled={!canDecide} onclick={() => decide('approve')}>
            {i18n.t('review-approve')}
          </button>
        </div>
      {/if}

      {#if review.outcome}
        {@const o = review.outcome}
        <div class="outcome" class:bad={o.kind === 'refused' || o.kind === 'failed'} role="status">
          {#if o.kind === 'approved'}
            {i18n.t('review-approved', { merge: shortHash(o.merge) })}
          {:else if o.kind === 'rejected'}
            {i18n.t('review-rejected')}
          {:else if o.kind === 'failed'}
            <span class="mono">{o.message}</span>
          {:else if o.refusal.kind === 'conflict'}
            {i18n.t('review-refused-conflict', { keys: o.refusal.keys.join(', ') })}
            {#if o.refusal.more > 0}{i18n.t('review-conflict-more', { count: o.refusal.more })}{/if}
          {:else}
            {i18n.t(o.plan.messageKey)}
          {/if}
        </div>
      {/if}
    </section>
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .person-only {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .changes {
    flex: none;
    width: 300px;
    margin: 0;
    padding: var(--space-2);
    list-style: none;
    overflow: auto;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .change {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    padding: 6px 8px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  .change:hover {
    background: var(--bg-surface-alt);
  }
  .change.selected {
    background: var(--accent-weak);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .why {
    font-size: var(--text-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    color: var(--text-muted);
    font-size: var(--text-hint);
  }
  .flag {
    font-size: var(--text-hint);
  }
  .detail {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px var(--space-3);
    margin: 0;
    font-size: var(--text-small);
  }
  .facts dt {
    color: var(--text-muted);
  }
  .facts dd {
    margin: 0;
  }
  .note {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-widget);
    background: var(--warning-weak);
    font-size: var(--text-small);
  }
  .note.danger,
  .flag.danger {
    color: var(--danger);
  }
  .note.danger {
    background: var(--danger-weak);
  }
  h3 {
    margin: 0;
    font-size: var(--text-small);
    color: var(--text-muted);
  }
  .diffs {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .decide {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
  .reason {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .label {
    color: var(--text-muted);
    font-size: var(--text-small);
  }
  input {
    font: inherit;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    background: var(--bg-surface);
    color: var(--text);
  }
  button.primary,
  button.ghost {
    font: inherit;
    padding: 6px 12px;
    border-radius: var(--radius-widget);
    cursor: pointer;
  }
  button.primary {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--text-on-accent, #fff);
  }
  button.ghost {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
  }
  button.ghost.danger {
    color: var(--danger);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .outcome {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-widget);
    background: var(--success-weak);
    font-size: var(--text-small);
  }
  .outcome.bad {
    background: var(--danger-weak);
  }
  .muted {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-small);
    padding: var(--space-2);
  }
  .muted-inline {
    color: var(--text-muted);
  }
  .mono {
    font-family: var(--font-mono);
  }
  .error {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    color: var(--danger);
    font-family: var(--font-mono);
    font-size: var(--text-small);
    white-space: pre-wrap;
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
    white-space: nowrap;
  }
  th {
    background: var(--bg-surface-alt);
    color: var(--text-muted);
    font-weight: 600;
  }
  td.changed {
    background: var(--warning-weak);
  }
  .before {
    color: var(--text-muted);
    text-decoration: line-through;
  }
  .arrow {
    color: var(--text-muted);
    margin: 0 3px;
  }
  .kind-col {
    width: 1%;
  }
  .badge {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    font-size: var(--text-hint);
    padding: 0 6px;
    border-radius: var(--radius-pill);
    border: 1px solid currentColor;
  }
  .badge.added {
    color: var(--success);
    background: var(--success-weak);
  }
  .badge.modified {
    color: var(--warning);
    background: var(--warning-weak);
  }
  .badge.removed {
    color: var(--danger);
    background: var(--danger-weak);
  }
  .row-removed td.mono {
    color: var(--text-muted);
    text-decoration: line-through;
  }
</style>
