# 0004: Runtime locale switcher (revises ADR-0015)

- **Status**: closed (resolved by ADR-0022, 2026-06-11)
- **Phase**: 2 (UX polish, post ADR-0020)
- **Opened**: 2026-06-04
- **Unblocked**: 2026-06-11 (ADR-0020 merged in PR #14, `develop@209fd81`)
- **Closed**: 2026-06-11 — implemented on `feature/runtime-locale-switcher`.
  ADR-0022 records the design (per-session in-process swap, no
  Command/Reply round trip, native names hard-coded in `apps/dbboard`,
  no font re-registration). All acceptance items below ticked.

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

- [x] Menu bar gains a "Language" / "言語" item (locale-aware label
      itself) that opens a submenu listing all 11 supported locales
      with their native names (e.g. `English`, `日本語`, `한국어`,
      `中文 (简体)`, `中文 (繁體)`, `Deutsch`, `Français`,
      `Español`, `Português (Brasil)`, `Русский`, `Italiano`)
- [x] Selecting a locale rebinds `dbboard-i18n`'s active bundle
      and triggers a full UI re-paint with no app restart
- [x] The active locale is visually marked in the submenu (check
      mark on the current entry)
- [x] `DBBOARD_LANG` (when set) still takes precedence at startup;
      runtime switching overrides it for the rest of the session
- [x] No persistence across launches (matches ADR-0020's
      "per-session, no last-active persistence" decision)
- [x] `DbError` body text stays English (ADR-0009 HTTP contract is
      untouched; this is a UI-side change only)
- [x] Unit test covering the bundle swap (`set_language_swaps_active_bundle_at_runtime`
      walks ja → en → zh-CN and asserts both `t!()` and
      `current_language()` flip on every step)
- [x] ADR-0015 status updated to "Superseded in part by ADR-0022
      for startup-only resolution"

## Notes

- ADR-0022 follows ADR-0020's pattern (short context citing
  first-use feedback, supersede the relevant ADR-0015 decision
  only, leave the rest intact).
- Final shape was simpler than the issue predicted: `dbboard-i18n`'s
  existing `init()` is already reselect-capable, so no
  `Arc<RwLock<FluentBundle>>` rework was needed. The new surface is
  just `set_language(tag)` + `current_language()` on the same global
  `FluentLanguageLoader`. Macros (`t!` / `t_args!`) are unchanged.
- The `ConnectionSwitcher`-style `Command`/`Reply` channel was
  **not** used for the locale switcher (see ADR-0022 "Alternatives
  considered"). Locale switching has no I/O — a synchronous UI-thread
  mutation + `request_repaint()` is the right shape; routing through
  the worker would only add latency.
- CJK font re-registration was **not needed**: `install_cjk_font`
  *appends* a CJK fallback to egui's font stack, so the same
  registered font covers `ja` / `ko` / `zh-CN` / `zh-TW` regardless
  of which is active. A `ja` → `zh-CN` switch renders without tofu
  using the existing startup-time registration.
- No web mirror — same category as ADR-0015 / ADR-0020 (desktop-
  side UX, no contract change).
- ~~Blocked by: ADR-0020 implementation~~ — ADR-0020 shipped in PR #14
  (`develop@209fd81`, 2026-06-11). ADR-0022 borrows the
  in-process-mutation framing but lands a leaner wiring.
