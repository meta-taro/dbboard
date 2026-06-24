# 0008: AI provider Settings UI + `ai-providers.toml` + runtime switcher (Phase 4 Stage 2 Group A)

- **Status**: open — opens against ADR-0025 (2026-06-24).
- **Phase**: 4 Stage 2 Group A (Persistence + Switcher).
  Phase 4 Stage 1 (issue 0005) is closed.
- **Opened**: 2026-06-24
- **Tracks**: ADR-0025
- **Depends on**: ADR-0013 (`SecretStore` + `connections.toml`
  template), ADR-0016 (`ConnectionAdmin` use-case shape),
  ADR-0020 (`swap_backend` + `ConnectionSwitcher` precedent),
  ADR-0022 (`set_language` runtime-switcher precedent),
  ADR-0023 (the `AiProvider` trait surface this slice persists
  and switches), ADR-0024 (`secure_fs` at-rest hardening — applies
  unchanged to the new TOML file). All listed prerequisites are
  closed and load-bearing.
- **Does NOT depend on**: any other Phase 4 Stage 2 group. Group B
  (streaming + cancel), Group C (AI in `history.jsonl`, v:2 schema
  bump + web brief), and Group D (DDL extraction +
  function-calling) are all independent of Group A and can land in
  any order after this one.

## Context

ADR-0023 §9 deferred eight Stage 2 items grouped in this session
into A / B / C / D. ADR-0025 covers Group A — the persistence and
multi-provider switcher — and explicitly re-defers the other
three groups.

The infrastructure to reuse already exists (ADR-0025 §Context
enumerates it). This issue is the implementation of the design
recorded in ADR-0025; nothing in this issue should re-litigate a
Decision recorded there. Surprises that surface during
implementation become a follow-up ADR.

## Acceptance

### `dbboard-config` (TOML schema + use-case)

- [ ] New module `crates/dbboard-config/src/ai_settings.rs`.
- [ ] New module `crates/dbboard-config/src/ai_store.rs` (the
      AI analogue of `store.rs`). Mirrors structure: `AiProviderFile`
      with `version: u32` (constant `AI_CONFIG_VERSION = 1`) +
      `active_id: Option<String>` + `providers: Vec<AiProviderEntry>`.
      `AiProviderEntry { id, name, kind: AiProviderKind }` —
      `serde(flatten)` on `kind` keeps the TOML flat (same shape as
      `ConnectionEntry`).
- [ ] `AiProviderKind` enum, `#[serde(tag = "kind", rename_all =
      "snake_case")]`. Single variant for Stage 2: `Anthropic {
      model: Option<String>, keyring_api_key_ref: String }`.
      Future providers land additively as new variants.
- [ ] `AiProviderFile::parse` validates: `version ==
      AI_CONFIG_VERSION` (else `AiSettingsError::UnsupportedVersion`),
      ids unique (else `AiSettingsError::DuplicateId`), `active_id`
      (when `Some`) references an existing id (else
      `AiSettingsError::UnknownActiveId`).
- [ ] `default_ai_providers_path()` in `store.rs` (or `ai_store.rs`)
      symmetric with `default_path` / `default_history_path`.
      Resolves to
      `<config_dir>/ai-providers.toml` via `ProjectDirs::from("dev",
      "dbboard", "dbboard")`.
- [ ] `load_or_empty(path)` / `save_atomic(path, file)` for
      `AiProviderFile`. `save_atomic` reuses
      `secure_fs::create_new_user_only` → `0o600` on Unix, inherited
      DACL on Windows.
- [ ] `AiSettingsError` enum (`Parse` / `Io` / `Serialize` /
      `UnsupportedVersion` / `DuplicateId` / `UnknownActiveId` /
      `Secret`), `#[derive(Debug, thiserror::Error)]`. Independent
      of `DbError` and `AiError` — these never reach the wire.
- [ ] `AiSettingsAdmin` struct mirroring `ConnectionAdmin`:
      - `load(path, store: Arc<dyn SecretStore>) -> Result<Self,
        AiSettingsError>`
      - `entries(&self) -> &[AiProviderEntry]`
      - `active_id(&self) -> Option<&str>`
      - `add(&mut self, draft: AiProviderDraft) ->
        Result<&AiProviderEntry, AiSettingsError>` — assigns or
        validates id, writes the API key into the `SecretStore`
        under `dbboard.ai.<id>.api_key`, appends the entry, calls
        `save_atomic`.
      - `update(&mut self, id: &str, edit: AiProviderEditDraft)`
        — `SecretField::{Unchanged, Replace, Clear}` semantics
        matching `ConnectionEditDraft`.
      - `delete(&mut self, id: &str)` — removes entry, deletes the
        matching keychain entry (best-effort; logs but does not
        fail the TOML write if the keychain delete fails — same
        posture as `ConnectionAdmin::delete` for orphaned secrets).
        Clears `active_id` if it pointed at this entry.
      - `set_active(&mut self, id: Option<&str>)` — validates id
        exists when `Some`, writes the slot, `save_atomic`.
- [ ] `AiProviderDraft` + `AiProviderEditDraft` mirror
      `ConnectionDraft` + `ConnectionEditDraft`. `api_key` field
      uses `SecretField`.
- [ ] Re-exports added to `crates/dbboard-config/src/lib.rs`:
      `AiProviderFile`, `AiProviderEntry`, `AiProviderKind`,
      `AiProviderDraft`, `AiProviderEditDraft`, `AiSettingsAdmin`,
      `AiSettingsError`, `default_ai_providers_path`,
      `AI_CONFIG_VERSION`.
- [ ] Unit tests: parse round-trip (empty, single entry, multi
      entry, with `active_id`, without `active_id`), reject unknown
      version, reject duplicate id, reject dangling `active_id`,
      `save_atomic` produces a re-parseable file, `secure_fs` mode
      check on Unix (gated on `#[cfg(unix)]`), `AiSettingsAdmin`
      round-trips entries through `InMemorySecretStore`,
      `set_active(None)` clears the field, `delete` clears
      `active_id` when it pointed at the deleted entry, `delete`'s
      keychain best-effort path is reachable. Target: ~20 tests
      matching the density of `store.rs::tests`.

### `dbboard-server` (switcher trait + worker variants)

- [ ] New `AiProviderSwitcher` trait next to `ConnectionSwitcher`:
      ```rust
      pub trait AiProviderSwitcher: Send + Sync {
          fn switch(&self, id: &str) -> Result<(), dbboard_ai::AiError>;
      }
      ```
      Module: existing `connection_switcher.rs` or new
      `ai_provider_switcher.rs` — implementer's call.
- [ ] Worker `Command` enum gains `SwitchAiProvider { id: String }`.
- [ ] Worker `Reply` enum gains `AiProviderSwitched { id: String }`
      and `AiProviderSwitchFailed { reason: String }`.
- [ ] Worker takes an `Arc<dyn AiProviderSwitcher>` next to the
      existing `Arc<dyn ConnectionSwitcher>` and dispatches
      `Command::SwitchAiProvider` through it (mirrors the
      `Command::SwitchConnection` handling).
- [ ] HTTP contract (`docs/api-contract.md`) **untouched** — confirm
      no new routes, no new DTOs, no new error categories. This is
      an in-process channel addition only.
- [ ] Unit tests on the new worker variants: a successful switch
      emits `AiProviderSwitched`; a failed switch emits
      `AiProviderSwitchFailed` with the `AiError::Display` text
      (translation happens UI-side per ADR-0023 Decision 8); the
      `Null` switcher fallback returns
      `AiError::Configuration("no ai store available")`.

### `apps/dbboard` (wiring)

- [ ] `DesktopAiSwitcher` struct mirroring `DesktopSwitcher`. Holds
      `Arc<RwLock<Option<Arc<dyn AiProvider>>>>` (the active-provider
      slot), `Arc<Mutex<AiSettingsAdmin>>`, `Arc<dyn SecretStore>`.
      `switch(id)` resolves the entry, looks up the secret, builds
      the concrete provider via `ai_provider_for_entry`, swaps the
      lock slot.
- [ ] `NullAiSwitcher` — surfaces
      `AiError::Configuration("no ai store available")`. Used when
      `AiSettingsAdmin::load` failed (no config dir, parse error).
- [ ] New function `ai_provider_for_entry(entry: &AiProviderEntry,
      secrets: &dyn SecretStore) -> Result<Arc<dyn AiProvider>,
      AiError>` — the AI analogue of `backend_config_for_entry`.
      Match on `AiProviderKind`; for `Anthropic`, resolve the
      keyring ref, read the model (entry override → crate default),
      construct `AnthropicProvider`.
- [ ] `resolve_ai_provider` refactored: returns
      `Arc<RwLock<Option<Arc<dyn AiProvider>>>>` (not
      `Option<Arc<...>>`). Precedence chain per ADR-0025 Decision 3:
      1. `DBBOARD_ANTHROPIC_API_KEY` env var path (Stage 1
         behaviour preserved verbatim).
      2. `ai-providers.toml` `active_id` path.
      3. Empty slot (`None` inside the lock).
      A broken `active_id` or keychain miss logs to stderr and
      degrades to `None`, same posture as Stage 1's construction
      failure path.
- [ ] `DbboardApp::connect` signature changes from
      `Option<Arc<dyn AiProvider>>` to `Arc<RwLock<Option<Arc<dyn
      AiProvider>>>>`. The only caller is `apps/dbboard::main` —
      caught at compile time. `has_ai_provider()` becomes
      `slot.read().unwrap_or_else(PoisonError::into_inner).is_some()`.
- [ ] Worker request handlers snapshot the provider via
      `slot.read()` once per request and complete against that
      snapshot (matches ADR-0020's "snapshot at request start"
      rule for adapters).
- [ ] README "AI integration (optional)" subsection rewritten to
      document the precedence chain and link `ai-providers.toml` →
      Settings UI path.
- [ ] Integration test: start with no env var, populate
      `ai-providers.toml` with one entry + `active_id`, confirm
      `resolve_ai_provider` returns `Some`. Repeat with `active_id =
      None`, confirm `None`. Repeat with env var set + TOML
      populated, confirm env var wins. Tests live in
      `apps/dbboard/tests/ai_provider_resolution.rs` using an
      `InMemorySecretStore` and a temp config path.

### `dbboard-ui` (Settings panel + worker dispatch)

- [ ] New `AiSettingsView` egui surface mirroring `ConnectionsView`.
      Module: `crates/dbboard-ui/src/ai_settings.rs`. Renders a
      list of `AiProviderEntry` rows with id / name / kind /
      `model` / active marker. Per-row buttons: "Edit" / "Delete" /
      "Use" (sets active). Add form at the bottom or in a modal.
- [ ] `AiSettingsView::take_pending_switch() -> Option<String>` —
      drained once per frame by the `DesktopApp::ui` loop and
      routed into the worker as
      `Command::SwitchAiProvider { id }`. Mirrors
      `ConnectionsView::take_pending_connect`.
- [ ] AI panel's existing dropdown stub (the single-provider label
      from Stage 1) updates to show the active provider's name when
      multiple are configured. When only one is configured, the
      Settings affordance is still discoverable via the menu.
- [ ] Menu bar: `t!("ai-settings-menu-button")` button next to the
      existing `ai-menu-button`. Visible whenever the binary can
      manage AI settings (i.e. there is a config dir to write
      `ai-providers.toml` to — same gating as
      `connections-window-title`).
- [ ] New Fluent keys for all 11 ADR-0015 locales: at minimum
      `ai-settings-menu-button`, `ai-settings-window-title`,
      `ai-settings-add`, `ai-settings-edit`, `ai-settings-delete`,
      `ai-settings-use`, `ai-settings-active-marker`,
      `ai-settings-empty`, `ai-settings-kind-anthropic`,
      `ai-settings-field-id`, `ai-settings-field-name`,
      `ai-settings-field-model`, `ai-settings-field-api-key`,
      `ai-settings-error-prefix-*` (per `AiSettingsError` variant).
      Tier 1 + Tier 2 in sync (ADR-0022 Consequences rule). Add the
      keys to all 11 `.ftl` files in the same commit.
- [ ] Unit tests on the view state machine: open / close, add form
      validation (empty id / duplicate id / empty API key),
      `take_pending_switch` returns the id and clears the slot, the
      view re-renders after `AiProviderSwitched` reply, the view
      surfaces `AiProviderSwitchFailed` as an inline error. Target:
      ~10 tests matching the density of `connections.rs`.

### Docs

- [ ] `README.md` "AI integration (optional)" subsection refreshed
      to enumerate the env-var / TOML / Settings UI paths and the
      precedence chain.
- [ ] `docs/connections.md` either extended with an "AI providers"
      sibling section, or a new `docs/ai.md` opens — implementer's
      call. Either way, document: where the file lives, schema,
      keychain naming convention (`dbboard.ai.<id>.api_key`,
      service `dbboard`), at-rest posture (`secure_fs` reuse → same
      `0o600` / inherited DACL as `connections.toml`), how to
      migrate from Stage 1 env vars (you don't have to — the env
      var keeps working as the highest-precedence path).
- [ ] `docs/roadmap.md` Phase 4 row: the "Settings UI for API key,
      provider choice" box ticks to `[x]` with a parenthetical
      pointing at ADR-0025 / PR #NN / issue 0008.

### Cross-repo

- [ ] **No outbound brief.** ADR-0025 explicitly stays
      desktop-only. The `0007-web-ai-phase6-no-contract-mirror`
      brief (2026-06-23, PR #33) already established that web's
      Phase 6 ships independently; Group A persistence does not
      change that posture. If implementation surfaces a wire-level
      surprise (it should not), open a fresh `0NNN-web-*` brief
      then.

### Verification (mandatory pre-commit)

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo check --all-targets --all-features`
- [ ] `cargo test --all-features`
- [ ] `cargo build --release` (pre-push)
- [ ] `cargo test --all-features --release` (pre-push)

## Slicing

The ADR records that slicing is the implementer's call. Natural
shape, mirroring how issue 0005 split (a/b → PRs #20/22/24 + #27):

- **Slice (a)**: TOML schema + `AiSettingsAdmin` + `dbboard-server`
  switcher trait + worker variants + `apps/dbboard` desktop
  switcher impl + resolution chain + tests. No UI. The slice
  validates the persistence and switch infrastructure end-to-end
  via integration tests; the AI panel still works (env-var path
  unchanged).
- **Slice (b)**: `AiSettingsView` egui surface + Fluent keys + 11
  locales + menu wiring + docs sweep.

Either slice can ship without the other being merged first
(slice b builds against the live channel variants from slice a
trivially — they're additive). Single-PR landing is also fine if
the implementer prefers; the ADR does not prescribe.

## Out of scope (re-confirmed from ADR-0025 §9 / ADR-0023 §9)

- Streaming (`AiProvider::streaming` accessor + chunked `Reply`
  variants).
- Cancel button + in-flight token budget meter.
- Multi-provider `kind` variants other than `anthropic` — schema
  permits them but no concrete impl ships here. Each follow-up
  provider (`openai`, `ollama`, …) opens its own ADR + issue.
- Conversation history (single-turn stays the Stage 2 surface).
- AI calls recorded in `history.jsonl` — blocked behind a v:2
  schema bump; guarded by
  `0007-web-ai-phase6-no-contract-mirror`'s explicit "do not do
  this without a fresh brief" instruction to web.
- Full DDL extraction on `DatabaseAdapter` (`dump_schema`).
- Function-calling / tool-use provider capability.

## Notes

- Numbering: `0008` follows `0007` in the desktop `.claude/issues/`
  sequence. `0008` is the **first internal issue** since the two
  cross-repo briefs (`0006` + `0007`, PR #33). The web ticket
  numbering (`0010` Aurora DSQL) remains independent.
- This issue does not require a `chore/post-prNN-doc-sync` for the
  ADR itself — the ADR ships alongside this issue in one PR.
  Implementation slice PRs do follow the standard
  `feat → chore post-PR doc-sync` cadence per the
  `[[feedback-keep-docs-fresh]]` memory.
- SemVer impact (ADR-0011): additive per ADR-0025 Consequences.
