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
| `accent` | Primary action, links | TBD |
| `danger` | Destructive, errors | TBD |
| `warning` | Caution, slow queries | TBD |
| `success` | Healthy connection, OK | TBD |

We will offer at least one dark theme as the default and one light theme
as an option.

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
