//! AI provider Settings window (ADR-0025 Phase 4 Stage 2 Group A
//! slice (b)).
//!
//! Mirror of [`crate::connections`] for the AI provider store. Same
//! mental model: this window manages the persisted entries in
//! `ai-providers.toml` (Add / Edit / Delete) and the per-row "Use"
//! button asks the host to switch the active provider in-process — the
//! analogue of the connection window's "Connect" button (ADR-0020 /
//! ADR-0025).
//!
//! Unlike connections, the active AI provider can be swapped without
//! restarting the binary: the worker's [`crate::Command::SwitchAiProvider`]
//! flows through the binary-supplied
//! [`crate::worker::AiProviderSwitcher`] and updates the
//! [`crate::worker::AiProviderSlot`] in place. A successful switch lands
//! as [`crate::Reply::AiProviderSwitched`]; the AI panel reads the slot
//! directly on every frame, so a swap reveals or refreshes the panel
//! without any further plumbing.
//!
//! The view is split into a small, easily-testable state machine
//! ([`AiSettingsView`] + [`Mode`]) and a thin egui rendering layer
//! ([`AiSettingsView::ui`]). The state machine is covered by unit tests
//! against an in-memory [`AiSettingsAdmin`]; the egui code path is
//! exercised end-to-end at the binary level only.

use dbboard_config::{
    AiProviderDraft, AiProviderEditDraft, AiProviderEntry, AiProviderKind, AiProviderKindDraft,
    AiProviderKindEditDraft, AiSettingsAdmin, AiSettingsError, SecretField,
};
use dbboard_i18n::t;
use eframe::egui;

use crate::errors::{self, ai_settings_error_display, DisplayError};

/// AI provider Settings window. Lives next to [`crate::DbboardApp`] in
/// the binary and is shown when the user opens it from the top bar.
#[derive(Debug)]
pub struct AiSettingsView {
    is_open: bool,
    mode: Mode,
    /// Last error from a failed submit, surfaced inline above the form
    /// buttons. Cleared on every successful submit or mode transition.
    /// Carries the localized message and the original English so the
    /// inline banner can show and copy both (ADR-0039).
    last_error: Option<DisplayError>,
    /// Id of a provider the user just asked to make active via the
    /// per-row "Use" button. Drained by the host (typically `DesktopApp`)
    /// via [`Self::take_pending_switch`] after every `ui()` call and
    /// turned into [`crate::DbboardApp::switch_ai_provider`]. Holds at
    /// most one id; a second click before the host drains overwrites the
    /// first.
    pending_switch: Option<String>,
}

impl Default for AiSettingsView {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutually exclusive states the AI Settings window can be in.
#[derive(Debug, Clone)]
pub enum Mode {
    /// List of entries with Add / Edit / Delete / Use buttons per row.
    List,
    /// New-entry form. Stage 2 ships only the Anthropic variant; the
    /// form is shaped for that kind directly.
    Add(AddFormState),
    /// Edit-an-existing-entry form. The id is read-only (it is the
    /// primary key of every keyring reference) and is shown as a
    /// disabled field.
    Edit { id: String, form: EditFormState },
    /// "Are you sure?" confirmation prompt before a destructive delete.
    /// Shows the entry's display name to reduce mis-clicks.
    ConfirmDelete { id: String, name: String },
}

/// Backing buffers for the Add form. Single kind (Anthropic) in
/// Stage 2; additional variants land as additive fields when their
/// provider crates ship.
#[derive(Debug, Default, Clone)]
pub struct AddFormState {
    pub id: String,
    pub name: String,
    /// Empty string maps to `None` — fall back to the provider crate's
    /// compile-time default model (ADR-0025 Decision 8).
    pub model: String,
    pub api_key: String,
}

/// Backing buffers for the Edit form. The id is held outside this struct
/// (on [`Mode::Edit`]) because it is not user-editable.
///
/// `replace_api_key` is the explicit opt-in: leaving it unticked keeps
/// the existing keyring entry untouched (same shape as the connections
/// window's `replace_token` / `replace_url`, ADR-0016 §3 — secrets are
/// write-only).
#[derive(Debug, Clone)]
pub struct EditFormState {
    pub name: String,
    pub model: String,
    pub replace_api_key: bool,
    pub new_api_key: String,
}

impl AiSettingsView {
    /// Construct a fresh, closed view in `List` mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: Mode::List,
            last_error: None,
            pending_switch: None,
        }
    }

    /// Show the window. Reading the entries on next frame happens
    /// inside [`Self::ui`]; nothing else changes here.
    pub fn open(&mut self) {
        self.is_open = true;
    }

    /// Hide the window. Form state is preserved so re-opening returns
    /// the user to where they were.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// Last error message produced by a failed submit, if any. The UI
    /// renders this above the action buttons.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_ref().map(DisplayError::localized)
    }

    /// Record a click on the per-row "Use" button. The host drains the
    /// value with [`Self::take_pending_switch`] after every `ui()` call.
    /// A repeat click before the host drains overwrites the previous id
    /// — only the most recent intent is honoured.
    pub fn request_use(&mut self, id: &str) {
        self.pending_switch = Some(id.to_string());
    }

    /// Drain a pending "Use" click, if any. Returns the id once and then
    /// resets to `None`, so the host should call this on every frame and
    /// forward the result into
    /// [`crate::DbboardApp::switch_ai_provider`].
    pub fn take_pending_switch(&mut self) -> Option<String> {
        self.pending_switch.take()
    }

    /// Switch to the Add form with empty fields.
    pub fn start_add(&mut self) {
        self.mode = Mode::Add(AddFormState::default());
        self.last_error = None;
    }

    /// Switch to the Edit form pre-filled from `entry`. The api-key
    /// `replace` toggle starts unticked so an unrelated `name`-only edit
    /// does not need to re-type the secret.
    pub fn start_edit(&mut self, entry: &AiProviderEntry) {
        self.mode = Mode::Edit {
            id: entry.id.clone(),
            form: EditFormState::from_entry(entry),
        };
        self.last_error = None;
    }

    /// Switch to the delete confirmation prompt for `entry`.
    pub fn start_delete(&mut self, entry: &AiProviderEntry) {
        self.mode = Mode::ConfirmDelete {
            id: entry.id.clone(),
            name: entry.name.clone(),
        };
        self.last_error = None;
    }

    /// Cancel whatever form is currently shown and return to `List`.
    pub fn cancel(&mut self) {
        self.mode = Mode::List;
        self.last_error = None;
    }

    /// Build an [`AiProviderDraft`] from the current Add form and route
    /// it through `admin`. On success the form is closed and the view
    /// returns to `List`; on failure the error is stored in
    /// [`Self::last_error`] and the form stays open so the user can
    /// retry.
    ///
    /// # Errors
    ///
    /// Any [`AiSettingsError`] from [`AiSettingsAdmin::add`] is
    /// forwarded.
    pub fn submit_add(&mut self, admin: &mut AiSettingsAdmin) -> Result<(), AiSettingsError> {
        let Mode::Add(form) = &self.mode else {
            return Ok(());
        };
        let draft = form.to_draft();
        match admin.add(draft) {
            Ok(_) => {
                self.mode = Mode::List;
                self.last_error = None;
                Ok(())
            }
            Err(err) => {
                self.last_error = Some(ai_settings_error_display(&err));
                Err(err)
            }
        }
    }

    /// Build an [`AiProviderEditDraft`] from the current Edit form and
    /// route it through `admin`. Behaves like [`Self::submit_add`] on
    /// success / failure.
    ///
    /// # Errors
    ///
    /// Any [`AiSettingsError`] from [`AiSettingsAdmin::update`] is
    /// forwarded.
    pub fn submit_edit(&mut self, admin: &mut AiSettingsAdmin) -> Result<(), AiSettingsError> {
        let Mode::Edit { id, form } = &self.mode else {
            return Ok(());
        };
        let draft = form.to_draft();
        match admin.update(id, draft) {
            Ok(_) => {
                self.mode = Mode::List;
                self.last_error = None;
                Ok(())
            }
            Err(err) => {
                self.last_error = Some(ai_settings_error_display(&err));
                Err(err)
            }
        }
    }

    /// Commit the delete confirmation. Behaves like [`Self::submit_add`]
    /// on success / failure.
    ///
    /// # Errors
    ///
    /// Any [`AiSettingsError`] from [`AiSettingsAdmin::delete`] is
    /// forwarded.
    pub fn submit_delete(&mut self, admin: &mut AiSettingsAdmin) -> Result<(), AiSettingsError> {
        let Mode::ConfirmDelete { id, .. } = &self.mode else {
            return Ok(());
        };
        let id_owned = id.clone();
        match admin.delete(&id_owned) {
            Ok(()) => {
                self.mode = Mode::List;
                self.last_error = None;
                Ok(())
            }
            Err(err) => {
                self.last_error = Some(ai_settings_error_display(&err));
                Err(err)
            }
        }
    }

    /// Render the window into `ctx`. No-op when closed.
    ///
    /// Holds a `&mut AiSettingsAdmin` for the duration of the call; the
    /// caller is responsible for guarding shared access (typically
    /// `Arc<Mutex<AiSettingsAdmin>>` in the desktop binary).
    ///
    /// `active_id` is the id currently bound to the running AI provider
    /// slot (ADR-0025). The active row is marked and its Use button is
    /// disabled to suppress no-op re-swaps.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        admin: &mut AiSettingsAdmin,
        active_id: Option<&str>,
    ) {
        if !self.is_open {
            return;
        }
        let mut is_open = self.is_open;
        egui::Window::new(t!("ai-settings-window-title"))
            .open(&mut is_open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                self.render(ui, admin, active_id);
            });
        self.is_open = is_open;
    }

    fn render(&mut self, ui: &mut egui::Ui, admin: &mut AiSettingsAdmin, active_id: Option<&str>) {
        match &mut self.mode {
            Mode::List => {
                Self::render_list(
                    ui,
                    admin,
                    &mut self.mode,
                    &mut self.pending_switch,
                    active_id,
                );
            }
            Mode::Add(form) => {
                render_add_form(ui, form);
                errors::render_error(ui, self.last_error.as_ref());
                let (submit_btn, cancel_btn) = render_form_buttons(ui);
                if submit_btn {
                    let _ = self.submit_add(admin);
                } else if cancel_btn {
                    self.cancel();
                }
            }
            Mode::Edit { id, form } => {
                render_edit_form(ui, id, form);
                errors::render_error(ui, self.last_error.as_ref());
                let (submit_btn, cancel_btn) = render_form_buttons(ui);
                if submit_btn {
                    let _ = self.submit_edit(admin);
                } else if cancel_btn {
                    self.cancel();
                }
            }
            Mode::ConfirmDelete { id: _, name } => {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("{}: {name}", t!("ai-settings-confirm-delete")),
                );
                errors::render_error(ui, self.last_error.as_ref());
                ui.horizontal(|ui| {
                    if ui.button(t!("ai-settings-delete-button")).clicked() {
                        let _ = self.submit_delete(admin);
                    }
                    if ui.button(t!("ai-settings-cancel-button")).clicked() {
                        self.cancel();
                    }
                });
            }
        }
    }

    fn render_list(
        ui: &mut egui::Ui,
        admin: &mut AiSettingsAdmin,
        mode: &mut Mode,
        pending_switch: &mut Option<String>,
        active_id: Option<&str>,
    ) {
        if ui.button(t!("ai-settings-add-button")).clicked() {
            *mode = Mode::Add(AddFormState::default());
            return;
        }
        ui.separator();

        // Snapshot so we can borrow admin mutably below in response to
        // the per-row buttons.
        let entries: Vec<AiProviderEntry> = admin.entries().to_vec();
        if entries.is_empty() {
            ui.label(t!("ai-settings-list-empty"));
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &entries {
                ui.horizontal(|ui| {
                    let is_active = active_id == Some(entry.id.as_str());
                    let label = if is_active {
                        format!(
                            "{} ({}) {}",
                            entry.name,
                            kind_label(&entry.kind),
                            t!("ai-settings-active-marker")
                        )
                    } else {
                        format!("{} ({})", entry.name, kind_label(&entry.kind))
                    };
                    ui.label(label);
                    // The Use button is the user-facing entry point for
                    // an in-process AI provider swap (ADR-0025). The
                    // active row's button is disabled — re-clicking it
                    // would only rebuild the same provider we already
                    // have live.
                    if ui
                        .add_enabled(
                            !is_active,
                            egui::Button::new(t!("ai-settings-use-button")).small(),
                        )
                        .clicked()
                    {
                        *pending_switch = Some(entry.id.clone());
                    }
                    if ui.small_button(t!("ai-settings-edit-button")).clicked() {
                        *mode = Mode::Edit {
                            id: entry.id.clone(),
                            form: EditFormState::from_entry(entry),
                        };
                    }
                    if ui.small_button(t!("ai-settings-delete-button")).clicked() {
                        *mode = Mode::ConfirmDelete {
                            id: entry.id.clone(),
                            name: entry.name.clone(),
                        };
                    }
                });
            }
        });
    }
}

impl AddFormState {
    /// Translate the form's freeform string buffers into a strongly-
    /// typed [`AiProviderDraft`] that [`AiSettingsAdmin::add`] will
    /// accept. `model` is `None` when the field is empty, matching the
    /// TOML schema's `#[serde(skip_serializing_if = "Option::is_none")]`.
    #[must_use]
    pub fn to_draft(&self) -> AiProviderDraft {
        AiProviderDraft {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: AiProviderKindDraft::Anthropic {
                model: optional(&self.model),
                api_key: self.api_key.clone(),
            },
        }
    }
}

impl EditFormState {
    /// Initialize an Edit form from an existing entry. `replace_api_key`
    /// starts unticked so a user editing only the `name` does not need
    /// to re-type the secret (ADR-0016 §3 — write-only secret handling,
    /// re-applied to AI keys per ADR-0025 §Decision 4).
    #[must_use]
    pub fn from_entry(entry: &AiProviderEntry) -> Self {
        let (model, _api_key_ref) = match &entry.kind {
            AiProviderKind::Anthropic {
                model,
                keyring_api_key_ref,
            } => (
                model.clone().unwrap_or_default(),
                keyring_api_key_ref.clone(),
            ),
        };
        Self {
            name: entry.name.clone(),
            model,
            replace_api_key: false,
            new_api_key: String::new(),
        }
    }

    /// Translate the form into a strongly-typed [`AiProviderEditDraft`].
    /// The api-key carries [`SecretField::Set`] only when the user
    /// explicitly ticked the replace box; otherwise it stays
    /// [`SecretField::Keep`] so the keyring is untouched.
    #[must_use]
    pub fn to_draft(&self) -> AiProviderEditDraft {
        let api_key = if self.replace_api_key {
            SecretField::Set(self.new_api_key.clone())
        } else {
            SecretField::Keep
        };
        AiProviderEditDraft {
            name: self.name.clone(),
            kind: AiProviderKindEditDraft::Anthropic {
                model: optional(&self.model),
                api_key,
            },
        }
    }
}

fn optional(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn kind_label(kind: &AiProviderKind) -> String {
    match kind {
        AiProviderKind::Anthropic { .. } => t!("ai-settings-kind-anthropic"),
    }
}

fn render_form_buttons(ui: &mut egui::Ui) -> (bool, bool) {
    let mut save = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        save = ui.button(t!("ai-settings-save-button")).clicked();
        cancel = ui.button(t!("ai-settings-cancel-button")).clicked();
    });
    (save, cancel)
}

fn render_add_form(ui: &mut egui::Ui, form: &mut AddFormState) {
    egui::Grid::new("ai-settings-add-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("ai-settings-field-id"));
            ui.text_edit_singleline(&mut form.id);
            ui.end_row();
            ui.label(t!("ai-settings-field-name"));
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();
            ui.label(t!("ai-settings-field-kind"));
            ui.label(t!("ai-settings-kind-anthropic"));
            ui.end_row();
            ui.label(t!("ai-settings-field-model"));
            ui.text_edit_singleline(&mut form.model);
            ui.end_row();
            ui.label(t!("ai-settings-field-api-key"));
            ui.add(egui::TextEdit::singleline(&mut form.api_key).password(true));
            ui.end_row();
        });
}

fn render_edit_form(ui: &mut egui::Ui, id: &str, form: &mut EditFormState) {
    egui::Grid::new("ai-settings-edit-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("ai-settings-field-id"));
            ui.add_enabled(false, egui::TextEdit::singleline(&mut id.to_string()));
            ui.end_row();
            ui.label(t!("ai-settings-field-name"));
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();
            ui.label(t!("ai-settings-field-model"));
            ui.text_edit_singleline(&mut form.model);
            ui.end_row();
            ui.label(t!("ai-settings-replace-api-key"));
            ui.checkbox(&mut form.replace_api_key, "");
            ui.end_row();
            if form.replace_api_key {
                ui.label(t!("ai-settings-field-api-key"));
                ui.add(egui::TextEdit::singleline(&mut form.new_api_key).password(true));
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_config::{
        AiProviderFile, AiSettingsAdmin, InMemorySecretStore, SecretStore, AI_CONFIG_VERSION,
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    fn new_admin() -> (AiSettingsAdmin, Arc<dyn SecretStore>, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("ai-providers.toml");
        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::default());
        let file = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: None,
            providers: vec![],
        };
        let admin = AiSettingsAdmin::new_with_file(path, Arc::clone(&secrets), file);
        (admin, secrets, tmp)
    }

    fn anthropic_draft(id: &str) -> AiProviderDraft {
        AiProviderDraft {
            id: id.to_string(),
            name: format!("name-{id}"),
            kind: AiProviderKindDraft::Anthropic {
                model: Some("claude-sonnet-4-6".to_string()),
                api_key: "sk-test".to_string(),
            },
        }
    }

    #[test]
    fn new_view_is_closed_in_list_mode_with_no_error_or_pending_switch() {
        let view = AiSettingsView::new();
        assert!(!view.is_open());
        assert!(matches!(view.mode(), Mode::List));
        assert!(view.last_error().is_none());
    }

    #[test]
    fn open_and_close_toggle_visibility() {
        let mut view = AiSettingsView::new();
        view.open();
        assert!(view.is_open());
        view.close();
        assert!(!view.is_open());
    }

    #[test]
    fn start_add_switches_to_add_with_empty_form_and_clears_error() {
        let mut view = AiSettingsView::new();
        // Seed an error so we can assert it gets cleared.
        view.last_error = Some(DisplayError::plain("prior"));
        view.start_add();
        match view.mode() {
            Mode::Add(form) => {
                assert!(form.id.is_empty());
                assert!(form.name.is_empty());
                assert!(form.api_key.is_empty());
            }
            other => panic!("expected Add, got {other:?}"),
        }
        assert!(view.last_error().is_none());
    }

    #[test]
    fn start_edit_prefills_name_and_model_but_leaves_secret_replace_unticked() {
        let entry = AiProviderEntry {
            id: "claude".into(),
            name: "Claude".into(),
            kind: AiProviderKind::Anthropic {
                model: Some("claude-opus-4-7".into()),
                keyring_api_key_ref: "dbboard.ai.claude.api_key".into(),
            },
        };
        let mut view = AiSettingsView::new();
        view.start_edit(&entry);
        match view.mode() {
            Mode::Edit { id, form } => {
                assert_eq!(id, "claude");
                assert_eq!(form.name, "Claude");
                assert_eq!(form.model, "claude-opus-4-7");
                assert!(!form.replace_api_key);
                assert!(form.new_api_key.is_empty());
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn start_delete_records_the_id_and_name() {
        let entry = AiProviderEntry {
            id: "claude".into(),
            name: "Claude".into(),
            kind: AiProviderKind::Anthropic {
                model: None,
                keyring_api_key_ref: "dbboard.ai.claude.api_key".into(),
            },
        };
        let mut view = AiSettingsView::new();
        view.start_delete(&entry);
        match view.mode() {
            Mode::ConfirmDelete { id, name } => {
                assert_eq!(id, "claude");
                assert_eq!(name, "Claude");
            }
            other => panic!("expected ConfirmDelete, got {other:?}"),
        }
    }

    #[test]
    fn cancel_returns_to_list_and_clears_error() {
        let mut view = AiSettingsView::new();
        view.start_add();
        view.last_error = Some(DisplayError::plain("oops"));
        view.cancel();
        assert!(matches!(view.mode(), Mode::List));
        assert!(view.last_error().is_none());
    }

    #[test]
    fn request_use_then_take_pending_switch_round_trip_drains_once() {
        let mut view = AiSettingsView::new();
        assert!(view.take_pending_switch().is_none());
        view.request_use("anthropic-sonnet");
        assert_eq!(
            view.take_pending_switch().as_deref(),
            Some("anthropic-sonnet")
        );
        assert!(view.take_pending_switch().is_none());
    }

    #[test]
    fn request_use_only_remembers_the_most_recent_click() {
        let mut view = AiSettingsView::new();
        view.request_use("first");
        view.request_use("second");
        assert_eq!(view.take_pending_switch().as_deref(), Some("second"));
    }

    #[test]
    fn submit_add_success_appends_entry_and_returns_to_list() {
        let (mut admin, _secrets, _tmp) = new_admin();
        let mut view = AiSettingsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            *form = AddFormState {
                id: "claude".into(),
                name: "Claude".into(),
                model: "claude-sonnet-4-6".into(),
                api_key: "sk-abc".into(),
            };
        }
        view.submit_add(&mut admin).expect("add should succeed");
        assert!(matches!(view.mode(), Mode::List));
        assert!(view.last_error().is_none());
        assert_eq!(admin.entries().len(), 1);
        assert_eq!(admin.entries()[0].id, "claude");
    }

    #[test]
    fn submit_add_duplicate_id_keeps_form_open_and_records_error() {
        let (mut admin, _secrets, _tmp) = new_admin();
        admin.add(anthropic_draft("dup")).expect("seed");
        let mut view = AiSettingsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            *form = AddFormState {
                id: "dup".into(),
                name: "Other".into(),
                model: String::new(),
                api_key: "sk-zzz".into(),
            };
        }
        let res = view.submit_add(&mut admin);
        assert!(matches!(res, Err(AiSettingsError::DuplicateId(_))));
        assert!(matches!(view.mode(), Mode::Add(_)));
        assert!(view.last_error().is_some());
        assert_eq!(admin.entries().len(), 1);
    }

    #[test]
    fn submit_edit_updates_name_without_touching_api_key_when_replace_is_unset() {
        let (mut admin, secrets, _tmp) = new_admin();
        admin.add(anthropic_draft("claude")).expect("seed");
        let original_key = secrets
            .get("dbboard.ai.claude.api_key")
            .expect("read seed key");

        let mut view = AiSettingsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            form.name = "Renamed".into();
        }
        view.submit_edit(&mut admin).expect("edit should succeed");
        assert_eq!(admin.entries()[0].name, "Renamed");
        let post_key = secrets.get("dbboard.ai.claude.api_key").expect("read key");
        assert_eq!(post_key, original_key, "api key must be untouched");
        assert!(matches!(view.mode(), Mode::List));
    }

    #[test]
    fn submit_edit_rewrites_api_key_when_replace_is_ticked() {
        let (mut admin, secrets, _tmp) = new_admin();
        admin.add(anthropic_draft("claude")).expect("seed");

        let mut view = AiSettingsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            form.replace_api_key = true;
            form.new_api_key = "sk-rotated".into();
        }
        view.submit_edit(&mut admin).expect("edit should succeed");
        let post_key = secrets.get("dbboard.ai.claude.api_key").expect("read key");
        assert_eq!(post_key, "sk-rotated");
    }

    #[test]
    fn submit_delete_removes_the_entry_and_returns_to_list() {
        let (mut admin, _secrets, _tmp) = new_admin();
        admin.add(anthropic_draft("claude")).expect("seed");
        let mut view = AiSettingsView::new();
        view.start_delete(&admin.entries()[0].clone());
        view.submit_delete(&mut admin)
            .expect("delete should succeed");
        assert!(admin.entries().is_empty());
        assert!(matches!(view.mode(), Mode::List));
    }
}
