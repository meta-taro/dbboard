# 0016: Extract Structure-tab render into its own module + drop the per-frame column clone

- **Status**: open — follow-up debt, not blocking
- **Phase**: maintenance / quality
- **Opened**: 2026-07-21
- **Raised by**: pre-merge Rust review of ADR-0045
  (`feature/adr-0045-local-annotations`, the local-annotations feature)
- **Depends on**: nothing; can land any time after ADR-0045 merges

## Context

ADR-0045 added the editable **Note** column to the Structure tab. The feature
is correct and merged, but the review flagged that it *continues* two
pre-existing trends in `crates/dbboard-ui/src/lib.rs` rather than reversing
them. None of these blocked the ADR-0045 merge; they are collected here so the
debt doesn't keep compounding silently across features.

## Items

1. **`crates/dbboard-ui/src/lib.rs` is ~4002 lines** (was ~3785 before this
   branch). CLAUDE.md's hard file limit is 800. This is the second feature in a
   row to grow the file instead of extracting a submodule. Extract the
   Structure-tab rendering into a `structure.rs` submodule:
   `render_structure`, `render_schema_grid`, `render_table_note`,
   `commit_structure_note`, `NoteTarget`, `StructureView`. Pre-existing debt,
   but the natural cut point is exactly the code ADR-0045 touched.

2. **`render_schema_grid` exceeds the 50-line function soft limit** (~100 lines
   of egui `TableBuilder` boilerplate plus the Note-column edit logic). Split
   the note-cell rendering into its own helper (e.g. `render_note_cell`).

3. **Per-frame `Vec<ColumnInfo>` clone in the render hot path**
   (`schema.columns.clone()`, ~`lib.rs:1798`). The function this replaced took
   `&TableSchema` by reference with zero allocation; the clone was introduced to
   sidestep the simultaneous `&mut self.structure` / `&self.annotations` borrows
   the Note column needs. Bounded impact (egui repaints mostly on interaction,
   column counts small) but a real, avoidable regression. Fix by splitting the
   borrows via a small struct or passing indices instead of cloning.

4. **`expect("structure present")` in `render_schema_grid` / `render_table_note`**
   is infallible only by convention (the sole caller early-returns when
   `self.structure` is `None`), not by construction. Passing `&mut StructureView`
   directly into the helpers would make the invariant type-enforced and drop the
   `expect`. Folds naturally into item 1's extraction.

5. **Minor test-coverage gaps in `annotations.rs`** (`#[cfg(test)] mod tests`):
   no test for genuinely malformed (non-TOML-syntax) input hitting
   `AnnotationsError::Parse` (only version-mismatch and duplicate-key paths are
   exercised); no isolated `prune` partial-case test; no test for
   `commit_structure_note`'s early return when no Structure view is open. Happy
   paths, round-trip, key-isolation, and prune-to-empty are already well
   covered — these are stakes-appropriate extras given the "documentation, not
   schema" data-loss framing, not load-bearing gaps.

## Acceptance

- [ ] Structure-tab render code lives in a focused `structure.rs`; `lib.rs`
      shrinks materially toward the 800-line limit.
- [ ] `render_schema_grid` is back under (or near) the 50-line soft limit via a
      `render_note_cell` helper.
- [ ] The Structure render path no longer clones the column vector per frame.
- [ ] The `expect("structure present")` calls are gone (invariant enforced by
      passing `&mut StructureView`).
- [ ] Added tests: malformed-TOML parse error, isolated `prune` partial case,
      `commit_structure_note` no-open-view early return.
- [ ] All four mandatory verification commands stay green; no behaviour change
      to the Note feature.

## Scope guard

Desktop-only, in-process, no wire/`history.jsonl` change → **no `dbboard-web`
mirror required**.
