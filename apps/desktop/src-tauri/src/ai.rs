//! AI assistant command surface (ADR-0052).
//!
//! The AI layer is the egui client's `ai.rs` + `ai_settings.rs` ported to
//! the Tauri transport. The provider trait and the two concrete providers
//! (`dbboard-ai`, `dbboard-anthropic`, `dbboard-openai`) are reused verbatim
//! — only the *transport* changes: an egui worker channel becomes a set of
//! Tauri commands, and streaming deltas that egui posted as `Reply`s are
//! emitted here as `ai:chunk` events.
//!
//! ## Guardrails (unchanged from egui)
//!
//! - The assistant **never runs SQL and never sees row data.** Explain sends
//!   only the SQL text the user typed; Suggest sends the natural-language
//!   prompt plus the table/column *names* (`list_tables`, and — when the user
//!   opts in — `describe_table` metadata). No `run_read_query` output ever
//!   reaches a provider.
//! - The API key lives **only** in the OS keyring at
//!   `dbboard.ai.<id>.api_key`; it is never written to `ai-providers.toml`,
//!   never logged, and never returned to the WebView.
//! - None of these commands is registered as an MCP tool, so external agents
//!   keep the exact read-only surface (parity with every other write vertical,
//!   ADR-0062).
//!
//! ## Slot / admin / cancel
//!
//! [`AiState`] holds the live provider [`slot`](AiState::slot) (an
//! `RwLock<Option<Arc<dyn AiProvider>>>` the streaming commands clone out of),
//! the optional [`AiSettingsAdmin`] behind a `Mutex` (the write path for
//! `ai-providers.toml` + keyring), the shared [`SecretStore`], and one
//! cancellation flag the in-flight stream polls. Only one AI request runs at a
//! time, so a single flag suffices — a new request clears it first.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use dbboard_ai::{AiProvider, ExplainRequest, StreamEvent, SuggestRequest};
use dbboard_config::secrets::SecretStore;
use dbboard_config::{
    default_ai_providers_path, AiProviderDraft, AiProviderEditDraft, AiProviderKind,
    AiProviderKindDraft, AiProviderKindEditDraft, AiSettingsAdmin, ConnectionKind, SecretField,
};
use dbboard_core::{dialect_for_adapter_id, SqlDialect, TableInfo, TableSchema};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{lock_poisoned, none_if_blank, secret_field, AppState};

/// The live provider handle every streaming command clones out of. A plain
/// `std` `RwLock` (not tokio's) because we only ever hold it long enough to
/// clone the inner `Arc` — never across an `.await`.
type AiProviderSlot = Arc<RwLock<Option<Arc<dyn AiProvider>>>>;

/// The event carrying each streaming delta to the panel. One name, emitted
/// repeatedly for the duration of a single [`ai_explain`]/[`ai_suggest`] call.
const AI_CHUNK_EVENT: &str = "ai:chunk";

/// Managed AI state, a field of [`AppState`].
///
/// `admin` is `None` when the OS reports no per-user config dir or
/// `ai-providers.toml` is unreadable — AI is opt-in, so that degrades to "no
/// provider management" rather than aborting startup (parity with egui's
/// `bootstrap_ai`). `secrets` is the same keyring handle the connection store
/// uses; the `ai.` infix keeps the two namespaces collision-free.
pub(crate) struct AiState {
    pub(crate) slot: AiProviderSlot,
    pub(crate) admin: Option<Arc<Mutex<AiSettingsAdmin>>>,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) cancel: Arc<AtomicBool>,
}

impl AiState {
    /// Stand up the AI layer against the platform `ai-providers.toml` and the
    /// shared keyring (ADR-0052). Resolves the initial provider via the same
    /// precedence chain the egui app uses — env (`DBBOARD_ANTHROPIC_API_KEY`
    /// [+ `DBBOARD_ANTHROPIC_MODEL`]) > TOML `active_id` > none — so a user who
    /// already exports the key keeps working with no TOML entry. Every failure
    /// on the optional layer is logged and degrades to "no provider"; a
    /// misconfigured assistant must never brick launch.
    pub(crate) fn bootstrap(secrets: &Arc<dyn SecretStore>) -> Self {
        let admin: Option<Arc<Mutex<AiSettingsAdmin>>> = match default_ai_providers_path() {
            Ok(path) => match AiSettingsAdmin::open(path, Arc::clone(secrets)) {
                Ok(admin) => Some(Arc::new(Mutex::new(admin))),
                Err(e) => {
                    eprintln!(
                        "dbboard: ai-providers.toml unreadable, AI provider management disabled \
                         (env var fallback still works): {e}"
                    );
                    None
                }
            },
            Err(_) => None,
        };

        let provider = resolve_initial_provider(
            std::env::var("DBBOARD_ANTHROPIC_API_KEY").ok().as_deref(),
            std::env::var("DBBOARD_ANTHROPIC_MODEL").ok().as_deref(),
            admin.as_deref(),
            &**secrets,
        );

        Self {
            slot: Arc::new(RwLock::new(provider)),
            admin,
            secrets: Arc::clone(secrets),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Clone the live provider out of the slot, if one is configured. Held
    /// only long enough to clone the `Arc`, so the guard never spans an await.
    fn provider(&self) -> Option<Arc<dyn AiProvider>> {
        self.slot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Build an [`AiProvider`] from a stored [`AiProviderKind`] by looking up the
/// keyring secret it references. Shared between startup and every provider
/// swap so both agree on how each kind is constructed (parity with egui's
/// `build_provider_for_kind`). A blank stored model falls back to the
/// provider's default model.
fn build_provider_for_kind(
    kind: &AiProviderKind,
    secrets: &dyn SecretStore,
) -> Result<Arc<dyn AiProvider>, String> {
    match kind {
        AiProviderKind::Anthropic {
            model,
            keyring_api_key_ref,
        } => {
            let key = secrets
                .get(keyring_api_key_ref)
                .map_err(|e| format!("api key lookup failed for {keyring_api_key_ref}: {e}"))?;
            let provider = match model.as_deref().filter(|m| !m.trim().is_empty()) {
                Some(m) => dbboard_anthropic::AnthropicProvider::new(key, m),
                None => dbboard_anthropic::AnthropicProvider::with_default_model(key),
            }
            .map_err(|e| e.to_string())?;
            Ok(Arc::new(provider))
        }
        AiProviderKind::OpenAi {
            model,
            keyring_api_key_ref,
        } => {
            let key = secrets
                .get(keyring_api_key_ref)
                .map_err(|e| format!("api key lookup failed for {keyring_api_key_ref}: {e}"))?;
            let provider = match model.as_deref().filter(|m| !m.trim().is_empty()) {
                Some(m) => dbboard_openai::OpenAiProvider::new(key, m),
                None => dbboard_openai::OpenAiProvider::with_default_model(key),
            }
            .map_err(|e| e.to_string())?;
            Ok(Arc::new(provider))
        }
    }
}

/// Precedence chain env > TOML `active_id` > none (ADR-0052). Every failure
/// logs and degrades to `None`.
fn resolve_initial_provider(
    env_api_key: Option<&str>,
    env_model: Option<&str>,
    admin: Option<&Mutex<AiSettingsAdmin>>,
    secrets: &dyn SecretStore,
) -> Option<Arc<dyn AiProvider>> {
    if let Some(key) = env_api_key.map(str::trim).filter(|k| !k.is_empty()) {
        let model = env_model.map(str::trim).filter(|m| !m.is_empty());
        let result = match model {
            Some(m) => dbboard_anthropic::AnthropicProvider::new(key, m),
            None => dbboard_anthropic::AnthropicProvider::with_default_model(key),
        };
        return match result {
            Ok(provider) => Some(Arc::new(provider)),
            Err(e) => {
                eprintln!("dbboard: AI provider init from env failed, assistant disabled: {e}");
                None
            }
        };
    }

    let admin = admin?;
    let kind = {
        let guard = admin.lock().unwrap_or_else(PoisonError::into_inner);
        let id = guard.active_id()?.to_string();
        guard
            .entries()
            .iter()
            .find(|e| e.id == id)
            .map(|entry| entry.kind.clone())
    }?;
    match build_provider_for_kind(&kind, secrets) {
        Ok(provider) => Some(provider),
        Err(e) => {
            eprintln!(
                "dbboard: AI provider init from ai-providers.toml failed, assistant disabled: {e}"
            );
            None
        }
    }
}

/// Map a connection's [`ConnectionKind`] to the lowercase SQL-dialect name the
/// prompt is tagged with (`"sqlite"` / `"postgres"`), the same string egui
/// passes to `AiPanel::prepare_send`. `None` for kinds that map to no dialect.
fn dialect_label_for_kind(kind: &ConnectionKind) -> Option<String> {
    let adapter_id = match kind {
        // Both libSQL forms speak the same dialect; where the database lives
        // is not something the prompt needs to know.
        ConnectionKind::Turso { .. } | ConnectionKind::TursoRemote { .. } => "turso",
        ConnectionKind::D1 { .. } => "d1",
        ConnectionKind::Postgres { .. } => "postgres",
        ConnectionKind::MySql { .. } => "mysql",
        ConnectionKind::Neon { .. } => "neon",
        ConnectionKind::Supabase { .. } => "supabase",
        ConnectionKind::AuroraDsql { .. } | ConnectionKind::AuroraDsqlIam { .. } => "aurora-dsql",
        // Firestore has no SQL dialect at all; `dialect_for_adapter_id`
        // deliberately does not map it, so the prompt stays untagged rather
        // than being told to write SQL against a document store.
        ConnectionKind::Firestore { .. } => "firestore",
        // Same as Firestore: a document store with no SQL dialect to tag.
        ConnectionKind::MongoDb { .. } => "mongodb",
    };
    dialect_for_adapter_id(adapter_id).map(|d| match d {
        SqlDialect::Sqlite => "sqlite".to_string(),
        SqlDialect::Postgres => "postgres".to_string(),
        SqlDialect::MySql => "mysql".to_string(),
    })
}

/// Look up the dialect label for one connection id, reading the kind off the
/// connection store. A missing connection is `None` (the request still runs;
/// the prompt is simply untagged).
fn dialect_for_connection(state: &AppState, connection_id: &str) -> Option<String> {
    let admin = state.admin.lock().ok()?;
    admin
        .entries()
        .iter()
        .find(|e| e.id == connection_id)
        .and_then(|e| dialect_label_for_kind(&e.kind))
}

// --- DTOs -----------------------------------------------------------------

/// Whether the assistant is usable right now, for the panel to gate on. The
/// panel hides its input until `active`, shows `provider_label` as a subtitle,
/// and only offers provider management when `can_manage` (an admin is present).
#[derive(Serialize)]
pub(crate) struct AiStatusDto {
    active: bool,
    provider_label: Option<String>,
    has_streaming: bool,
    can_manage: bool,
}

/// The per-chunk payload of an `ai:chunk` event. `text_delta` is the new text
/// to append; `tokens_in`/`tokens_out` are the *cumulative* counts so far and
/// replace (never sum) the meter — matching the egui `on_stream_chunk` rule.
#[derive(Serialize, Clone)]
struct AiChunkDto {
    text_delta: String,
    tokens_in: u32,
    tokens_out: u32,
}

/// The terminal result of a single explain/suggest call. `text` is the whole
/// answer (the chunks were only for live rendering); `cancelled` means the
/// user stopped it mid-stream and `text` holds the partial answer.
/// `prefetch_warnings` counts tables whose `describe_table` failed during a
/// Suggest with "include column details" — non-blocking (ADR-0028).
#[derive(Serialize)]
pub(crate) struct AiOutcomeDto {
    text: String,
    tokens_in: u32,
    tokens_out: u32,
    cancelled: bool,
    prefetch_warnings: u32,
}

/// One configured provider, for the management list. Never carries the api key
/// (it lives only in the keyring); `active` marks the one the assistant uses.
#[derive(Serialize)]
pub(crate) struct AiProviderView {
    id: String,
    name: String,
    kind: String,
    model: Option<String>,
    active: bool,
}

/// Add/edit-time kind + inline key, as the provider form submits it. `model`
/// blank ⇒ the provider's default model. On edit, a blank `api_key` keeps the
/// stored secret (parity with the connection form, ADR-0016).
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AiKindInput {
    Anthropic {
        model: Option<String>,
        api_key: Option<String>,
    },
    #[serde(rename = "openai")]
    OpenAi {
        model: Option<String>,
        api_key: Option<String>,
    },
}

// --- Streaming accumulator (pure) -----------------------------------------

/// Running totals for one in-flight stream. Kept as a small struct so the
/// fold from `StreamEvent` to the terminal [`AiOutcomeDto`] is unit-testable
/// without a live provider or `AppHandle`.
#[derive(Default)]
struct StreamAcc {
    text: String,
    tokens_in: u32,
    tokens_out: u32,
}

impl StreamAcc {
    /// Fold one event in, returning the delta text to emit (if any). Usage and
    /// MessageStart carry no visible text but update the meter; a TextDelta
    /// appends and echoes. Cumulative token counts *replace*, never sum.
    fn apply(&mut self, event: &StreamEvent) -> Option<String> {
        match event {
            StreamEvent::MessageStart { tokens_in } => {
                self.tokens_in = *tokens_in;
                None
            }
            StreamEvent::TextDelta(delta) => {
                self.text.push_str(delta);
                Some(delta.clone())
            }
            StreamEvent::Usage {
                tokens_in,
                tokens_out,
            } => {
                self.tokens_in = *tokens_in;
                self.tokens_out = *tokens_out;
                None
            }
            StreamEvent::MessageStop { .. } | StreamEvent::Error(_) => None,
        }
    }
}

/// Drive a provider stream to completion, emitting an `ai:chunk` per event and
/// polling the shared cancel flag between events. Shared by explain and
/// suggest — the only difference upstream is which request built the stream.
async fn drive_stream(
    app: &AppHandle,
    provider: Arc<dyn AiProvider>,
    mut stream: dbboard_ai::AiStream,
    cancel: &AtomicBool,
    prefetch_warnings: u32,
) -> Result<AiOutcomeDto, String> {
    // A fresh request must never inherit a stale cancel from a prior run.
    cancel.store(false, Ordering::SeqCst);
    let _ = provider; // keep the provider alive for the stream's lifetime.

    let mut acc = StreamAcc::default();
    let mut cancelled = false;
    while let Some(event) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        let event = event.map_err(|e| e.to_string())?;
        if let StreamEvent::Error(e) = &event {
            // A provider-signalled mid-stream error is terminal: surface it
            // rather than returning a half-answer as success.
            return Err(e.to_string());
        }
        let delta = acc.apply(&event);
        // Emit on every event so the meter advances even on token-only Usage
        // events; a failed emit just means no window is listening.
        let _ = app.emit(
            AI_CHUNK_EVENT,
            AiChunkDto {
                text_delta: delta.unwrap_or_default(),
                tokens_in: acc.tokens_in,
                tokens_out: acc.tokens_out,
            },
        );
        if matches!(event, StreamEvent::MessageStop { .. }) {
            break;
        }
    }

    Ok(AiOutcomeDto {
        text: acc.text,
        tokens_in: acc.tokens_in,
        tokens_out: acc.tokens_out,
        cancelled,
        prefetch_warnings,
    })
}

// --- Commands -------------------------------------------------------------

/// Whether the assistant is usable and, if so, its provider label + streaming
/// capability. Pure read of the slot; safe to call on every panel open.
#[tauri::command]
pub(crate) fn ai_status(state: tauri::State<'_, AppState>) -> AiStatusDto {
    let provider = state.ai.provider();
    let (active, provider_label, has_streaming) = match &provider {
        Some(p) => {
            let (name, model) = p.identity();
            let label = if model.trim().is_empty() {
                name.to_string()
            } else {
                format!("{name} · {model}")
            };
            (true, Some(label), p.capabilities().has_streaming)
        }
        None => (false, None, false),
    };
    AiStatusDto {
        active,
        provider_label,
        has_streaming,
        can_manage: state.ai.admin.is_some(),
    }
}

/// Explain the SQL the user typed. Sends only the SQL text (+ the connection's
/// dialect, when known) — never schema, never row data. Streams `ai:chunk`
/// events and returns the whole answer.
#[tauri::command]
pub(crate) async fn ai_explain(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    connection_id: Option<String>,
    sql: String,
) -> Result<AiOutcomeDto, String> {
    let provider = state
        .ai
        .provider()
        .ok_or_else(|| "no AI provider is configured".to_string())?;
    let dialect = connection_id
        .as_deref()
        .and_then(|id| dialect_for_connection(&state, id));
    let stream = provider
        .stream_explain(&ExplainRequest { sql, dialect })
        .await
        .map_err(|e| e.to_string())?;
    drive_stream(&app, Arc::clone(&provider), stream, &state.ai.cancel, 0).await
}

/// Draft SQL from a natural-language prompt. Sends the prompt, the dialect, and
/// the table/column *names* from `list_tables`; when `include_details` is set,
/// also fans out `describe_table` for column-level metadata (names/types/PK —
/// still no row data). A table whose describe fails is skipped, counted, and
/// reported as a non-blocking warning (ADR-0028). Streams `ai:chunk` events.
#[tauri::command]
pub(crate) async fn ai_suggest(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    connection_id: String,
    prompt: String,
    include_details: bool,
) -> Result<AiOutcomeDto, String> {
    let provider = state
        .ai
        .provider()
        .ok_or_else(|| "no AI provider is configured".to_string())?;
    let dialect = dialect_for_connection(&state, &connection_id);

    let schema: Vec<TableInfo> = state
        .service
        .list_tables(&connection_id)
        .await
        .map_err(|e| e.to_string())?;

    // ADR-0028: the describe fan-out only runs when the user asked for details
    // *and* there are tables to describe; an empty schema skips the round-trip.
    let mut prefetch_warnings = 0u32;
    let full_schema: Option<Vec<TableSchema>> = if include_details && !schema.is_empty() {
        let mut described: Vec<TableSchema> = Vec::with_capacity(schema.len());
        for table in &schema {
            match state
                .service
                .describe_table(&connection_id, table.schema.as_deref(), &table.name)
                .await
            {
                Ok(ts) => described.push(ts),
                // Partial failure is non-blocking: continue without this table.
                Err(_) => prefetch_warnings += 1,
            }
        }
        Some(described)
    } else {
        None
    };

    let stream = provider
        .stream_suggest_sql(&SuggestRequest {
            prompt,
            dialect,
            schema,
            full_schema,
        })
        .await
        .map_err(|e| e.to_string())?;
    drive_stream(
        &app,
        Arc::clone(&provider),
        stream,
        &state.ai.cancel,
        prefetch_warnings,
    )
    .await
}

/// Request cancellation of the in-flight AI request. Flips the shared flag the
/// running stream polls between events; the stream stops at the next event and
/// returns a `cancelled` outcome carrying whatever text arrived so far.
#[tauri::command]
pub(crate) fn cancel_ai(state: tauri::State<'_, AppState>) {
    state.ai.cancel.store(true, Ordering::SeqCst);
}

// --- Provider management --------------------------------------------------

/// Borrow the admin or return the "no store" error every management command
/// shares. `None` means the OS reported no config dir or the TOML was
/// unreadable at startup.
fn admin_handle(state: &AppState) -> Result<&Arc<Mutex<AiSettingsAdmin>>, String> {
    state
        .ai
        .admin
        .as_ref()
        .ok_or_else(|| "AI provider storage is unavailable on this host".to_string())
}

/// List every configured provider (id / name / kind / model / active). Never
/// includes the api key.
#[tauri::command]
pub(crate) fn list_ai_providers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AiProviderView>, String> {
    let admin = admin_handle(&state)?.lock().map_err(|_| lock_poisoned())?;
    let active = admin.active_id().map(str::to_string);
    Ok(admin
        .entries()
        .iter()
        .map(|e| provider_view(e, active.as_deref()))
        .collect())
}

/// Add a provider: writes the non-secret entry to `ai-providers.toml` and the
/// key to the keyring atomically (rolled back together on failure). Does not
/// auto-activate — the user picks "Use" to switch to it (parity with egui).
#[tauri::command]
pub(crate) fn add_ai_provider(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    kind: AiKindInput,
) -> Result<(), String> {
    let draft = AiProviderDraft {
        id,
        name,
        kind: to_add_kind(kind)?,
    };
    let admin = admin_handle(&state)?;
    let mut guard = admin.lock().map_err(|_| lock_poisoned())?;
    guard.add(draft).map(|_| ()).map_err(|e| e.to_string())
}

/// Edit a provider. The id and kind are immutable (a kind change is a delete +
/// re-add); a blank api key keeps the stored one. If the edited provider is the
/// active one, the live slot is rebuilt so the change takes effect immediately.
#[tauri::command]
pub(crate) fn update_ai_provider(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<(), String> {
    let admin = admin_handle(&state)?;
    {
        let mut guard = admin.lock().map_err(|_| lock_poisoned())?;
        // The kind is fixed by the stored entry — the form only edits name /
        // model / key. Build the matching edit-draft variant from it.
        let existing_kind = guard
            .entries()
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.kind.clone())
            .ok_or_else(|| format!("no AI provider with id \"{id}\""))?;
        let edit = AiProviderEditDraft {
            name,
            kind: to_edit_kind(&existing_kind, none_if_blank(model), secret_field(api_key)),
        };
        guard.update(&id, edit).map_err(|e| e.to_string())?;
    }
    // A stored-secret or model change on the *active* provider must reach the
    // live slot; a resync failure is logged, not fatal (the TOML write already
    // succeeded, so the rename must not report failure).
    resync_active_slot(&state.ai, admin);
    Ok(())
}

/// Delete a provider and purge its keyring secret. If it was the active one,
/// the admin clears `active_id`, so the live slot is cleared to match.
#[tauri::command]
pub(crate) fn delete_ai_provider(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let admin = admin_handle(&state)?;
    {
        let mut guard = admin.lock().map_err(|_| lock_poisoned())?;
        guard.delete(&id).map_err(|e| e.to_string())?;
    }
    resync_active_slot(&state.ai, admin);
    Ok(())
}

/// Activate a provider (or clear the active one with `id = None`). Building the
/// provider comes *first*: a bad key fails the command with `active_id`
/// untouched, so a broken activation can never leave the panel pointing at a
/// provider that will error on every send. On success the slot is swapped
/// before the new `active_id` is persisted (a slot that works but fails to
/// persist beats a TOML that records an id we never wired).
#[tauri::command]
pub(crate) fn set_active_ai_provider(
    state: tauri::State<'_, AppState>,
    id: Option<String>,
) -> Result<(), String> {
    let admin = admin_handle(&state)?;

    let Some(id) = id else {
        // Clear: drop the live provider, then persist the empty active id.
        *state
            .ai
            .slot
            .write()
            .unwrap_or_else(PoisonError::into_inner) = None;
        let mut guard = admin.lock().map_err(|_| lock_poisoned())?;
        return guard.set_active(None).map_err(|e| e.to_string());
    };

    let kind = {
        let guard = admin.lock().map_err(|_| lock_poisoned())?;
        guard
            .entries()
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.kind.clone())
            .ok_or_else(|| format!("no AI provider with id \"{id}\""))?
    };
    let provider = build_provider_for_kind(&kind, &*state.ai.secrets)?;
    *state
        .ai
        .slot
        .write()
        .unwrap_or_else(PoisonError::into_inner) = Some(provider);

    let mut guard = admin.lock().map_err(|_| lock_poisoned())?;
    if let Err(e) = guard.set_active(Some(id.clone())) {
        // The swap already took effect this session; a persist failure only
        // affects which provider the *next* launch picks. Log and proceed.
        eprintln!(
            "dbboard: activated AI provider '{id}' in memory, but persisting active_id failed: {e}"
        );
    }
    Ok(())
}

/// Rebuild the live slot to match the admin's current `active_id`, best-effort.
/// Used after an edit/delete that may have touched the active provider. A build
/// failure logs and leaves the slot as-is rather than failing the caller.
fn resync_active_slot(ai: &AiState, admin: &Arc<Mutex<AiSettingsAdmin>>) {
    let kind = {
        let guard = admin.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(id) = guard.active_id().map(str::to_string) else {
            // No active provider ⇒ the slot should be empty too.
            *ai.slot.write().unwrap_or_else(PoisonError::into_inner) = None;
            return;
        };
        guard
            .entries()
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.kind.clone())
    };
    let Some(kind) = kind else { return };
    match build_provider_for_kind(&kind, &*ai.secrets) {
        Ok(provider) => {
            *ai.slot.write().unwrap_or_else(PoisonError::into_inner) = Some(provider);
        }
        Err(e) => eprintln!("dbboard: could not rebuild active AI provider after edit: {e}"),
    }
}

/// Project one stored entry to the non-secret [`AiProviderView`].
fn provider_view(entry: &dbboard_config::AiProviderEntry, active: Option<&str>) -> AiProviderView {
    let (kind, model) = match &entry.kind {
        AiProviderKind::Anthropic { model, .. } => ("anthropic", model.clone()),
        AiProviderKind::OpenAi { model, .. } => ("openai", model.clone()),
    };
    AiProviderView {
        id: entry.id.clone(),
        name: entry.name.clone(),
        kind: kind.to_string(),
        model,
        active: active == Some(entry.id.as_str()),
    }
}

/// Map the add form's kind DTO to the config draft. An add requires the key,
/// so a blank/absent key is rejected here rather than storing an empty secret.
fn to_add_kind(kind: AiKindInput) -> Result<AiProviderKindDraft, String> {
    let require_key = |api_key: Option<String>| -> Result<String, String> {
        match api_key {
            Some(k) if !k.trim().is_empty() => Ok(k),
            _ => Err("an API key is required to add a provider".to_string()),
        }
    };
    Ok(match kind {
        AiKindInput::Anthropic { model, api_key } => AiProviderKindDraft::Anthropic {
            model: none_if_blank(model),
            api_key: require_key(api_key)?,
        },
        AiKindInput::OpenAi { model, api_key } => AiProviderKindDraft::OpenAi {
            model: none_if_blank(model),
            api_key: require_key(api_key)?,
        },
    })
}

/// Build the edit-draft kind from the *stored* kind (the discriminator is
/// immutable) plus the form's new model and secret field.
fn to_edit_kind(
    stored: &AiProviderKind,
    model: Option<String>,
    api_key: SecretField,
) -> AiProviderKindEditDraft {
    match stored {
        AiProviderKind::Anthropic { .. } => AiProviderKindEditDraft::Anthropic { model, api_key },
        AiProviderKind::OpenAi { .. } => AiProviderKindEditDraft::OpenAi { model, api_key },
    }
}

#[cfg(test)]
mod tests {
    //! The commands are thin wrappers over `AiSettingsAdmin` (its keyring/TOML
    //! rollback is covered by `dbboard-config`) and over the provider stream
    //! (covered by `dbboard-ai`/`dbboard-anthropic`). What *this* module owns
    //! is the wiring: dialect derivation, the stream fold, the JSON shapes the
    //! panel parses, and the add/edit DTO mapping. These pin exactly that.
    use super::*;
    use dbboard_ai::{AiError, StopReason};

    fn anthropic_entry(id: &str, model: Option<&str>) -> dbboard_config::AiProviderEntry {
        dbboard_config::AiProviderEntry {
            id: id.to_string(),
            name: format!("{id} name"),
            kind: AiProviderKind::Anthropic {
                model: model.map(str::to_string),
                keyring_api_key_ref: format!("dbboard.ai.{id}.api_key"),
            },
        }
    }

    #[test]
    fn dialect_label_maps_each_kind_to_its_sql_dialect() {
        assert_eq!(
            dialect_label_for_kind(&ConnectionKind::Turso {
                path: ":memory:".into()
            }),
            Some("sqlite".to_string())
        );
        assert_eq!(
            dialect_label_for_kind(&ConnectionKind::Postgres {
                keyring_url_ref: "r".into()
            })
            .as_deref(),
            Some("postgres")
        );
        assert_eq!(
            dialect_label_for_kind(&ConnectionKind::Neon {
                keyring_url_ref: "r".into()
            })
            .as_deref(),
            Some("postgres")
        );
    }

    #[test]
    fn stream_acc_appends_text_and_replaces_cumulative_tokens() {
        let mut acc = StreamAcc::default();
        assert_eq!(
            acc.apply(&StreamEvent::MessageStart { tokens_in: 10 }),
            None
        );
        assert_eq!(acc.tokens_in, 10);

        assert_eq!(
            acc.apply(&StreamEvent::TextDelta("SELECT ".into()))
                .as_deref(),
            Some("SELECT ")
        );
        assert_eq!(
            acc.apply(&StreamEvent::TextDelta("1".into())).as_deref(),
            Some("1")
        );
        assert_eq!(acc.text, "SELECT 1");

        // Cumulative usage REPLACES, never sums.
        acc.apply(&StreamEvent::Usage {
            tokens_in: 10,
            tokens_out: 3,
        });
        acc.apply(&StreamEvent::Usage {
            tokens_in: 10,
            tokens_out: 7,
        });
        assert_eq!(acc.tokens_out, 7, "later Usage replaces the earlier count");
        assert_eq!(acc.tokens_in, 10);

        // Terminal events add no visible text.
        assert_eq!(
            acc.apply(&StreamEvent::MessageStop {
                stop_reason: StopReason::EndTurn
            }),
            None
        );
    }

    #[test]
    fn stream_acc_treats_a_mid_stream_error_as_no_text() {
        let mut acc = StreamAcc::default();
        assert_eq!(
            acc.apply(&StreamEvent::Error(AiError::Network("boom".into()))),
            None
        );
        assert!(acc.text.is_empty());
    }

    #[test]
    fn provider_view_hides_the_key_and_flags_the_active_entry() {
        let entry = anthropic_entry("a", Some("claude-sonnet-5"));
        let view = provider_view(&entry, Some("a"));
        assert_eq!(view.kind, "anthropic");
        assert_eq!(view.model.as_deref(), Some("claude-sonnet-5"));
        assert!(view.active);

        let json = serde_json::to_value(&view).expect("serialize");
        // The panel keys off exactly these fields; a key must never appear.
        assert!(json.get("id").is_some());
        assert!(json.get("model").is_some());
        assert!(json.get("active").is_some());
        assert!(json.get("api_key").is_none(), "secret must never serialize");
        assert!(json.get("keyring_api_key_ref").is_none());

        // A non-matching active id leaves the entry inactive.
        assert!(!provider_view(&anthropic_entry("b", None), Some("a")).active);
    }

    #[test]
    fn to_add_kind_requires_a_non_blank_key() {
        let ok = to_add_kind(AiKindInput::Anthropic {
            model: Some("  ".into()),
            api_key: Some("sk-123".into()),
        })
        .expect("a keyed anthropic draft");
        match ok {
            AiProviderKindDraft::Anthropic { model, api_key } => {
                assert!(model.is_none(), "a blank model collapses to the default");
                assert_eq!(api_key, "sk-123");
            }
            _ => panic!("expected an anthropic draft"),
        }

        assert!(
            to_add_kind(AiKindInput::OpenAi {
                model: None,
                api_key: Some("   ".into()),
            })
            .is_err(),
            "a blank key must be rejected, never stored empty"
        );
        assert!(to_add_kind(AiKindInput::OpenAi {
            model: None,
            api_key: None,
        })
        .is_err());
    }

    #[test]
    fn to_edit_kind_keeps_the_stored_discriminator() {
        let stored = AiProviderKind::OpenAi {
            model: Some("gpt-4o".into()),
            keyring_api_key_ref: "r".into(),
        };
        // Even though the form only sends model+key, the kind stays OpenAi.
        match to_edit_kind(&stored, Some("gpt-4o-mini".into()), SecretField::Keep) {
            AiProviderKindEditDraft::OpenAi { model, api_key } => {
                assert_eq!(model.as_deref(), Some("gpt-4o-mini"));
                assert!(matches!(api_key, SecretField::Keep));
            }
            _ => panic!("edit must not switch the provider kind"),
        }
    }

    #[test]
    fn ai_status_dto_keeps_its_frontend_json_shape() {
        let dto = AiStatusDto {
            active: true,
            provider_label: Some("Anthropic · claude-sonnet-5".into()),
            has_streaming: true,
            can_manage: true,
        };
        let json = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(json.get("active").and_then(|v| v.as_bool()), Some(true));
        assert!(json.get("provider_label").is_some());
        assert!(json.get("has_streaming").is_some());
        assert!(json.get("can_manage").is_some());
    }

    #[test]
    fn ai_outcome_dto_keeps_its_frontend_json_shape() {
        let dto = AiOutcomeDto {
            text: "SELECT 1".into(),
            tokens_in: 12,
            tokens_out: 3,
            cancelled: false,
            prefetch_warnings: 2,
        };
        let json = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(json.get("text").and_then(|v| v.as_str()), Some("SELECT 1"));
        assert_eq!(json.get("tokens_in").and_then(|v| v.as_u64()), Some(12));
        assert_eq!(json.get("cancelled").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            json.get("prefetch_warnings").and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn ai_kind_input_parses_the_openai_discriminator() {
        // The frontend sends `kind: "openai"` (not "open_ai"); the rename must
        // hold or every OpenAI add/edit would fail to deserialize.
        let json = serde_json::json!({ "kind": "openai", "model": null, "api_key": "sk" });
        let parsed: AiKindInput = serde_json::from_value(json).expect("parse openai");
        assert!(matches!(parsed, AiKindInput::OpenAi { .. }));

        let json = serde_json::json!({ "kind": "anthropic", "model": "m", "api_key": "sk" });
        let parsed: AiKindInput = serde_json::from_value(json).expect("parse anthropic");
        assert!(matches!(parsed, AiKindInput::Anthropic { .. }));
    }
}
