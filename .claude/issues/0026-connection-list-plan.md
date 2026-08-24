# 0026: The connection list — what could be built there, and in what order

- **Status**: open — C's one line, F, A and B are built, which is all of #192;
  D and E are still planning only, and nothing here is scheduled by this file
- **Opened**: 2026-08-22
- **Asked for**: operator-controlled order, and colour marks. Order is built
  (2026-08-24); colour marks are not.
- **Owner**: maintainer decides what, if anything, moves to `docs/roadmap.md`
- **Related**: #192 (order / narrowing / truncation — open), #213 and PR #216
  (duplicate and repair — the last work in this area), ADR-0045 (annotations:
  the precedent for a per-id sidecar), ADR-0087 / ADR-0088 (the two existing
  optional scalars on `ConnectionEntry`), DESIGN.md §Color Palette (locked v1)

## Where the list stands today

Verified against `feature/duplicate-and-repair-connection` (= `develop` + #216).

- The sidebar renders `workspace.connections` in the order
  `ConnectionAdmin::entries()` returns it, which is `[[connections]]` file
  order. No sort, no stored position (`Sidebar.svelte:126`, `store.rs:481`).
- A row is `icon | name | kind` on one line. `.nav-name` is
  `flex:1; overflow:hidden; text-overflow:ellipsis` and `.nav-meta` is
  `flex:none`, so the adapter label always wins the width and the **name** is
  what gets cut (`Sidebar.svelte:400-412`).
- The row's `title` is `c.id`, not `c.name`. So at the exact moment the name
  truncates, hovering shows something else (`Sidebar.svelte:131`).
- The only search box in that column filters tables and columns, and sits
  *below* the connection list (`Sidebar.svelte:145`).
- `ConnectionEntry` carries `id`, `name`, flattened `kind`, `mcp_write`,
  `mcp_alias`, `ssh`. Two of those are working precedents for "optional scalar,
  skipped when default, emitted before the `ssh` table".
- `ConnectionManager.svelte` was **1614 lines** against CLAUDE.md's 800-line
  hard limit, which is what F below is about. It is 662 since the split, so the
  remaining items land in a file that has room for them.

## Two facts that decide most of the design

**1. Order needs no schema change.** `[[connections]]` is a TOML array of
tables, which preserves order, and `ConnectionAdmin` hands the `Vec` back as
parsed. So reordering *is* rewriting the `Vec`: no new field, no
`CONFIG_VERSION` bump, readable by every older build, and it travels inside a
`.dbbx` for free because `BundlePayload` carries the whole `ConnectionFile`
(`bundle.rs:55-60`). That answers #192's fourth criterion outright.

**2. A new scalar field silently disappears if an older build saves.** Nothing
in `dbboard-config` sets `deny_unknown_fields`, so an older dbboard *reads* a
file containing `colour = "…"` without complaint — and then drops it on its next
`save_atomic`. Order is immune to this (it is the array itself); a colour field
is not. Three ways out, in ascending cost: accept the loss (it is cosmetic),
keep the colour in a sidecar keyed by id the way `annotations.toml` does
(ADR-0045) so no old build ever rewrites it, or bump `CONFIG_VERSION` and write
a migration.

## The menu

### A. Operator-controlled order — #192 criterion 1

Reorder the `Vec` and save: a `ConnectionAdmin::move_to(id, index)` beside
`add` / `update` / `delete`, and ▲▼ buttons on each row of the manager list.
The cheapest item on this page that removes a daily irritation, and the only one
whose storage question is already settled.

**Built** (2026-08-24, `feature/connection-order`), as described: `move_to`
errors rather than clamps an index the list does not have, because a clamp would
put the connection somewhere the operator did not point at; a move to the index
an entry already occupies is a no-op that does not rewrite the file. The index
arithmetic that decides when ▲ and ▼ are disabled lives in
`$lib/connections/order.ts` so the ends of the list are testable at all.

Drag-and-drop in the sidebar is the nicer version of the same feature and a much
larger one — pointer events, a keyboard equivalent so it stays reachable without
a mouse, autoscroll at the edges. Worth doing only if ▲▼ turns out to be tedious
in practice, which is knowable after a week of use and not before.

### B. Narrow the list by typing — #192 criterion 2

A second search input, above the list, visually distinct from the table/column
one below it. Pure frontend: no crate, no config, no contract change. Filters on
name and id, not on kind.

**Built** (2026-08-24, `feature/connection-order`), in the manager list rather
than the sidebar — that is where the rows carry actions, and where A put the
▲▼. Every typed word has to match, so a second word narrows. The kind stays
unmatched as planned: typing `my` to reach "my shop" would otherwise return
every MySQL row. `$lib/connections/filter.ts`.

One thing the plan did not anticipate: **A and B interfere.** ▲▼ move an entry
within the *stored* list, so while rows are hidden they would move a connection
past something the operator cannot see. They are disabled while a filter is
narrowing, rather than remapped — "below the next visible row" is a different
feature, and a silent wrong answer is worse than a disabled button.

### C. Stop the name and the adapter label fighting — #192 criterion 3

Three sub-options in ascending cost: make `title` show the name (do this
regardless — it is one line); move the label onto a second line or onto the icon
as a badge; make the sidebar width draggable. Note the label is `capitalize`d
raw kind text, so `Aurora-Dsql-Iam` is the widest thing that can appear there.

### D. Colour marks — the thing that was asked for

What it is actually for: telling production apart from a copy at a glance, in a
list where several rows read alike. That argues for the colour being a property
of the **connection** — travelling with an export, appearing wherever the
connection does — rather than a local display preference.

It needed three decisions. Two are now settled; the third is a constraint, not a choice:

- **Storage** — a scalar on `ConnectionEntry` (travels; subject to fact 2) or a
  sidecar keyed by id (survives an older build; does not travel unless the
  bundle learns about it). Recommendation: the scalar, and accept fact 2. Losing
  a colour is not losing a credential.
- **Palette — decided (2026-08-22, by the maintainer): a conventional named set,
  about what Google Calendar offers.** That settles the open question in this
  plan. Concretely it means: a fixed, closed list of named colours — not a
  colour picker, not a hex field. Eight is enough (Google offers eleven and the
  tail is rarely used); each gets a name the operator can say out loud, because
  a name is what makes it usable when the colour cannot be seen. DESIGN.md gains
  a third colour axis: identity colours, distinct from the accent and from the
  semantic `--danger` / `--warn` / `--success`. Each needs a light and a dark
  value — Google's hues are tuned for a white chip and go muddy on a dark
  surface, so they cannot be pasted in as-is. The set is not stored per
  connection as a hex string but as its **name**; that keeps the theme swap
  possible later and keeps a stray value out of the config file.
- **Colour alone is not a mark.** It fails for a colour-blind operator and in a
  greyscale screenshot. Whatever ships should pair the colour with something
  non-chromatic — a short tag string, or a shape on the icon.

### E. Grouping / folders — not yet

#192 draws this line and it is the right one: grouping is only worth doing if
ordering turns out not to be enough. Revisit once A has been in use.

### F. The prerequisite nobody asked for

`ConnectionManager.svelte` at 1614 lines is twice the hard limit. A, B, C and D
all add UI to it. Splitting the list view, the form, and the
export/import/duplicate/repair dialogs into siblings is not a feature and should
not be sold as one — but stacking A through D onto the current file makes it
worse in a way that gets progressively harder to undo.

## Suggested order

1. ~~**C, the one-line half** — `title` shows the name. Minutes.~~ Done
   (`b48c974`).
2. ~~**A (▲▼)** — settled storage, daily payoff.~~ Done — see A above.
3. ~~**B** — pure frontend, no dependencies.~~ Done — see B above.
   **A + B + C close #192.**
4. ~~**F** — split the manager before D adds a colour picker to it.~~ Done
   (`497a185`, `e0ce918`): 1,617 lines to 662. It ran ahead of B because both
   remaining items add UI to that file.
5. **D** — the palette is settled, so this is now unblocked.
6. **E** — only if A did not settle it. Ask again after a week of ▲▼.

A, B and C together close #192. D and E do not, and should not be folded into
it.

## Not in scope here

Whether `dbboard-mcp` should see order or colour. The agent surface is kept
small on purpose: a colour is for a human eye and an order is for a human hand.
Left out until a use turns up.
