# 0014: Light / Dark / Auto theme

- **Status**: closed — shipped 2026-07-17 (PR #77)
- **Phase**: 5 (quality of life)
- **Opened**: 2026-07-16

## Context

The app currently ships a single visual theme. Light/dark switching is now
a baseline expectation, plus an **Auto** mode that follows the OS setting.
The maintainer wants all three: Light, Dark, and Auto (system).

egui already provides `egui::Visuals::light()` / `dark()`, and eframe
surfaces the OS preference (system theme) so Auto can track it and react
when the user flips the OS setting at runtime.

## Acceptance

- [ ] A theme control (menu bar entry, e.g. **View → Theme**, or next to
      the Language menu) offers Light / Dark / Auto.
- [ ] Selecting Light or Dark applies immediately to the whole UI
      (`ctx.set_visuals`), including the result grid, editor, dialogs, and
      error display.
- [ ] Auto follows the OS light/dark preference and updates live when the
      OS setting changes, without a restart.
- [ ] The choice persists across restarts (stored with the other app
      config, not hard-coded), and defaults to **Auto** for a first run.
- [ ] The i18n keys for the menu labels exist in en + ja (en is the source
      of truth; other locales fall back to en).
- [ ] The dirty-cell tint (issue 0013) and any custom colours read well in
      both themes — pick tints from the active `Visuals`, not fixed RGB.

## Notes

- The binary (`apps/dbboard`) already owns cross-cutting startup wiring
  (locale, fonts, clock). Reading the persisted theme + resolving Auto vs
  the OS theme fits there; the UI just applies the resolved `Visuals`.
- Small ADR is probably warranted for the persistence location + the
  Auto-follows-OS behaviour, but the change is desktop-only / in-process
  (no HTTP contract, no web mirror).
- Watch interaction with the CJK font registration already done at
  startup — switching visuals must not drop the font set.
