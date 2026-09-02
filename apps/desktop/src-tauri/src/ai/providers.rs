//! Configuring which provider the assistant uses (ADR-0052).
//!
//! The write path for `ai-providers.toml`, kept apart from the streaming
//! commands next door because the two share almost nothing: these take the
//! admin lock and touch the keyring, those clone an `Arc` out of the slot and
//! never write. What joins them is the slot itself — an edit or a delete can
//! invalidate the provider a stream is about to use, so every command here
//! ends by resyncing it.
//!
//! The api key is write-only throughout: it enters through the form, goes to
//! the keyring, and is never projected back out (ADR-0016).

use std::sync::{Arc, Mutex, PoisonError};

use dbboard_config::{
    AiProviderDraft, AiProviderEditDraft, AiProviderKind, AiProviderKindDraft,
    AiProviderKindEditDraft, AiSettingsAdmin, SecretField,
};
use serde::Serialize;

use crate::connections::input::secret_field;
use crate::{lock_poisoned, none_if_blank, AppState};

use super::{build_provider_for_kind, AiState};

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
    use super::*;

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
