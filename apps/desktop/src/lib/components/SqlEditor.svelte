<script lang="ts">
  // CodeMirror 6 SQL editor (ADR-0060). Colours are wired to the design tokens
  // via var(...) in the CM theme/highlight style, so light↔dark follows the
  // same token switch as the rest of the app — no per-theme editor config.
  import { onMount } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    highlightActiveLineGutter,
    highlightSpecialChars,
    drawSelection,
    placeholder as cmPlaceholder,
  } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
  import {
    autocompletion,
    completionKeymap,
    closeBrackets,
    closeBracketsKeymap,
  } from '@codemirror/autocomplete';
  import { sql } from '@codemirror/lang-sql';
  import { syntaxHighlighting, HighlightStyle } from '@codemirror/language';
  import { tags as t } from '@lezer/highlight';
  import { ExternalDoc } from '$lib/editor/external-doc';

  interface Props {
    /** Seeds the initial document and receives what the user types. It is
     *  deliberately *not* watched afterwards — to replace the contents from
     *  outside, call `setDoc` through `bind:this`. */
    value: string;
    onRun?: () => void;
    placeholder?: string;
  }
  let { value = $bindable(), onRun, placeholder = 'SELECT …' }: Props =
    $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  const external = new ExternalDoc();

  /** Replace the editor's contents from outside (sidebar menu, history
   *  replay). Callable through `bind:this` before the view exists — the
   *  document is buffered and applied on mount. */
  export function setDoc(text: string) {
    external.push(text);
    flush();
  }

  function flush() {
    if (!view || !external.hasPending) return;
    const next = external.take() as string;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: next },
      selection: { anchor: next.length },
    });
  }

  // SQL syntax palette mapped onto the shared design tokens (keyword = accent,
  // string = success/green, number = warning/amber, comment = faint) so it
  // reads as the same product and re-themes for free.
  const highlight = HighlightStyle.define([
    { tag: [t.keyword, t.modifier], color: 'var(--text-accent)', fontWeight: '600' },
    { tag: [t.string, t.special(t.string)], color: 'var(--success)' },
    { tag: [t.number, t.bool, t.null], color: 'var(--warning)' },
    { tag: [t.lineComment, t.blockComment], color: 'var(--faint)', fontStyle: 'italic' },
    { tag: [t.operator, t.punctuation, t.paren], color: 'var(--text-muted)' },
    { tag: [t.function(t.variableName), t.typeName], color: 'var(--accent-hover)' },
  ]);

  const theme = EditorView.theme({
    '&': {
      color: 'var(--text)',
      backgroundColor: 'var(--bg-surface)',
      fontSize: 'var(--text-body)',
      borderRadius: 'var(--radius-widget)',
    },
    '&.cm-focused': { outline: 'none' },
    '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.55' },
    '.cm-content': { padding: 'var(--space-2) 0', caretColor: 'var(--accent)' },
    '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)' },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-code)',
      color: 'var(--faint)',
      border: 'none',
      borderRight: '1px solid var(--border)',
    },
    '.cm-activeLine': { backgroundColor: 'var(--accent-weak)' },
    '.cm-activeLineGutter': {
      backgroundColor: 'var(--accent-weak)',
      color: 'var(--text-accent)',
    },
    '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': {
      backgroundColor: 'var(--accent-weak)',
    },
    '.cm-tooltip': {
      backgroundColor: 'var(--bg-surface)',
      border: '1px solid var(--border)',
      borderRadius: 'var(--radius-widget)',
      boxShadow: 'var(--shadow-popover)',
    },
    '.cm-tooltip-autocomplete ul li[aria-selected]': {
      backgroundColor: 'var(--accent-weak)',
      color: 'var(--text-accent)',
    },
  });

  onMount(() => {
    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        drawSelection(),
        highlightActiveLine(),
        history(),
        closeBrackets(),
        autocompletion(),
        sql(),
        syntaxHighlighting(highlight),
        theme,
        cmPlaceholder(placeholder),
        EditorView.lineWrapping,
        // Run binding is highest precedence so Cmd/Ctrl-Enter is never swallowed.
        keymap.of([
          {
            key: 'Mod-Enter',
            preventDefault: true,
            run: () => {
              onRun?.();
              return true;
            },
          },
        ]),
        keymap.of([
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...historyKeymap,
          ...completionKeymap,
        ]),
        // Push edits back to the bound value without clobbering external sets.
        EditorView.updateListener.of((u) => {
          if (!u.docChanged) return;
          const next = u.state.doc.toString();
          if (next !== value) value = next;
        }),
      ],
    });
    view = new EditorView({ state, parent: host });
    // A `setDoc` that arrived before the view existed lands here.
    flush();
    return () => view?.destroy();
  });
</script>

<div class="editor-host" bind:this={host}></div>

<style>
  .editor-host {
    border: 1px solid var(--border);
    border-radius: var(--radius-widget);
    overflow: hidden;
  }
  /* CodeMirror paints its own focus ring via the accent caret + active line;
     lift the border to accent when the editor is focused. */
  .editor-host:focus-within {
    border-color: var(--accent);
  }
  :global(.editor-host .cm-editor) {
    max-height: 320px;
  }
</style>
