# 0004: Runtime locale switcher (revises ADR-0015)

- **Status**: open (unblocked)
- **Phase**: 2 (UX polish, post ADR-0020)
- **Opened**: 2026-06-04
- **Unblocked**: 2026-06-11 (ADR-0020 merged in PR #14, `develop@209fd81`)

## Context

ADR-0015 (multilingual UI) chose **startup-only** locale resolution:
`DBBOARD_LANG` → OS → `en`. To switch languages today the user has
to set `DBBOARD_LANG=<tag>` and restart the app. The egui menu bar
has no language switcher.

First-real-world-use feedback (2026-06-04, same session that
produced ADR-0020): "11 言語に対応したのに切り替えのメニューバーも
ないですね" — the maintainer pointed out the gap. The same shape as
the ADR-0020 problem (we shipped a capability but never wired the
UI affordance), and the same fix shape: mutate the running process
instead of forcing a restart.

Wait until ADR-0020 lands — the in-process-mutation precedent it
sets makes the locale switcher a direct port of the same pattern,
with smaller blast radius (no adapter, no HTTP, just text
re-rendering).

## Acceptance

- [ ] Menu bar gains a "Language" / "言語" item (locale-aware label
      itself) that opens a submenu listing all 11 supported locales
      with their native names (e.g. `English`, `日本語`, `한국어`,
      `中文 (简体)`, `中文 (繁體)`, `Deutsch`, `Français`,
      `Español`, `Português (Brasil)`, `Русский`, `Italiano`)
- [ ] Selecting a locale rebinds `dbboard-i18n`'s active bundle
      and triggers a full UI re-paint with no app restart
- [ ] The active locale is visually marked in the submenu (check
      mark on the current entry)
- [ ] `DBBOARD_LANG` (when set) still takes precedence at startup;
      runtime switching overrides it for the rest of the session
- [ ] No persistence across launches (matches ADR-0020's
      "per-session, no last-active persistence" decision)
- [ ] `DbError` body text stays English (ADR-0009 HTTP contract is
      untouched; this is a UI-side change only)
- [ ] Unit test covering the bundle swap (set locale → assert
      `t!("some-key")` returns the expected translation)
- [ ] ADR-0015 status updated to "Superseded in part by ADR-NNNN
      for startup-only resolution"

## Notes

- The ADR for this work should follow ADR-0020's pattern: short
  context (point at first-use feedback), supersede ADR-0015's
  startup-only decision, leave the resolution chain and locale set
  intact.
- fluent-rs supports runtime locale changes natively; the only
  question is how to plumb a "current locale" handle through
  `dbboard-ui`'s widgets so a swap triggers a re-render. Likely
  shape: `Arc<RwLock<FluentBundle>>` behind the existing
  `t!()` / `t_args!()` macros, plus an egui `request_repaint()`
  on swap.
- CJK font registration in `apps/dbboard` is already locale-aware
  (per ADR-0015 consequence list); it only runs at startup though,
  so a `ja` → `zh-CN` switch at runtime needs to confirm the
  registered font still covers the new locale's glyph set. If it
  doesn't, this issue grows to include font re-registration.
- No web mirror — same category as ADR-0015 / ADR-0020 (desktop-
  side UX, no contract change).
- ~~Blocked by: ADR-0020 implementation~~ — ADR-0020 shipped in PR #14
  (`develop@209fd81`, 2026-06-11). The in-process-mutation precedent
  and the `ConnectionSwitcher`-style trait wiring through
  `Command`/`Reply` are the direct template for the locale switcher.
