# 0013: Inline cell editing with explicit Save (HeidiSQL-style)

- **Status**: open — **needs an ADR** (first write path in the app)
- **Phase**: 5 (quality of life), but architecturally significant
- **Opened**: 2026-07-16
- **Depends on**: ADR-0028 (`describe_table` gives the columns and
  primary key needed to build a safe `WHERE`).

## Context

Today the result grid is read-only by design. The maintainer wants to edit
data in place and save it, modelled on **HeidiSQL**:

1. **Double-click** a result cell → it turns into an inline edit field
   (a form control in place of the text).
2. **Blur** (focus leaves the field) → the edit is *staged* (仮登録), not
   yet written. The cell keeps the new value and is marked dirty.
3. A **Save** button appears **below the grid**. It is only shown/enabled
   while there is at least one staged edit.
4. Pressing **Save** runs the actual `UPDATE` SQL and commits every staged
   edit. Nothing touches the database before Save.
5. **Dirty cells are tinted** — a staged (unsaved) cell gets a faint
   background colour so the user can see exactly what will be written.

This is the app's **first mutation path** — everything so far reads. That
is why it needs an ADR, not a silent edit: it introduces write-back,
per-row identity, dirty-state, and a save transaction.

## Acceptance

- [ ] Double-clicking a cell switches it to an inline editor; single-click
      still selects the row (must not collide with existing row-select /
      copy behaviour).
- [ ] Blur stages the edit: the cell shows the new value, is tracked as
      dirty, and gets the faint dirty-tint background. No SQL runs yet.
- [ ] A Save button below the grid appears only when ≥1 edit is staged;
      it reports how many rows/cells are pending.
- [ ] Save builds and runs `UPDATE <table> SET <col> = ? WHERE <pk> = ?`
      per edited row, keyed on the table's primary key from
      `describe_table` (ADR-0028). All staged edits commit together;
      on success the dirty-tint clears.
- [ ] A row with **no usable row identity** cannot be edited — the cell
      does not enter edit mode, with a clear hint why (a blind `UPDATE`
      without a unique key could rewrite multiple rows). "Usable identity"
      is **adapter-specific** (see the coverage section below): a declared
      primary key everywhere, plus the implicit `rowid` on SQLite-family
      tables that have one.
- [ ] Editing is only offered for a result set that maps to a single
      updatable base table (a plain `SELECT` from one table). Joined /
      computed / multi-table results are read-only.
- [ ] **Views and other non-table objects are read-only** unless the ADR
      explicitly decides otherwise — do not silently generate an `UPDATE`
      against a view (see coverage section).
- [ ] A staged edit can be reverted (per-cell and/or "discard all") before
      Save, restoring the original value and clearing the tint.
- [ ] Errors from Save use the unified copyable error display (ADR-0039)
      and leave the edits staged (not silently dropped) so the user can
      retry.
- [ ] Behaviour tests: dirty-tracking (stage → dirty set), UPDATE-SQL
      generation (correct `SET`/`WHERE`, parameterised, per PK), and
      "no PK ⇒ not editable" — all without a live DB.

## Per-DB dialect & object-type coverage (raised 2026-07-16)

Write-back is **not uniform across the adapters** — this must be designed
per family, not once generically.

| Family | Adapters | Identifier quote | Placeholder | No-declared-PK identity |
|---|---|---|---|---|
| SQLite | Turso / libSQL, Cloudflare D1 | `"col"` | `?` | implicit **`rowid`** (except `WITHOUT ROWID` tables → refuse) |
| Postgres | Supabase, Neon, Aurora DSQL | `"col"` | `$1` | none reliable (`ctid` is fragile / not stable) → refuse without a real key |

- **Row identity is adapter-specific.** SQLite-family tables without a
  declared primary key still have a usable `rowid`, so "no PK ⇒ refuse" is
  *too strict* there — prefer `rowid`/`oid`-style identity where the
  adapter guarantees it. Postgres has no equally safe implicit key, so
  refuse when there is no PK/unique key.
- **Views**: not directly updatable by default. SQLite/libSQL/D1 views are
  read-only unless they have an `INSTEAD OF` trigger; Postgres updates only
  *simple* views (single base table, no aggregate/DISTINCT/GROUP BY, …) and
  auto-updatable rules. Default: **treat views as read-only** and only
  revisit updatable-view support behind an explicit ADR decision. The UI
  must know whether a queried object is a table or a view — `describe_table`
  / `list_tables` (ADR-0028) should distinguish object kind, or the ADR
  adds that.
- **Other objects** (materialized views, CTE/derived results, foreign
  tables): read-only.
- **Dialect specifics to pin in the ADR**: identifier quoting, placeholder
  style (`?` vs `$n`), boolean/NULL/date encoding, and how each adapter
  reports "rows affected" so Save can confirm exactly one row changed.

## Notes / open questions for the ADR

- **Identity / WHERE clause**: prefer the primary key; fall back to an
  adapter-guaranteed implicit identity (SQLite `rowid`) where available;
  otherwise refuse. Never `WHERE` on all original column values (fragile,
  and ambiguous on duplicate rows).
- **Type fidelity**: the editor works on text; the `UPDATE` must round-trip
  the column type (NULL vs empty string, numbers, booleans, dates). NULL
  needs an explicit affordance, not "empty text".
- **Concurrency**: someone else may have changed the row since it was read.
  Optimistic check (WHERE also matches the original values) vs. last-writer
  -wins. Decide in the ADR; simplest safe default is PK-only + report rows
  affected.
- **Adapter surface**: `run_sql` already executes arbitrary SQL including
  `UPDATE`, so no new adapter method is strictly required — but the ADR
  should say whether write-back goes through a typed, parameterised path
  rather than string-built SQL (injection + type safety).
- **Scope guard**: this is desktop-only / in-process. If the HTTP wire
  contract or `history.jsonl` is touched, a cross-repo brief to
  `dbboard-web` is required; otherwise none.
- **Layering**: SQL generation + dirty model belong below the egui event
  handlers (a use-case module next to the adapter trait), per CLAUDE.md
  "no business logic in UI event handlers".
