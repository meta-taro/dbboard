# 0015: Adopt the app icon as the official logo

- **Status**: closed — shipped 2026-07-17 (PR #78)
- **Phase**: 5 (quality of life / branding)
- **Opened**: 2026-07-16

## Context

The Windows app icon (`apps/dbboard/assets/dbboard.ico`) was hand-authored
for ADR-0032 (Windows packaging) — a hand-drawn indigo rounded-square with
a database-cylinder mark, generated with PowerShell + GDI+ because no image
tool was available. It is **fully original**: no third-party asset, no
traced or licensed artwork, so there is **no copyright issue** with
adopting it as the project's official logo.

The maintainer wants to formalise it: this icon becomes *the* dbboard logo,
not just a Windows exe icon.

## Acceptance

- [ ] The logo/icon lives in a canonical, documented location and is
      referenced (not duplicated ad hoc) by the Windows build, the WiX
      installer, and any README/docs usage.
- [ ] `DESIGN.md` documents the logo: the mark, the palette (the indigo),
      its origin (hand-authored, original work), and basic usage.
- [ ] The README shows the logo (a small header image), self-contained in
      the repo — no external hosting.
- [ ] A source/master form is kept (at minimum the generation script or a
      high-res PNG) so the logo can be re-rendered at new sizes without
      reverse-engineering the `.ico`.
- [ ] Licensing note: the logo is covered by the project's own licence /
      is original work; state this so downstream users know the reuse
      terms.

## Notes

- Origin of record: ADR-0032, and the session note in
  `.claude/project-status.md` (Windows packaging entry) describing the
  PowerShell + GDI+ generation.
- Optional polish: a cleaner vector (SVG) master would scale better than
  the GDI+ raster, but is not required to "make it official".
- Desktop-only / branding — no code contract change, no web mirror. If
  `dbboard-web` wants to share the mark, that is a separate, coordinated
  decision.
