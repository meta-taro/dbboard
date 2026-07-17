# DESIGN.md — Visual Direction

This file captures the visual direction of dbboard. It is a living
document: fill in concrete values as the UI phase progresses.

> Status: **placeholder** — UI work has not started yet. The intent is to
> lock direction here before writing UI code so that egui styling stays
> consistent.

## Vibe

- **Tone**: precise, calm, developer-tool. Closer to a code editor than a
  consumer app.
- **Density**: information-dense by default, with comfortable spacing
  when content is sparse.
- **Motion**: minimal. Animations only where they communicate state
  (e.g. query running, connection status).

## Color Palette

To be defined. Slots:

| Token | Role | Value |
|---|---|---|
| `bg.canvas` | App background | TBD |
| `bg.surface` | Panels, cards | TBD |
| `bg.surface.alt` | Alternate row, hover | TBD |
| `fg.primary` | Body text | TBD |
| `fg.muted` | Secondary text | TBD |
| `accent` | Primary action, links | `#4F46E5` (brand indigo, from the logo) |
| `danger` | Destructive, errors | TBD |
| `warning` | Caution, slow queries | TBD |
| `success` | Healthy connection, OK | TBD |

We offer a **Light**, **Dark**, and **Auto** (follow-OS) theme; Auto is the
default (ADR-0041). Any brand-tinted UI colour (e.g. the accent, or the
staged-edit tint in issue 0013) must be read from the active egui
`Visuals` so it holds up in both themes rather than hard-coding one RGB.

## Logo

dbboard's logo is a **white database-cylinder mark on an indigo
rounded square** — a stacked-disks "database" glyph, the same silhouette
used for the schema browser.

![dbboard logo](apps/dbboard/assets/dbboard-logo-256.png)

- **Master / source of truth**: `apps/dbboard/assets/dbboard.ico` is the
  shipped multi-resolution icon (16–256 px, PNG-based) embedded in the
  Windows `.exe` (`build.rs`) and the WiX installer (`wix/main.wxs`).
  `apps/dbboard/assets/dbboard-logo-256.png` is the 256 px master used for
  docs and for re-rendering at other sizes. Reference these files; do not
  copy the image around ad hoc.
- **Palette**: background indigo **`#4F46E5`**, mark **`#FFFFFF`**. The
  indigo is the project's `accent` colour above.
- **Shape**: rounded square (app-icon convention on Windows/macOS), so it
  reads cleanly as a taskbar / Start-menu / dock icon.
- **Origin & licence**: hand-authored for the Windows packaging work
  (ADR-0032) via a throwaway PowerShell + GDI+ script, because no image
  tooling or brand asset existed. It is **fully original** — no
  third-party, traced, or licensed artwork — so it carries **no external
  copyright encumbrance** and is covered by the project's own licence
  (MIT). Downstream users may reuse it under those terms.
- **Future polish** (optional, not required): a hand-drawn SVG master
  would scale better than the GDI+ raster; until then the 256 px PNG is
  the largest clean source.

## Typography

- **UI sans**: TBD (egui default acceptable for v0).
- **Code / SQL / results**: monospace, TBD.
- **Sizes** (rem-equivalent in egui px):
  - body: TBD
  - heading: TBD
  - small / hint: TBD

## Spacing & Radius

- Base unit: TBD (likely 4px).
- Card radius: TBD.
- Button radius: TBD.

## Components (initial scope)

- **Connection list** (sidebar) — list of configured databases with
  status pills.
- **Schema browser** — tree view of tables / views / functions.
- **SQL editor** — monospace, syntax-aware where feasible.
- **Result table** — virtualised, sortable, copy-friendly.
- **Status bar** — connection health, last query timing.

Each component will get a small style spec in this file once it is built.

## Layout

- Default: three-pane (sidebar / editor / results) inspired by classic
  DB clients.
- Resizable splitters with sensible minimum sizes.
- Responsive only in the sense of "behaves well at 1280×720 and up".

## Accessibility

- Respect OS font scale.
- Keyboard-first navigation for power users.
- Sufficient contrast at every theme variant (target WCAG AA).
