# Desktop (Tauri) ↔ egui feature parity

Status of the Tauri 2 + SvelteKit desktop app (`apps/desktop/`) against the
mature egui client (`crates/dbboard-ui/`). The Tauri app began as a **read-only
spike** (ADR-0046 / ADR-0059): a static SPA over 7 read-only IPC commands
reusing `McpService`. Write surfaces are deliberately absent and gated behind a
new ADR — see "Deliberately out of scope" below.

The read-only spike has since begun its **v0.4.0 feature-parity** effort: write
surfaces are being promoted one vertical at a time, each behind its own ADR (see
the ADR-0062 scope map in `decisions.md`). Landed write surfaces are listed in
their own section below.

Legend: ✅ done · 🟦 done this pass · ⛔ not yet (needs an ADR / write surface) ·
➖ intentionally omitted.

_Last updated: 2026-07-29 (SSH tunnel connection UI — ADR-0069; desktop leads egui on tunnel editing)._

## Read / inspect (the spike's remit)

| Feature | egui | Tauri | Notes |
|---|---|---|---|
| List connections | ✅ | ✅ | Read-only view; no add/edit/delete (see below). |
| First-run empty state | ✅ | ✅ | With zero connections the Query panel explains where to register one and shows the resolved `connections.toml` path (read-only `config_path` command). |
| Browse tables | ✅ | ✅ | Sidebar list with count. |
| Schema search (tables + columns) | ✅ | ✅ | Debounced `search_schema`. |
| Table structure (columns / types / PK) | ✅ | ✅ | Structure tab. |
| Foreign-key relationships | ✅ | ✅ | Shown on the Structure tab. |
| Local annotations (table/column notes) | ✅ | ✅ | Display **and** edit — see the write-surface section (ADR-0045). |
| Run read-only SQL | ✅ | ✅ | SELECT/WITH/EXPLAIN; writes rejected at the engine. |
| Aurora DSQL read path | ✅ | ✅ | Streaming row-cap instead of `DECLARE CURSOR` (854b1f2). |
| Results grid: sort / multi-select / copy TSV·CSV / download | ✅ | ✅ | |
| Row-limit control + explicit cap display | ✅ | 🟦 | Selector 100/200/500/1000; grid shows "capped at N". Backend hard cap is 1000 (`MAX_MAX_ROWS`). |
| SQL query history | ✅ | 🟦 | Per-connection, localStorage, dedup, click-to-load, clear. |
| Multi-language i18n (11 locales) | ✅ | 🟦 | en + ja complete; other 9 fall back to en (translations ported from `dbboard-i18n`). |
| Language switcher | ✅ | 🟦 | Top bar; persisted. |
| Theme (auto/light/dark) | ✅ | ✅ | Now localized. |
| Version + Help/About | ✅ | 🟦 | About dialog: version (`getVersion`), docs hint, repo link. |

## Write surfaces landed under v0.4.0 parity

Each of these was a ⛔ read-only-spike boundary that has since been promoted
behind its own ADR. The read/write split is enforced at the method level:
external MCP agents keep the exact read-only surface — none of these writes is
registered as an MCP tool.

| Feature | egui | Tauri | ADR / safeguard |
|---|---|---|---|
| Connection CRUD (add/edit/delete) | ✅ (`connections.rs`) | ✅ | ADR-0062. `ConnectionAdmin` owns `connections.toml` + keyring with rollback; secrets never in TOML. |
| Bundle import/export (passphrase-encrypted) | ✅ | ✅ | ADR-0062 / ADR-0038. |
| Annotation editing (table/column notes, empty = delete) | ✅ | ✅ | ADR-0045. Local-only write to `annotations.toml`; no DB write. |
| CSV/dataset export (CSV / CSV-with-BOM / TSV, row selection) | ✅ (`export.rs`) | ✅ | ADR-0035 / ADR-0049. |
| Inline cell editing (update rows) | ✅ (`edit.rs`) | ✅ | ADR-0063. UPDATE-only; requires a **declared PK**; `rows_affected == 1` commit gate; rowid-only/view results stay read-only. |
| Logical backup / dump (whole-connection SQL) | ✅ (`backup.rs`) | ✅ | ADR-0064 (wires ADR-0049/0050). Read-only dump to a file; `dump:progress` events + cancel flag; warn-and-allow threshold (frontend-owned); SQLite/Turso data-only (no DDL). |
| Logical restore / import (apply a `.sql` script) | ✅ (`restore.rs`) | ✅ | ADR-0065 (wires ADR-0051). Applies a chosen `.sql` file; `restore:progress` events + cancel flag; empty-target confirmation gate; per-engine transaction (atomic batch vs per-statement `on_error`); unparsed statements run best-effort. |
| AI assistant (explain SQL / draft SQL) | ✅ (`ai.rs`, `ai_settings.rs`) | ✅ | ADR-0066 (wires ADR-0052). Streams via `ai:chunk` + cancel flag; **never runs SQL, never sends row data** — Explain sends SQL text, Suggest sends schema names only (`describe_table` on opt-in). API key keyring-only (`dbboard.ai.<id>.api_key`), never in TOML/logs/WebView. No AI command is an MCP tool. |
| Auto-update (check + Install & Restart) | ✅ (inform-only, ADR-0040) | ✅ | ADR-0067 (wires ADR-0044/0043). `tauri-plugin-updater` verifies a signed `latest.json` and installs in place, then `tauri-plugin-process` relaunches — one step past egui's inform-only check. Same `DBBOARD_NO_UPDATE_CHECK` opt-out; minisign public key committed, private key CI-secret only; `latest.json` assembled in `release.yml`. |
| SSH-tunnel connection editing (bastion host/port/user, key/password auth, host-key pin) | ➖ (TOML-only) | ✅ | ADR-0069 (wires ADR-0034/0016). Desktop **leads** egui: the connection form gains an SSH section for tunnel-capable kinds; the tunnel itself (russh local forward, ADR-0069) is shared, but egui has no editor UI — a tunnel is added there by hand-editing `connections.toml`. Passphrase/password keyring-only (`ssh_passphrase`/`ssh_password`), never in TOML; host-key verification mandatory (fingerprint XOR known_hosts, no blind-accept). |

## Deliberately out of scope (still pending an ADR / write surface)

These remain **write surfaces or external integrations** not yet ported. Adding
any of them breaches the ADR-0046/0059 boundary and must be introduced with a
new ADR, not folded in silently.

| Feature | egui | Tauri | Blocker |
|---|---|---|---|
| Row insert / delete | ✅ (`edit.rs`) | ⛔ | Cell editing (ADR-0063) covers UPDATE only; INSERT/DELETE need their own gate. |

## Notes for the next pass

- The 1000-row hard cap (`crates/dbboard-mcp/src/service.rs`) is a reconnaissance
  ceiling. Real bulk work needs either a raised cap or pagination — both are
  backend-policy changes (ADR), not a frontend tweak.
- The **v0.4.0 feature-parity** effort is complete: every write/integration
  vertical from the egui client is now ported (connections, cell edit,
  annotations, export, backup, restore, AI assistant) and, with auto-update
  landed (ADR-0067), the Tauri app can update itself in place. The one remaining
  ⛔ row (row insert/delete) is a genuinely new write surface in *both* clients,
  not a port. Each such addition still opens with its own ADR describing the
  surface and its safeguards.
- **SSH-tunnel editing (ADR-0069) is the first surface where the desktop app
  leads egui rather than catching up.** The tunnel plumbing (russh local
  forward) is shared by both clients, but only the desktop form can create or
  edit a tunnel; in egui a tunnel is still TOML-only. This is intentional — the
  desktop app is now the tunnel editor of record.
- **Before the first v0.4.0 release:** set the `TAURI_SIGNING_PRIVATE_KEY`
  GitHub Actions secret (empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) to the
  generated minisign private key. The public key is already embedded in
  `tauri.conf.json`; without the secret the `build-tauri-*` release jobs cannot
  sign updater artifacts and will fail (ADR-0067).
