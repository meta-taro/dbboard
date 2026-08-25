# 0016: Extract Structure-tab render into its own module + drop the per-frame column clone

- **Status**: closed — 2026-08-24. Items 1–4 are obsolete (the code they name
  no longer exists); the two surviving test gaps in item 5 are done.
- **Phase**: maintenance / quality
- **Opened**: 2026-07-21
- **Closed**: 2026-08-24
- **Raised by**: pre-merge Rust review of ADR-0045
  (`feature/adr-0045-local-annotations`, the local-annotations feature)

## Why most of this is obsolete

Every item below except item 5 is about `crates/dbboard-ui/src/lib.rs`, the
egui client. That crate was **retired at v0.4.0** (`af17200`, ADR-0089) in
favour of the one Tauri 2 + SvelteKit client. There is no `dbboard-ui` in the
workspace, no `render_schema_grid`, no `StructureView`, and no per-frame
`Vec<ColumnInfo>` clone: egui repainted every frame, Svelte does not repaint
at all unless state changes, so the hot path the review measured has no
counterpart. The Structure tab now lives in
`apps/desktop/src/lib/components/StructurePanel.svelte` (468 lines, inside
both limits).

The issue outlived its subject by four months because it was filed against a
file rather than against a behaviour. Nothing in items 1–4 describes something
a user can observe, so nothing in them survived the port.

**This does not mean the debt moved.** The Rust hard-limit violation that
remains is a *different* file — `apps/desktop/src-tauri/src/lib.rs`, 2,880
lines against the 800 hard limit. It is not this issue, and filing it here
would repeat the mistake of attaching a limit to a path. It is recorded in
`.claude/next-actions.md` instead, where the order lives.

## Items

1. ~~`crates/dbboard-ui/src/lib.rs` is ~4002 lines; extract `structure.rs`~~ —
   **obsolete**, crate retired at v0.4.0.
2. ~~`render_schema_grid` exceeds the 50-line soft limit~~ — **obsolete**.
3. ~~Per-frame `Vec<ColumnInfo>` clone in the render hot path~~ — **obsolete**;
   no per-frame render path exists in the Svelte client.
4. ~~`expect("structure present")` is infallible only by convention~~ —
   **obsolete**.
5. **Test-coverage gaps in `crates/dbboard-config/src/annotations.rs`** — this
   file is still here and untouched by the port. Two of the three gaps were
   real and are now closed; the third named an egui function.
   - [x] Malformed (non-TOML-syntax) input reaching `AnnotationsError::Parse`
         → `syntactically_broken_toml_is_a_parse_error`. The other rejection
         paths all run *after* a successful deserialise, so none of them
         reached this arm.
   - [x] Isolated `prune` partial case → `prune_only_drops_the_entries_that_emptied_out`.
         The existing `empty_note_clears_and_prunes_the_entry` only covers
         both retain predicates going false at once; nothing covered a table
         stanza surviving on its table note alone, or a sibling table
         surviving its neighbour being emptied.
   - ~~`commit_structure_note`'s early return when no Structure view is open~~
     — **obsolete**, that function was egui's.

## Acceptance

- [x] Added tests: malformed-TOML parse error, isolated `prune` partial case.
- [x] All four mandatory verification commands stay green; no behaviour
      change (tests only — no non-test line was touched).
- [n/a] The four egui items: subject removed at v0.4.0.

## Scope guard

Desktop-only, in-process, no wire/`history.jsonl` change → **no `dbboard-web`
mirror required**.
