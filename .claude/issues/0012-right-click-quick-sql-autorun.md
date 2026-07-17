# 0012: Right-click quick-SQL should run, not just insert

- **Status**: closed — shipped 2026-07-17 (PR #76)
- **Phase**: 5 (quality of life)
- **Opened**: 2026-07-16
- **Tracks**: extends PR #59 (table right-click starter queries)

## Context

Right-clicking a sidebar table offers "Select all rows" / "Count rows"
starter queries (PR #59). Today the handler only drops the SQL into the
editor — `crates/dbboard-ui/src/lib.rs:1331` sets `self.sql = sql` and
stops; the user still has to press Run. The maintainer wants the pick to
**execute immediately** so a right-click → result is one step.

The plumbing already exists: `run_sql()` (same method the Run button and
F5 / Ctrl+Enter call) reads `self.sql` and dispatches. The starter
queries are read-only (`SELECT` / `SELECT COUNT(*)`), so auto-running one
is safe by construction — the "no DELETE/DROP from a mis-click" guarantee
still holds.

## Acceptance

- [ ] Picking a starter query from the table right-click menu sets the
      editor text **and** runs it in one action (result appears without a
      second Run press).
- [ ] The editor still shows the generated SQL (so the user can tweak and
      re-run) — auto-run does not hide or lock the text.
- [ ] Auto-run respects `self.busy` — no double-dispatch while a query is
      already in flight (defer or ignore, matching the Run button's
      `add_enabled(!self.busy, …)`).
- [ ] The auto-`LIMIT` guard (ADR-0030) still applies to the bare
      `SELECT *` starter, exactly as when typed by hand.
- [ ] Unit/behaviour test covers "starter pick triggers a run" without a
      live DB (assert the run path is invoked, not just the text set).

## Notes

- Likely shape: in `render_tables_panel`, after `self.sql = sql`, set a
  flag (e.g. `run_after_apply`) and call `self.run_sql()` once the
  `&self.tables` borrow ends (same deferral pattern already used for
  `quick_sql` / `open_structure`). Do not call `run_sql()` inside the
  `context_menu` closure — the borrow is still held there.
- Keep it read-only. If write-capable starters are ever added, revisit the
  "auto-run is always safe" assumption.
