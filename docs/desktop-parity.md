# Desktop (Tauri) ↔ egui feature parity

Status of the Tauri 2 + SvelteKit desktop app (`apps/desktop/`) against the
mature egui client (`crates/dbboard-ui/`). The Tauri app began as a **read-only
spike** (ADR-0046 / ADR-0059): a static SPA over 7 read-only IPC commands
reusing `McpService`. Write surfaces are deliberately absent and gated behind a
new ADR — see "Deliberately out of scope" below.

Legend: ✅ done · 🟦 done this pass · ⛔ not yet (needs an ADR / write surface) ·
➖ intentionally omitted.

_Last updated: 2026-07-27 (added first-run empty state)._

## Read / inspect (the spike's remit)

| Feature | egui | Tauri | Notes |
|---|---|---|---|
| List connections | ✅ | ✅ | Read-only view; no add/edit/delete (see below). |
| First-run empty state | ✅ | ✅ | With zero connections the Query panel explains where to register one and shows the resolved `connections.toml` path (read-only `config_path` command). |
| Browse tables | ✅ | ✅ | Sidebar list with count. |
| Schema search (tables + columns) | ✅ | ✅ | Debounced `search_schema`. |
| Table structure (columns / types / PK) | ✅ | ✅ | Structure tab. |
| Foreign-key relationships | ✅ | ✅ | Shown on the Structure tab. |
| Local annotations (table/column notes) | ✅ | ✅ (display) | egui **edits** notes (ADR-0045); Tauri only **displays** them — editing is a write surface (⛔). |
| Run read-only SQL | ✅ | ✅ | SELECT/WITH/EXPLAIN; writes rejected at the engine. |
| Aurora DSQL read path | ✅ | ✅ | Streaming row-cap instead of `DECLARE CURSOR` (854b1f2). |
| Results grid: sort / multi-select / copy TSV·CSV / download | ✅ | ✅ | |
| Row-limit control + explicit cap display | ✅ | 🟦 | Selector 100/200/500/1000; grid shows "capped at N". Backend hard cap is 1000 (`MAX_MAX_ROWS`). |
| SQL query history | ✅ | 🟦 | Per-connection, localStorage, dedup, click-to-load, clear. |
| Multi-language i18n (11 locales) | ✅ | 🟦 | en + ja complete; other 9 fall back to en (translations ported from `dbboard-i18n`). |
| Language switcher | ✅ | 🟦 | Top bar; persisted. |
| Theme (auto/light/dark) | ✅ | ✅ | Now localized. |
| Version + Help/About | ✅ | 🟦 | About dialog: version (`getVersion`), docs hint, repo link. |

## Deliberately out of scope for the read-only spike

These are **write surfaces or external integrations**. Adding any of them
breaches the ADR-0046/0059 boundary and must be introduced with a new ADR, not
folded in silently.

| Feature | egui | Tauri | Blocker |
|---|---|---|---|
| Connection CRUD (add/edit/delete) | ✅ (`connections.rs`) | ⛔ | Write surface; needs credential-handling ADR + IPC. |
| Data editing (insert/update/delete rows) | ✅ (`edit.rs`) | ⛔ | Write surface; the spike is read-only by design. |
| Annotation editing | ✅ | ⛔ | Writes `annotations.toml`; a (local-only) write surface. |
| Backup / restore | ✅ (`backup.rs`, `restore.rs`) | ⛔ | Bulk read + write; separate ADR. |
| AI assistant (explain / draft SQL) | ✅ (`ai.rs`, `ai_settings.rs`) | ⛔ | Provider integration + key storage; the About dialog omits the egui "About AI Assistant" text because the feature is absent here. |
| CSV/dataset export beyond the grid | ✅ (`export.rs`) | Partial | Grid copies/downloads the current result; no dedicated export view. |
| Auto-update / update-check | ✅ (release flow) | ⛔ | Tauri updater plugin + signing; targeted for the v0.4.0 release work. |

## Notes for the next pass

- The 1000-row hard cap (`crates/dbboard-mcp/src/service.rs`) is a reconnaissance
  ceiling. Real bulk work needs either a raised cap or pagination — both are
  backend-policy changes (ADR), not a frontend tweak.
- Promoting any ⛔ row is the natural start of a **v0.4.0 feature-parity** effort;
  each should open with an ADR describing the write surface and its safeguards.
