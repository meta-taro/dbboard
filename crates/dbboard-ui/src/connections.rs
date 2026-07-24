//! Connection management window (ADR-0016, Stage 1).
//!
//! `HeidiSQL` mental model: each running dbboard process owns exactly one
//! active connection; managing connections is a separate concern from
//! using one. This module is the **management** half — Add / Edit /
//! Delete forms over the local store. It never switches which
//! connection the running process is talking to, and the UI surfaces a
//! persistent hint that new or edited entries take effect on the
//! **next** restart (ADR-0016 §1, §2).
//!
//! The view is intentionally split into a small, easily-testable state
//! machine ([`ConnectionsView`] + [`Mode`]) and a thin egui rendering
//! layer ([`ConnectionsView::ui`]). The state machine is covered by
//! unit tests against an in-memory [`ConnectionAdmin`]; the egui code
//! path is exercised end-to-end at the binary level only.

use std::path::PathBuf;

use dbboard_config::{
    ConfigError, ConnectionAdmin, ConnectionDraft, ConnectionEditDraft, ConnectionEntry,
    ConnectionKind, ConnectionKindDraft, ConnectionKindEditDraft, SecretField,
};
use dbboard_i18n::t;
use eframe::egui;
use zeroize::Zeroize;

use crate::errors::{self, config_error_display, DisplayError};

/// The connection management window. Lives next to [`crate::DbboardApp`]
/// in the binary and is shown when the user opens it from the top bar.
#[derive(Debug)]
pub struct ConnectionsView {
    is_open: bool,
    mode: Mode,
    /// Last error from a failed submit, surfaced inline above the form
    /// buttons. Cleared on every successful submit or mode transition.
    /// Carries both the localized message and the original English so the
    /// inline banner can show and copy both (ADR-0039).
    last_error: Option<DisplayError>,
    /// Last success message (green), e.g. an export/import summary
    /// (ADR-0038). Shown in List mode after a completed transfer; cleared
    /// on the next mode transition so it does not linger stale.
    last_info: Option<String>,
    /// Id of a connection the user just asked to switch to via the
    /// per-row "Connect" button (ADR-0020). Drained by the host
    /// (typically `DesktopApp`) via [`Self::take_pending_connect`]
    /// after every `ui()` call and turned into a
    /// [`crate::DbboardApp::switch_connection`]. Holds at most one id;
    /// a second click before the host drains overwrites the first, so
    /// only the most recent intent reaches the worker.
    pending_connect: Option<String>,
    /// An export blob awaiting a native "Save As" dialog, plus the number
    /// of connections it holds (ADR-0038). Set when the user confirms an
    /// export; drained by the host via [`Self::drive_file_dialogs`] *after*
    /// it releases the `ConnectionAdmin` lock, because the native dialog
    /// blocks this thread for an unbounded time and the connection switcher
    /// shares that lock (same rationale as [`Self::pending_connect`]).
    pending_save: Option<(Vec<u8>, usize)>,
    /// `true` when the user clicked "Choose file…" in the Import form and a
    /// native open dialog is owed (ADR-0038). Drained lock-free by the host
    /// in [`Self::drive_file_dialogs`] for the same reason as
    /// [`Self::pending_save`].
    pending_pick: bool,
}

impl Default for ConnectionsView {
    fn default() -> Self {
        Self::new()
    }
}

/// Mutually exclusive states the connections window can be in.
#[derive(Debug, Clone)]
pub enum Mode {
    /// List of entries with Add / Edit / Delete buttons per row.
    List,
    /// New-entry form. The form remembers every kind's fields even
    /// when the kind selector flips, so the user does not lose typing
    /// they did before switching tabs.
    Add(AddFormState),
    /// Edit-an-existing-entry form. The id is read-only here (it is
    /// the primary key of every keyring reference) and is shown as a
    /// disabled field.
    Edit { id: String, form: EditFormState },
    /// "Are you sure?" confirmation prompt before a destructive
    /// delete. Shows the entry's display name to reduce mis-clicks.
    ConfirmDelete { id: String, name: String },
    /// Passphrase form for exporting the whole store to an encrypted
    /// bundle (ADR-0038). Requires a passphrase typed twice.
    Export(ExportFormState),
    /// Passphrase form for importing an encrypted bundle (ADR-0038).
    /// Requires a chosen `.dbbx` file plus its passphrase.
    Import(ImportFormState),
}

/// Backing buffers for the Export form (ADR-0038). Both fields hold
/// secret passphrase material and are zeroized when the form closes.
#[derive(Debug, Default, Clone)]
pub struct ExportFormState {
    pub passphrase: String,
    pub confirm: String,
}

/// Backing buffers for the Import form (ADR-0038). `file_path` is set by
/// the native file picker; `file_name` is its display label. `passphrase`
/// is secret material and is zeroized when the form closes.
#[derive(Debug, Default, Clone)]
pub struct ImportFormState {
    pub passphrase: String,
    pub file_name: String,
    pub file_path: Option<PathBuf>,
}

/// Adapter kind chosen by the kind selector in the Add form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindSelector {
    #[default]
    Turso,
    D1,
    Postgres,
    Neon,
    Supabase,
    AuroraDsql,
}

/// Backing buffers for the Add form. Every kind's fields are kept side
/// by side so flipping the kind dropdown does not clobber typing
/// already done under another kind.
#[derive(Debug, Default, Clone)]
pub struct AddFormState {
    pub id: String,
    pub name: String,
    pub kind: KindSelector,
    pub turso_path: String,
    pub d1_account_id: String,
    pub d1_database_id: String,
    pub d1_base_url: String,
    pub d1_token: String,
    pub pg_url: String,
    pub neon_url: String,
    pub supabase_url: String,
    pub aurora_dsql_url: String,
}

/// Backing buffers for the Edit form. The id is held outside this
/// struct (on [`Mode::Edit`]) because it is not user-editable; only
/// `name` and the per-kind buffers are.
#[derive(Debug, Clone)]
pub struct EditFormState {
    pub name: String,
    pub kind: EditKindState,
}

/// Per-kind buffers for the Edit form. Each variant mirrors the
/// existing entry's [`ConnectionKind`]; secret fields use a `replace_*`
/// + `new_*` pair to keep "leave it alone" distinct from "overwrite
///   it" (ADR-0016 §3 — secrets are write-only: the UI never reads them
///   back).
#[derive(Debug, Clone)]
pub enum EditKindState {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: String,
        replace_token: bool,
        new_token: String,
    },
    Postgres {
        replace_url: bool,
        new_url: String,
    },
    Neon {
        replace_url: bool,
        new_url: String,
    },
    Supabase {
        replace_url: bool,
        new_url: String,
    },
    AuroraDsql {
        replace_url: bool,
        new_url: String,
    },
    /// Aurora DSQL IAM (ADR-0036) is config-file-only in v1: the list
    /// offers Connect and Delete but not Edit, so this variant is a
    /// read-only marker with no editable buffers. It exists only to keep
    /// [`EditFormState::from_entry`] total.
    AuroraDsqlIam,
}

impl ConnectionsView {
    /// Construct a fresh, closed view in `List` mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: Mode::List,
            last_error: None,
            last_info: None,
            pending_connect: None,
            pending_save: None,
            pending_pick: false,
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

    /// Last success message (e.g. an export/import summary), if any. The
    /// UI renders this in green at the top of List mode.
    #[must_use]
    pub fn last_info(&self) -> Option<&str> {
        self.last_info.as_deref()
    }

    /// Record a click on the per-row "Connect" button (ADR-0020). The
    /// host drains the value with [`Self::take_pending_connect`] after
    /// every `ui()` call. A repeat click before the host drains
    /// overwrites the previous id — only the most recent intent is
    /// honoured, since older clicks are stale.
    pub fn request_connect(&mut self, id: &str) {
        self.pending_connect = Some(id.to_string());
    }

    /// Drain a pending "Connect" click, if any. Returns the id once and
    /// then resets to `None`, so the host should call this on every
    /// frame and forward the result into
    /// [`crate::DbboardApp::switch_connection`].
    pub fn take_pending_connect(&mut self) -> Option<String> {
        self.pending_connect.take()
    }

    /// Switch to the Add form with empty fields.
    pub fn start_add(&mut self) {
        self.mode = Mode::Add(AddFormState::default());
        self.last_error = None;
        self.last_info = None;
    }

    /// Switch to the Export passphrase form (ADR-0038).
    pub fn start_export(&mut self) {
        self.mode = Mode::Export(ExportFormState::default());
        self.last_error = None;
        self.last_info = None;
    }

    /// Switch to the Import passphrase form (ADR-0038).
    pub fn start_import(&mut self) {
        self.mode = Mode::Import(ImportFormState::default());
        self.last_error = None;
        self.last_info = None;
    }

    /// Switch to the Edit form pre-filled from `entry`. Secret fields
    /// start with `replace_*` unticked so an unrelated `name`-only edit
    /// does not need to re-type the secret.
    pub fn start_edit(&mut self, entry: &ConnectionEntry) {
        self.mode = Mode::Edit {
            id: entry.id.clone(),
            form: EditFormState::from_entry(entry),
        };
        self.last_error = None;
        self.last_info = None;
    }

    /// Switch to the delete confirmation prompt for `entry`.
    pub fn start_delete(&mut self, entry: &ConnectionEntry) {
        self.mode = Mode::ConfirmDelete {
            id: entry.id.clone(),
            name: entry.name.clone(),
        };
        self.last_error = None;
        self.last_info = None;
    }

    /// Cancel whatever form is currently shown and return to `List`.
    /// Scrubs any passphrase buffers first so a cancelled export/import
    /// does not leave secret material in memory (ADR-0038).
    pub fn cancel(&mut self) {
        self.scrub_passphrases();
        self.mode = Mode::List;
        self.last_error = None;
    }

    /// Zero out any passphrase strings held by the current form before it
    /// is dropped or replaced (ADR-0038). A no-op for non-transfer modes.
    fn scrub_passphrases(&mut self) {
        match &mut self.mode {
            Mode::Export(form) => {
                form.passphrase.zeroize();
                form.confirm.zeroize();
            }
            Mode::Import(form) => form.passphrase.zeroize(),
            _ => {}
        }
    }

    /// Build a [`ConnectionDraft`] from the current Add form and route
    /// it through `admin`. On success the form is closed and the view
    /// returns to `List`; on failure the error is stored in
    /// [`Self::last_error`] and the form stays open so the user can
    /// retry.
    ///
    /// # Errors
    ///
    /// Any [`ConfigError`] from [`ConnectionAdmin::add`] is forwarded.
    /// A separate [`ConfigError::DuplicateId`] is produced (with an
    /// empty id) if the form has no id, since the keyring scheme
    /// derives every reference from the id.
    pub fn submit_add(&mut self, admin: &mut ConnectionAdmin) -> Result<(), ConfigError> {
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
                self.last_error = Some(config_error_display(&err));
                Err(err)
            }
        }
    }

    /// Build a [`ConnectionEditDraft`] from the current Edit form and
    /// route it through `admin`. Behaves like [`Self::submit_add`] on
    /// success / failure.
    ///
    /// # Errors
    ///
    /// Any [`ConfigError`] from [`ConnectionAdmin::update`] is
    /// forwarded.
    pub fn submit_edit(&mut self, admin: &mut ConnectionAdmin) -> Result<(), ConfigError> {
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
                self.last_error = Some(config_error_display(&err));
                Err(err)
            }
        }
    }

    /// Commit the delete confirmation. Behaves like [`Self::submit_add`]
    /// on success / failure.
    ///
    /// # Errors
    ///
    /// Any [`ConfigError`] from [`ConnectionAdmin::delete`] is forwarded.
    pub fn submit_delete(&mut self, admin: &mut ConnectionAdmin) -> Result<(), ConfigError> {
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
                self.last_error = Some(config_error_display(&err));
                Err(err)
            }
        }
    }

    /// Validate the Export form and encrypt the whole store into a bundle
    /// blob (ADR-0038). Returns `Some(blob)` ready for the caller to write
    /// to a user-chosen file; `None` (with [`Self::last_error`] set) if the
    /// two passphrases disagree or [`ConnectionAdmin::export_bundle`] fails.
    /// The mode stays `Export` on either outcome so the caller can drive
    /// the save dialog (success) or the user can retry (failure).
    ///
    /// The passphrase is cloned out of the form only for the duration of
    /// the call and zeroized before returning.
    pub fn submit_export(&mut self, admin: &ConnectionAdmin) -> Option<Vec<u8>> {
        // Compare before cloning so the common typo-in-confirm case does no
        // extra secret allocation.
        let matches = match &self.mode {
            Mode::Export(form) => form.passphrase == form.confirm,
            _ => return None,
        };
        if !matches {
            self.last_error = Some(DisplayError::plain(t!("connections-passphrase-mismatch")));
            return None;
        }
        let mut passphrase = match &self.mode {
            Mode::Export(form) => form.passphrase.clone(),
            _ => return None,
        };
        let outcome = match admin.export_bundle(&passphrase) {
            Ok(blob) => {
                self.last_error = None;
                Some(blob)
            }
            Err(err) => {
                self.last_error = Some(config_error_display(&err));
                None
            }
        };
        passphrase.zeroize();
        outcome
    }

    /// Close the Export form after the bundle has been written, returning
    /// to List with a green success summary (ADR-0038). `count` is the
    /// number of connections written.
    pub fn finish_export(&mut self, count: usize) {
        self.scrub_passphrases();
        self.mode = Mode::List;
        self.last_error = None;
        self.last_info = Some(format!("{} ({})", t!("connections-export-ok"), count));
    }

    /// Decrypt `blob` under the Import form's passphrase and merge it into
    /// the store (ADR-0038). On success returns to List with a green
    /// summary of imported vs skipped ids and returns `true`; on failure
    /// sets [`Self::last_error`], leaves the form open, and returns
    /// `false`. The passphrase is zeroized before returning either way.
    pub fn submit_import(&mut self, admin: &mut ConnectionAdmin, blob: &[u8]) -> bool {
        let mut passphrase = match &self.mode {
            Mode::Import(form) => form.passphrase.clone(),
            _ => return false,
        };
        let result = admin.import_bundle(blob, &passphrase);
        passphrase.zeroize();

        match result {
            Ok(report) => {
                // Append the skipped-id list only when non-empty so the
                // common all-imported case stays terse.
                let detail = if report.skipped.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", report.skipped.join(", "))
                };
                let msg = format!(
                    "{}: {} / {}: {}{}",
                    t!("connections-import-imported"),
                    report.imported.len(),
                    t!("connections-import-skipped"),
                    report.skipped.len(),
                    detail,
                );
                self.scrub_passphrases();
                self.mode = Mode::List;
                self.last_error = None;
                self.last_info = Some(msg);
                true
            }
            Err(err) => {
                self.last_error = Some(config_error_display(&err));
                false
            }
        }
    }

    /// Render the window into `ctx`. No-op when closed.
    ///
    /// Holds a `&mut ConnectionAdmin` for the duration of the call;
    /// the caller is responsible for guarding shared access (typically
    /// `Arc<Mutex<ConnectionAdmin>>` in the desktop binary).
    ///
    /// `active_id` is the connection id currently bound to the running
    /// server (ADR-0020). The active row is marked and its button
    /// relabelled "Reconnect": clicking it rebuilds the live adapter,
    /// which is the recovery path when a short-lived credential has
    /// expired (ADR-0036) rather than a redundant swap.
    /// `switch_error` is the display-ready message for the last failed
    /// in-process connection switch (ADR-0020), or `None` when the last
    /// switch succeeded. Rendered inline in List mode next to the Connect
    /// buttons so a failed "Connect" click is visible rather than silently
    /// swallowed — the switcher runs off-thread and its `SwitchFailed`
    /// reply otherwise has no on-screen home.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        admin: &mut ConnectionAdmin,
        active_id: &str,
        switch_error: Option<&str>,
    ) {
        if !self.is_open {
            return;
        }
        let mut is_open = self.is_open;
        egui::Window::new(t!("connections-window-title"))
            .open(&mut is_open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                self.render(ui, admin, active_id, switch_error);
            });
        // Closing via the window's own title-bar X bypasses `cancel()`, so
        // scrub any passphrase left in an open Export/Import form here too
        // (ADR-0038) — otherwise it would linger in memory until the form
        // is revisited.
        if self.is_open && !is_open {
            self.scrub_passphrases();
        }
        self.is_open = is_open;
    }

    /// Run any native file dialog owed by a transfer (ADR-0038). The host
    /// MUST call this after releasing the `ConnectionAdmin` lock: the
    /// native dialog blocks this thread for an unbounded time and the
    /// connection switcher shares that lock, so running it under the lock
    /// would stall an in-flight Connect/Reconnect. Needs no admin access —
    /// saving writes bytes and picking returns a path; the actual
    /// encrypt/import already ran under the lock inside [`Self::render`].
    /// A no-op unless an export blob is awaiting save or an import file
    /// pick was requested.
    pub fn drive_file_dialogs(&mut self) {
        if let Some((blob, count)) = self.pending_save.take() {
            match save_bundle_via_dialog(&blob) {
                SaveOutcome::Written => self.finish_export(count),
                // User backed out of the dialog — keep the Export form open
                // so they can retry or cancel.
                SaveOutcome::Cancelled => {}
                SaveOutcome::Failed(msg) => self.last_error = Some(DisplayError::plain(msg)),
            }
        }
        if std::mem::take(&mut self.pending_pick) {
            if let Some((path, name)) = pick_bundle_file() {
                if let Mode::Import(form) = &mut self.mode {
                    form.file_name = name;
                    form.file_path = Some(path);
                }
            }
        }
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        admin: &mut ConnectionAdmin,
        active_id: &str,
        switch_error: Option<&str>,
    ) {
        // The restart hint is always visible at the top so the user
        // can never mistake an Add for an in-process switch (ADR-0016).
        ui.label(t!("connections-restart-hint"));
        ui.separator();

        // A completed export/import leaves a green summary here (ADR-0038)
        // until the next mode transition clears it.
        if let Some(info) = &self.last_info {
            ui.colored_label(crate::theme::success(ui.visuals().dark_mode), info);
        }

        match &mut self.mode {
            Mode::List => {
                Self::render_list(
                    ui,
                    admin,
                    &mut self.mode,
                    &mut self.pending_connect,
                    &mut self.last_info,
                    active_id,
                    switch_error,
                );
            }
            Mode::Add(form) => {
                let submit_now = render_add_form(ui, form);
                errors::render_error(ui, self.last_error.as_ref());
                let (submit_btn, cancel_btn) = render_form_buttons(ui);
                if submit_btn || submit_now {
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
                    crate::theme::danger(ui.visuals().dark_mode),
                    format!("{}: {name}", t!("connections-confirm-delete")),
                );
                errors::render_error(ui, self.last_error.as_ref());
                ui.horizontal(|ui| {
                    if ui.button(t!("connections-delete-button")).clicked() {
                        let _ = self.submit_delete(admin);
                    }
                    if ui.button(t!("connections-cancel-button")).clicked() {
                        self.cancel();
                    }
                });
            }
            Mode::Export(form) => {
                render_export_form(ui, form);
                errors::render_error(ui, self.last_error.as_ref());
                let (export_btn, cancel_btn) = render_transfer_buttons(
                    ui,
                    &t!("connections-export-do"),
                    /* enabled */ true,
                );
                if export_btn {
                    if let Some(blob) = self.submit_export(admin) {
                        // Defer the blocking Save dialog to the host, which
                        // runs it after releasing the ConnectionAdmin lock
                        // (ADR-0038); the switcher shares that lock.
                        let count = admin.entries().len();
                        self.pending_save = Some((blob, count));
                    }
                } else if cancel_btn {
                    self.cancel();
                }
            }
            Mode::Import(form) => {
                let choose = render_import_form(ui, form);
                // Capture every form-derived value before touching `self`,
                // so the `&mut self.mode` borrow ends here (mirrors the Add
                // arm's borrow discipline).
                let has_file = form.file_path.is_some();
                let chosen = form.file_path.clone();
                errors::render_error(ui, self.last_error.as_ref());
                let (import_btn, cancel_btn) =
                    render_transfer_buttons(ui, &t!("connections-import-do"), has_file);
                if choose {
                    // Defer the blocking open dialog to the host (see above).
                    self.pending_pick = true;
                } else if import_btn {
                    if let Some(path) = chosen {
                        match std::fs::read(&path) {
                            Ok(bytes) => {
                                self.submit_import(admin, &bytes);
                            }
                            Err(err) => {
                                self.last_error = Some(DisplayError::plain(err.to_string()));
                            }
                        }
                    }
                } else if cancel_btn {
                    self.cancel();
                }
            }
        }
    }

    fn render_list(
        ui: &mut egui::Ui,
        admin: &mut ConnectionAdmin,
        mode: &mut Mode,
        pending_connect: &mut Option<String>,
        last_info: &mut Option<String>,
        active_id: &str,
        switch_error: Option<&str>,
    ) {
        // A failed Connect leaves the previous adapter live and lands a
        // SwitchFailed reply off-thread; surface it here (red, above the
        // list) so the click is never silently swallowed (ADR-0020). The
        // message already embeds the English DbError body inline
        // (`switch_error_message`), so wrap it as a plain copyable error
        // (ADR-0039) rather than re-deriving a localized/original split.
        let switch_error = switch_error.map(DisplayError::plain);
        errors::render_error(ui, switch_error.as_ref());
        // Add / Export / Import all leave List mode; clear a lingering
        // green transfer summary so it does not bleed into the next form
        // (ADR-0038).
        let mut leave = None;
        ui.horizontal(|ui| {
            if ui.button(t!("connections-add-button")).clicked() {
                leave = Some(Mode::Add(AddFormState::default()));
            }
            if ui.button(t!("connections-export-button")).clicked() {
                leave = Some(Mode::Export(ExportFormState::default()));
            }
            if ui.button(t!("connections-import-button")).clicked() {
                leave = Some(Mode::Import(ImportFormState::default()));
            }
        });
        if let Some(next) = leave {
            *mode = next;
            *last_info = None;
            return;
        }
        ui.separator();

        // Take a snapshot so we can borrow admin mutably below in
        // response to the per-row buttons.
        let entries: Vec<ConnectionEntry> = admin.entries().to_vec();
        if entries.is_empty() {
            ui.label(t!("connections-list-empty"));
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &entries {
                ui.horizontal(|ui| {
                    let is_active = entry.id == active_id;
                    let label = if is_active {
                        format!(
                            "{} ({}) {}",
                            entry.name,
                            kind_label(&entry.kind),
                            t!("connections-active-marker")
                        )
                    } else {
                        format!("{} ({})", entry.name, kind_label(&entry.kind))
                    };
                    ui.label(label);
                    // ADR-0020 + ADR-0036: the primary button drives an
                    // in-process adapter swap. The active row shows an
                    // *enabled* "Reconnect" rather than a disabled
                    // "Connect": rebuilding the live adapter is the
                    // recovery path when a short-lived credential (Aurora
                    // DSQL IAM token) has expired and the server rejects
                    // reconnects with "access denied". Both actions funnel
                    // through the same `pending_connect` request.
                    let button_clicked = match row_connect_action(is_active) {
                        RowConnectAction::Reconnect => ui
                            .add(egui::Button::new(t!("connections-reconnect-button")).small())
                            .clicked(),
                        RowConnectAction::Connect => ui
                            .add(egui::Button::new(t!("connections-connect-button")).small())
                            .clicked(),
                    };
                    if button_clicked {
                        *pending_connect = Some(entry.id.clone());
                    }
                    // Edit is disabled for config-file-only kinds
                    // (Aurora DSQL IAM, ADR-0036): their fields are
                    // hand-authored in connections.toml.
                    if ui
                        .add_enabled(
                            is_ui_editable(&entry.kind),
                            egui::Button::new(t!("connections-edit-button")).small(),
                        )
                        .clicked()
                    {
                        *mode = Mode::Edit {
                            id: entry.id.clone(),
                            form: EditFormState::from_entry(entry),
                        };
                        // Leaving List mode: drop a stale transfer summary
                        // so it does not linger over the Edit form (ADR-0038).
                        *last_info = None;
                    }
                    if ui.small_button(t!("connections-delete-button")).clicked() {
                        *mode = Mode::ConfirmDelete {
                            id: entry.id.clone(),
                            name: entry.name.clone(),
                        };
                        *last_info = None;
                    }
                });
            }
        });
    }
}

impl AddFormState {
    /// Translate the form's freeform string buffers into a strongly-
    /// typed [`ConnectionDraft`] that [`ConnectionAdmin::add`] will
    /// accept. `base_url` is `None` when the field is empty, matching
    /// the TOML schema's `#[serde(skip_serializing_if = "Option::is_none")]`.
    #[must_use]
    pub fn to_draft(&self) -> ConnectionDraft {
        let kind = match self.kind {
            KindSelector::Turso => ConnectionKindDraft::Turso {
                path: self.turso_path.clone(),
            },
            KindSelector::D1 => ConnectionKindDraft::D1 {
                account_id: self.d1_account_id.clone(),
                database_id: self.d1_database_id.clone(),
                base_url: optional(&self.d1_base_url),
                token: self.d1_token.clone(),
            },
            KindSelector::Postgres => ConnectionKindDraft::Postgres {
                url: self.pg_url.clone(),
            },
            KindSelector::Neon => ConnectionKindDraft::Neon {
                url: self.neon_url.clone(),
            },
            KindSelector::Supabase => ConnectionKindDraft::Supabase {
                url: self.supabase_url.clone(),
            },
            KindSelector::AuroraDsql => ConnectionKindDraft::AuroraDsql {
                url: self.aurora_dsql_url.clone(),
            },
        };
        ConnectionDraft {
            id: self.id.clone(),
            name: self.name.clone(),
            kind,
        }
    }
}

impl EditFormState {
    /// Initialize an Edit form from an existing entry. Secret fields
    /// start as `replace_* = false` so a user editing only the `name`
    /// does not need to re-type the secret (ADR-0016 §3 — write-only
    /// secret handling).
    #[must_use]
    pub fn from_entry(entry: &ConnectionEntry) -> Self {
        let kind = match &entry.kind {
            ConnectionKind::Turso { path } => EditKindState::Turso { path: path.clone() },
            ConnectionKind::D1 {
                account_id,
                database_id,
                base_url,
                keyring_token_ref: _,
            } => EditKindState::D1 {
                account_id: account_id.clone(),
                database_id: database_id.clone(),
                base_url: base_url.clone().unwrap_or_default(),
                replace_token: false,
                new_token: String::new(),
            },
            ConnectionKind::Postgres { keyring_url_ref: _ } => EditKindState::Postgres {
                replace_url: false,
                new_url: String::new(),
            },
            ConnectionKind::Neon { keyring_url_ref: _ } => EditKindState::Neon {
                replace_url: false,
                new_url: String::new(),
            },
            ConnectionKind::Supabase { keyring_url_ref: _ } => EditKindState::Supabase {
                replace_url: false,
                new_url: String::new(),
            },
            ConnectionKind::AuroraDsql { keyring_url_ref: _ } => EditKindState::AuroraDsql {
                replace_url: false,
                new_url: String::new(),
            },
            // Config-file-only in v1; the list gates its Edit button off,
            // so this arm exists only for exhaustiveness (ADR-0036).
            ConnectionKind::AuroraDsqlIam { .. } => EditKindState::AuroraDsqlIam,
        };
        Self {
            name: entry.name.clone(),
            kind,
        }
    }

    /// Translate the form into an admin-layer [`ConnectionEditDraft`].
    #[must_use]
    pub fn to_draft(&self) -> ConnectionEditDraft {
        let kind = match &self.kind {
            EditKindState::Turso { path } => ConnectionKindEditDraft::Turso { path: path.clone() },
            EditKindState::D1 {
                account_id,
                database_id,
                base_url,
                replace_token,
                new_token,
            } => ConnectionKindEditDraft::D1 {
                account_id: account_id.clone(),
                database_id: database_id.clone(),
                base_url: optional(base_url),
                token: if *replace_token {
                    SecretField::Set(new_token.clone())
                } else {
                    SecretField::Keep
                },
            },
            EditKindState::Postgres {
                replace_url,
                new_url,
            } => ConnectionKindEditDraft::Postgres {
                url: if *replace_url {
                    SecretField::Set(new_url.clone())
                } else {
                    SecretField::Keep
                },
            },
            EditKindState::Neon {
                replace_url,
                new_url,
            } => ConnectionKindEditDraft::Neon {
                url: if *replace_url {
                    SecretField::Set(new_url.clone())
                } else {
                    SecretField::Keep
                },
            },
            EditKindState::Supabase {
                replace_url,
                new_url,
            } => ConnectionKindEditDraft::Supabase {
                url: if *replace_url {
                    SecretField::Set(new_url.clone())
                } else {
                    SecretField::Keep
                },
            },
            EditKindState::AuroraDsql {
                replace_url,
                new_url,
            } => ConnectionKindEditDraft::AuroraDsql {
                url: if *replace_url {
                    SecretField::Set(new_url.clone())
                } else {
                    SecretField::Keep
                },
            },
            // Unreachable in practice — the list gates Edit off for this
            // kind — but if ever submitted, `update()` rejects it as a
            // KindMismatch, which is the safe outcome (ADR-0036).
            EditKindState::AuroraDsqlIam => ConnectionKindEditDraft::AuroraDsqlIam,
        };
        ConnectionEditDraft {
            name: self.name.clone(),
            kind,
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

/// The per-row primary button shown next to a connection (ADR-0020 +
/// ADR-0036).
///
/// ADR-0020 originally rendered the active row's Connect button
/// **disabled**, reasoning that re-selecting the live connection would
/// "only rebuild the same adapter we already have live". ADR-0036 turns
/// that rebuild into the recovery path: when a short-lived credential
/// (e.g. an Aurora DSQL IAM token minted at build time, ~15 min TTL)
/// expires out from under the pool, the server starts rejecting
/// reconnects with `access denied` and the *only* way back is to rebuild
/// the adapter — which mints a fresh token. So the active row now offers
/// an **enabled Reconnect** button instead of a disabled Connect one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowConnectAction {
    /// Switch to a connection that is not currently active.
    Connect,
    /// Rebuild the adapter for the already-active connection (recovery).
    Reconnect,
}

/// Decide which primary button an entry row should render, given whether
/// it is the currently-active connection. Both actions funnel through the
/// same `pending_connect` request; only the label differs.
#[must_use]
pub fn row_connect_action(is_active: bool) -> RowConnectAction {
    if is_active {
        RowConnectAction::Reconnect
    } else {
        RowConnectAction::Connect
    }
}

fn kind_label(kind: &ConnectionKind) -> &'static str {
    match kind {
        ConnectionKind::Turso { .. } => "Turso",
        ConnectionKind::D1 { .. } => "Cloudflare D1",
        ConnectionKind::Postgres { .. } => "Postgres",
        ConnectionKind::Neon { .. } => "Neon",
        ConnectionKind::Supabase { .. } => "Supabase",
        ConnectionKind::AuroraDsql { .. } => "Aurora DSQL",
        ConnectionKind::AuroraDsqlIam { .. } => "Aurora DSQL (IAM)",
    }
}

/// Whether the UI offers an Edit form for `kind`. The Aurora DSQL IAM
/// kind (ADR-0036) is config-file-only in v1 — it stores hand-authored
/// AWS credentials in `connections.toml` — so the list shows Connect and
/// Delete for it but disables Edit.
fn is_ui_editable(kind: &ConnectionKind) -> bool {
    !matches!(kind, ConnectionKind::AuroraDsqlIam { .. })
}

fn render_form_buttons(ui: &mut egui::Ui) -> (bool, bool) {
    let mut submit = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        submit = ui.button(t!("connections-save-button")).clicked();
        cancel = ui.button(t!("connections-cancel-button")).clicked();
    });
    (submit, cancel)
}

fn render_add_form(ui: &mut egui::Ui, form: &mut AddFormState) -> bool {
    egui::Grid::new("connections-add-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("connections-field-id"));
            ui.text_edit_singleline(&mut form.id);
            ui.end_row();
            ui.label(t!("connections-field-name"));
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();
            ui.label(t!("connections-field-kind"));
            egui::ComboBox::from_id_salt("connections-kind-selector")
                .selected_text(kind_selector_label(form.kind))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.kind, KindSelector::Turso, "Turso");
                    ui.selectable_value(&mut form.kind, KindSelector::D1, "Cloudflare D1");
                    ui.selectable_value(&mut form.kind, KindSelector::Postgres, "Postgres");
                    ui.selectable_value(&mut form.kind, KindSelector::Neon, "Neon");
                    ui.selectable_value(&mut form.kind, KindSelector::Supabase, "Supabase");
                    ui.selectable_value(&mut form.kind, KindSelector::AuroraDsql, "Aurora DSQL");
                });
            ui.end_row();
        });
    ui.separator();
    match form.kind {
        KindSelector::Turso => {
            ui.label(t!("connections-field-turso-path"));
            ui.text_edit_singleline(&mut form.turso_path);
        }
        KindSelector::D1 => {
            ui.label(t!("connections-field-d1-account"));
            ui.text_edit_singleline(&mut form.d1_account_id);
            ui.label(t!("connections-field-d1-database"));
            ui.text_edit_singleline(&mut form.d1_database_id);
            ui.label(t!("connections-field-d1-base-url"));
            ui.text_edit_singleline(&mut form.d1_base_url);
            ui.label(t!("connections-field-d1-token"));
            ui.add(egui::TextEdit::singleline(&mut form.d1_token).password(true));
        }
        KindSelector::Postgres => {
            ui.label(t!("connections-field-pg-url"));
            ui.add(egui::TextEdit::singleline(&mut form.pg_url).password(true));
        }
        KindSelector::Neon => {
            // Neon shares the Postgres URL field semantically; we just
            // reuse the same Fluent key so all 11 locales stay synced
            // without a new tier-1 key for an identical concept.
            ui.label(t!("connections-field-pg-url"));
            ui.add(egui::TextEdit::singleline(&mut form.neon_url).password(true));
        }
        KindSelector::Supabase => {
            // Supabase is also pg-wire — same reasoning as Neon: reuse
            // the existing field key rather than fan out a synonym
            // across all 11 locales (ADR-0019).
            ui.label(t!("connections-field-pg-url"));
            ui.add(egui::TextEdit::singleline(&mut form.supabase_url).password(true));
        }
        KindSelector::AuroraDsql => {
            // Aurora DSQL is pg-wire too (ADR-0021); reuse the existing
            // tier-1 Fluent key so the 11-locale catalog stays stable.
            // The URL's password segment is expected to carry a
            // short-lived IAM authentication token (~15 min TTL).
            ui.label(t!("connections-field-pg-url"));
            ui.add(egui::TextEdit::singleline(&mut form.aurora_dsql_url).password(true));
        }
    }
    false
}

fn render_edit_form(ui: &mut egui::Ui, id: &str, form: &mut EditFormState) {
    egui::Grid::new("connections-edit-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("connections-field-id"));
            // Id is read-only on edit; render as a disabled text field
            // so it is still selectable for copy-paste.
            let mut id_buf = id.to_string();
            ui.add_enabled(false, egui::TextEdit::singleline(&mut id_buf));
            ui.end_row();
            ui.label(t!("connections-field-name"));
            ui.text_edit_singleline(&mut form.name);
            ui.end_row();
        });
    ui.separator();
    match &mut form.kind {
        EditKindState::Turso { path } => {
            ui.label(t!("connections-field-turso-path"));
            ui.text_edit_singleline(path);
        }
        EditKindState::D1 {
            account_id,
            database_id,
            base_url,
            replace_token,
            new_token,
        } => {
            ui.label(t!("connections-field-d1-account"));
            ui.text_edit_singleline(account_id);
            ui.label(t!("connections-field-d1-database"));
            ui.text_edit_singleline(database_id);
            ui.label(t!("connections-field-d1-base-url"));
            ui.text_edit_singleline(base_url);
            ui.checkbox(replace_token, t!("connections-replace-token"));
            ui.add_enabled(
                *replace_token,
                egui::TextEdit::singleline(new_token).password(true),
            );
        }
        EditKindState::Postgres {
            replace_url,
            new_url,
        }
        | EditKindState::Neon {
            replace_url,
            new_url,
        }
        | EditKindState::Supabase {
            replace_url,
            new_url,
        }
        | EditKindState::AuroraDsql {
            replace_url,
            new_url,
        } => {
            ui.checkbox(replace_url, t!("connections-replace-url"));
            ui.add_enabled(
                *replace_url,
                egui::TextEdit::singleline(new_url).password(true),
            );
        }
        // Config-file-only (ADR-0036): the list gates Edit off for this
        // kind, so this arm is never reached — no editable fields to show.
        EditKindState::AuroraDsqlIam => {}
    }
}

fn kind_selector_label(kind: KindSelector) -> &'static str {
    match kind {
        KindSelector::Turso => "Turso",
        KindSelector::D1 => "Cloudflare D1",
        KindSelector::Postgres => "Postgres",
        KindSelector::Neon => "Neon",
        KindSelector::Supabase => "Supabase",
        KindSelector::AuroraDsql => "Aurora DSQL",
    }
}

/// The native file extension for an encrypted connection bundle
/// (ADR-0038). "dbbx" = **dbb**oard e**x**port.
const BUNDLE_EXTENSION: &str = "dbbx";
/// Default file name the Save dialog opens with.
const BUNDLE_DEFAULT_FILE: &str = "dbboard-connections.dbbx";

/// Render the Export passphrase form (ADR-0038). The user types the
/// passphrase twice; [`ConnectionsView::submit_export`] rejects a
/// mismatch before any crypto runs.
fn render_export_form(ui: &mut egui::Ui, form: &mut ExportFormState) {
    ui.heading(t!("connections-export-heading"));
    ui.label(t!("connections-export-passphrase-hint"));
    egui::Grid::new("connections-export-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("connections-passphrase"));
            ui.add(egui::TextEdit::singleline(&mut form.passphrase).password(true));
            ui.end_row();
            ui.label(t!("connections-passphrase-confirm"));
            ui.add(egui::TextEdit::singleline(&mut form.confirm).password(true));
            ui.end_row();
        });
    ui.separator();
}

/// Render the Import passphrase form (ADR-0038). Returns `true` when the
/// "Choose file…" button was clicked so the caller can open the native
/// picker (kept out of this fn to keep it headless-testable).
fn render_import_form(ui: &mut egui::Ui, form: &mut ImportFormState) -> bool {
    ui.heading(t!("connections-import-heading"));
    ui.label(t!("connections-import-passphrase-hint"));
    let mut choose = false;
    ui.horizontal(|ui| {
        choose = ui.button(t!("connections-choose-file")).clicked();
        let name = if form.file_name.is_empty() {
            t!("connections-no-file-chosen")
        } else {
            form.file_name.clone()
        };
        ui.label(name);
    });
    egui::Grid::new("connections-import-grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("connections-passphrase"));
            ui.add(egui::TextEdit::singleline(&mut form.passphrase).password(true));
            ui.end_row();
        });
    ui.separator();
    choose
}

/// Render the confirm / cancel button row shared by the Export and Import
/// forms. The primary button is disabled when `enabled` is false (Import
/// with no file chosen yet). Returns `(primary_clicked, cancel_clicked)`.
fn render_transfer_buttons(ui: &mut egui::Ui, primary_label: &str, enabled: bool) -> (bool, bool) {
    let mut primary = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        primary = ui
            .add_enabled(enabled, egui::Button::new(primary_label))
            .clicked();
        cancel = ui.button(t!("connections-cancel-button")).clicked();
    });
    (primary, cancel)
}

/// Outcome of driving the native "Save As" dialog for an export bundle.
#[derive(Debug)]
enum SaveOutcome {
    /// The blob was written to the user-chosen path.
    Written,
    /// The user dismissed the dialog without choosing a path.
    Cancelled,
    /// A path was chosen but the write failed; carries a display message.
    Failed(String),
}

/// Open a native "Save As" dialog and write `blob` to the chosen path
/// (ADR-0038). The default file name uses the `.dbbx` extension. Mirrors
/// the CSV export glue in `lib.rs`: rfd for the dialog, `std::fs::write`
/// for the bytes, and the outcome mapped back to the caller rather than
/// surfaced via a message box here.
fn save_bundle_via_dialog(blob: &[u8]) -> SaveOutcome {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(t!("connections-bundle-filter"), &[BUNDLE_EXTENSION])
        .set_file_name(BUNDLE_DEFAULT_FILE)
        .save_file()
    else {
        return SaveOutcome::Cancelled;
    };
    match std::fs::write(&path, blob) {
        Ok(()) => SaveOutcome::Written,
        Err(err) => SaveOutcome::Failed(err.to_string()),
    }
}

/// Open a native file picker for an existing bundle (ADR-0038). Returns
/// the chosen path plus its file-name label, or `None` if dismissed.
fn pick_bundle_file() -> Option<(PathBuf, String)> {
    let path = rfd::FileDialog::new()
        .add_filter(t!("connections-bundle-filter"), &[BUNDLE_EXTENSION])
        .pick_file()?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some((path, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_config::{ConnectionFile, InMemorySecretStore, SecretStore};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn build_admin() -> (tempfile::TempDir, Arc<InMemorySecretStore>, ConnectionAdmin) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let admin = ConnectionAdmin::new_with_file(
            path,
            secrets.clone() as Arc<dyn SecretStore>,
            ConnectionFile::empty(),
        );
        (dir, secrets, admin)
    }

    #[test]
    fn new_view_starts_closed_in_list_mode_with_no_error() {
        let view = ConnectionsView::new();
        assert!(!view.is_open());
        assert!(matches!(view.mode(), Mode::List));
        assert!(view.last_error().is_none());
    }

    #[test]
    fn open_and_close_toggle_visibility_only() {
        let mut view = ConnectionsView::new();
        view.open();
        assert!(view.is_open());
        view.close();
        assert!(!view.is_open());
        assert!(matches!(view.mode(), Mode::List));
    }

    #[test]
    fn start_add_switches_to_a_blank_add_form() {
        let mut view = ConnectionsView::new();
        view.start_add();
        match view.mode() {
            Mode::Add(form) => {
                assert_eq!(form.id, "");
                assert_eq!(form.name, "");
                assert_eq!(form.kind, KindSelector::Turso);
            }
            other => panic!("expected Add, got {other:?}"),
        }
    }

    #[test]
    fn start_edit_prefills_from_the_existing_entry_without_secret() {
        let mut view = ConnectionsView::new();
        let entry = ConnectionEntry {
            id: "prod".into(),
            name: "Prod".into(),
            kind: ConnectionKind::D1 {
                account_id: "acct".into(),
                database_id: "db".into(),
                base_url: Some("https://example.test".into()),
                keyring_token_ref: "dbboard.prod.token".into(),
            },
        };
        view.start_edit(&entry);
        match view.mode() {
            Mode::Edit { id, form } => {
                assert_eq!(id, "prod");
                assert_eq!(form.name, "Prod");
                match &form.kind {
                    EditKindState::D1 {
                        account_id,
                        database_id,
                        base_url,
                        replace_token,
                        new_token,
                    } => {
                        assert_eq!(account_id, "acct");
                        assert_eq!(database_id, "db");
                        assert_eq!(base_url, "https://example.test");
                        // Secret defaults to Keep — the UI must never
                        // show the user a pre-filled secret.
                        assert!(!*replace_token);
                        assert!(new_token.is_empty());
                    }
                    other => panic!("expected D1 edit state, got {other:?}"),
                }
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn start_delete_records_the_entry_id_and_name() {
        let mut view = ConnectionsView::new();
        let entry = ConnectionEntry {
            id: "x".into(),
            name: "X DB".into(),
            kind: ConnectionKind::Turso {
                path: ":memory:".into(),
            },
        };
        view.start_delete(&entry);
        match view.mode() {
            Mode::ConfirmDelete { id, name } => {
                assert_eq!(id, "x");
                assert_eq!(name, "X DB");
            }
            other => panic!("expected ConfirmDelete, got {other:?}"),
        }
    }

    #[test]
    fn cancel_returns_to_list_and_clears_last_error() {
        let mut view = ConnectionsView::new();
        view.last_error = Some(DisplayError::plain("stale"));
        view.start_add();
        view.cancel();
        assert!(matches!(view.mode(), Mode::List));
        assert!(view.last_error().is_none());
    }

    #[test]
    fn submit_add_turso_persists_via_admin_and_returns_to_list() {
        let (_dir, _secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "local".into();
            form.name = "Local".into();
            form.kind = KindSelector::Turso;
            form.turso_path = ":memory:".into();
        }
        view.submit_add(&mut admin).expect("submit_add");
        assert!(matches!(view.mode(), Mode::List));
        assert_eq!(admin.entries().len(), 1);
        assert_eq!(admin.entries()[0].id, "local");
    }

    #[test]
    fn submit_add_d1_routes_the_token_through_the_secret_store() {
        let (_dir, secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "prod".into();
            form.name = "Prod".into();
            form.kind = KindSelector::D1;
            form.d1_account_id = "acct".into();
            form.d1_database_id = "db".into();
            form.d1_base_url = "  ".into(); // whitespace → None
            form.d1_token = "t0k3n".into();
        }
        view.submit_add(&mut admin).expect("submit_add");

        match &admin.entries()[0].kind {
            ConnectionKind::D1 {
                account_id,
                database_id,
                base_url,
                keyring_token_ref,
            } => {
                assert_eq!(account_id, "acct");
                assert_eq!(database_id, "db");
                assert!(base_url.is_none()); // whitespace-only is dropped
                assert_eq!(keyring_token_ref, "dbboard.prod.token");
            }
            other => panic!("expected D1, got {other:?}"),
        }
        assert_eq!(secrets.get("dbboard.prod.token").expect("token"), "t0k3n");
    }

    #[test]
    fn submit_add_postgres_routes_the_url_through_the_secret_store() {
        let (_dir, secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "neon".into();
            form.name = "Neon".into();
            form.kind = KindSelector::Postgres;
            form.pg_url = "postgres://example/db".into();
        }
        view.submit_add(&mut admin).expect("submit_add");
        assert_eq!(
            secrets.get("dbboard.neon.url").expect("url"),
            "postgres://example/db"
        );
    }

    #[test]
    fn submit_add_neon_routes_the_url_through_the_secret_store_and_records_neon_kind() {
        let (_dir, secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "prod-neon".into();
            form.name = "Prod Neon".into();
            form.kind = KindSelector::Neon;
            form.neon_url = "postgres://neon.example/db?sslmode=require".into();
        }
        view.submit_add(&mut admin).expect("submit_add");

        match &admin.entries()[0].kind {
            ConnectionKind::Neon { keyring_url_ref } => {
                assert_eq!(keyring_url_ref, "dbboard.prod-neon.url");
            }
            other => panic!("expected Neon, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.prod-neon.url").expect("url"),
            "postgres://neon.example/db?sslmode=require"
        );
    }

    #[test]
    fn start_edit_on_neon_entry_prefills_without_secret() {
        let mut view = ConnectionsView::new();
        let entry = ConnectionEntry {
            id: "n".into(),
            name: "N".into(),
            kind: ConnectionKind::Neon {
                keyring_url_ref: "dbboard.n.url".into(),
            },
        };
        view.start_edit(&entry);
        match view.mode() {
            Mode::Edit { id, form } => {
                assert_eq!(id, "n");
                match &form.kind {
                    EditKindState::Neon {
                        replace_url,
                        new_url,
                    } => {
                        assert!(!*replace_url);
                        assert!(new_url.is_empty());
                    }
                    other => panic!("expected Neon edit state, got {other:?}"),
                }
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn submit_edit_on_neon_with_replace_url_true_overwrites_the_secret() {
        let (_dir, secrets, mut admin) = build_admin();
        admin
            .add(ConnectionDraft {
                id: "n".into(),
                name: "N".into(),
                kind: ConnectionKindDraft::Neon {
                    url: "postgres://neon.example/old".into(),
                },
            })
            .expect("seed");

        let mut view = ConnectionsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            if let EditKindState::Neon {
                replace_url,
                new_url,
            } = &mut form.kind
            {
                *replace_url = true;
                *new_url = "postgres://neon.example/new".into();
            }
        }
        view.submit_edit(&mut admin).expect("submit_edit");
        assert_eq!(
            secrets.get("dbboard.n.url").expect("url"),
            "postgres://neon.example/new"
        );
    }

    #[test]
    fn submit_add_supabase_routes_the_url_through_the_secret_store_and_records_supabase_kind() {
        let (_dir, secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "prod-supa".into();
            form.name = "Prod Supabase".into();
            form.kind = KindSelector::Supabase;
            form.supabase_url = "postgres://supabase.example/db?sslmode=require".into();
        }
        view.submit_add(&mut admin).expect("submit_add");

        match &admin.entries()[0].kind {
            ConnectionKind::Supabase { keyring_url_ref } => {
                assert_eq!(keyring_url_ref, "dbboard.prod-supa.url");
            }
            other => panic!("expected Supabase, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.prod-supa.url").expect("url"),
            "postgres://supabase.example/db?sslmode=require"
        );
    }

    #[test]
    fn start_edit_on_supabase_entry_prefills_without_secret() {
        let mut view = ConnectionsView::new();
        let entry = ConnectionEntry {
            id: "s".into(),
            name: "S".into(),
            kind: ConnectionKind::Supabase {
                keyring_url_ref: "dbboard.s.url".into(),
            },
        };
        view.start_edit(&entry);
        match view.mode() {
            Mode::Edit { id, form } => {
                assert_eq!(id, "s");
                match &form.kind {
                    EditKindState::Supabase {
                        replace_url,
                        new_url,
                    } => {
                        assert!(!*replace_url);
                        assert!(new_url.is_empty());
                    }
                    other => panic!("expected Supabase edit state, got {other:?}"),
                }
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn submit_edit_on_supabase_with_replace_url_true_overwrites_the_secret() {
        let (_dir, secrets, mut admin) = build_admin();
        admin
            .add(ConnectionDraft {
                id: "s".into(),
                name: "S".into(),
                kind: ConnectionKindDraft::Supabase {
                    url: "postgres://supabase.example/old".into(),
                },
            })
            .expect("seed");

        let mut view = ConnectionsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            if let EditKindState::Supabase {
                replace_url,
                new_url,
            } = &mut form.kind
            {
                *replace_url = true;
                *new_url = "postgres://supabase.example/new".into();
            }
        }
        view.submit_edit(&mut admin).expect("submit_edit");
        assert_eq!(
            secrets.get("dbboard.s.url").expect("url"),
            "postgres://supabase.example/new"
        );
    }

    #[test]
    fn submit_add_aurora_dsql_routes_the_url_through_the_secret_store_and_records_aurora_dsql_kind()
    {
        let (_dir, secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "dsql-prod".into();
            form.name = "DSQL Prod".into();
            form.kind = KindSelector::AuroraDsql;
            form.aurora_dsql_url =
                "postgres://admin:iam-token@example.dsql.us-east-1.on.aws/postgres?sslmode=require"
                    .into();
        }
        view.submit_add(&mut admin).expect("submit_add");

        match &admin.entries()[0].kind {
            ConnectionKind::AuroraDsql { keyring_url_ref } => {
                assert_eq!(keyring_url_ref, "dbboard.dsql-prod.url");
            }
            other => panic!("expected AuroraDsql, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.dsql-prod.url").expect("url"),
            "postgres://admin:iam-token@example.dsql.us-east-1.on.aws/postgres?sslmode=require"
        );
    }

    #[test]
    fn start_edit_on_aurora_dsql_entry_prefills_without_secret() {
        let mut view = ConnectionsView::new();
        let entry = ConnectionEntry {
            id: "d".into(),
            name: "D".into(),
            kind: ConnectionKind::AuroraDsql {
                keyring_url_ref: "dbboard.d.url".into(),
            },
        };
        view.start_edit(&entry);
        match view.mode() {
            Mode::Edit { id, form } => {
                assert_eq!(id, "d");
                match &form.kind {
                    EditKindState::AuroraDsql {
                        replace_url,
                        new_url,
                    } => {
                        assert!(!*replace_url);
                        assert!(new_url.is_empty());
                    }
                    other => panic!("expected AuroraDsql edit state, got {other:?}"),
                }
            }
            other => panic!("expected Edit, got {other:?}"),
        }
    }

    #[test]
    fn submit_edit_on_aurora_dsql_with_replace_url_true_overwrites_the_secret() {
        let (_dir, secrets, mut admin) = build_admin();
        admin
            .add(ConnectionDraft {
                id: "d".into(),
                name: "D".into(),
                kind: ConnectionKindDraft::AuroraDsql {
                    url: "postgres://admin:old@example.dsql.us-east-1.on.aws/postgres".into(),
                },
            })
            .expect("seed");

        let mut view = ConnectionsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            if let EditKindState::AuroraDsql {
                replace_url,
                new_url,
            } = &mut form.kind
            {
                *replace_url = true;
                *new_url = "postgres://admin:new@example.dsql.us-east-1.on.aws/postgres".into();
            }
        }
        view.submit_edit(&mut admin).expect("submit_edit");
        assert_eq!(
            secrets.get("dbboard.d.url").expect("url"),
            "postgres://admin:new@example.dsql.us-east-1.on.aws/postgres"
        );
    }

    #[test]
    fn submit_add_duplicate_id_keeps_the_form_open_and_records_the_error() {
        let (_dir, _secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        // Pre-populate via admin so the second add collides.
        admin
            .add(ConnectionDraft {
                id: "dup".into(),
                name: "First".into(),
                kind: ConnectionKindDraft::Turso {
                    path: ":memory:".into(),
                },
            })
            .expect("seed");

        view.start_add();
        if let Mode::Add(form) = &mut view.mode {
            form.id = "dup".into();
            form.name = "Second".into();
            form.turso_path = ":memory:".into();
        }
        let err = view.submit_add(&mut admin).expect_err("dup must fail");
        assert!(matches!(err, ConfigError::DuplicateId(_)));
        // The form is still open so the user can fix the id.
        assert!(matches!(view.mode(), Mode::Add(_)));
        assert!(view.last_error().is_some());
    }

    #[test]
    fn submit_edit_with_replace_token_false_keeps_the_existing_secret() {
        let (_dir, secrets, mut admin) = build_admin();
        admin
            .add(ConnectionDraft {
                id: "prod".into(),
                name: "Prod".into(),
                kind: ConnectionKindDraft::D1 {
                    account_id: "acct".into(),
                    database_id: "db".into(),
                    base_url: None,
                    token: "original".into(),
                },
            })
            .expect("seed");

        let mut view = ConnectionsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            form.name = "Renamed".into();
            // replace_token left false (default) → secret untouched.
        }
        view.submit_edit(&mut admin).expect("submit_edit");

        assert_eq!(admin.entries()[0].name, "Renamed");
        assert_eq!(
            secrets.get("dbboard.prod.token").expect("token"),
            "original"
        );
    }

    #[test]
    fn submit_edit_with_replace_token_true_overwrites_the_secret() {
        let (_dir, secrets, mut admin) = build_admin();
        admin
            .add(ConnectionDraft {
                id: "prod".into(),
                name: "Prod".into(),
                kind: ConnectionKindDraft::D1 {
                    account_id: "acct".into(),
                    database_id: "db".into(),
                    base_url: None,
                    token: "original".into(),
                },
            })
            .expect("seed");

        let mut view = ConnectionsView::new();
        view.start_edit(&admin.entries()[0].clone());
        if let Mode::Edit { form, .. } = &mut view.mode {
            if let EditKindState::D1 {
                replace_token,
                new_token,
                ..
            } = &mut form.kind
            {
                *replace_token = true;
                *new_token = "rotated".into();
            }
        }
        view.submit_edit(&mut admin).expect("submit_edit");
        assert_eq!(secrets.get("dbboard.prod.token").expect("token"), "rotated");
    }

    #[test]
    fn submit_delete_removes_the_entry_and_returns_to_list() {
        let (_dir, _secrets, mut admin) = build_admin();
        admin
            .add(ConnectionDraft {
                id: "x".into(),
                name: "X".into(),
                kind: ConnectionKindDraft::Turso {
                    path: ":memory:".into(),
                },
            })
            .expect("seed");

        let mut view = ConnectionsView::new();
        view.start_delete(&admin.entries()[0].clone());
        view.submit_delete(&mut admin).expect("submit_delete");
        assert!(admin.entries().is_empty());
        assert!(matches!(view.mode(), Mode::List));
    }

    #[test]
    fn submit_add_outside_of_add_mode_is_a_noop() {
        let (_dir, _secrets, mut admin) = build_admin();
        let mut view = ConnectionsView::new();
        // Mode is List (default), not Add.
        view.submit_add(&mut admin).expect("noop");
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn add_form_state_to_draft_drops_whitespace_only_base_url() {
        let mut form = AddFormState {
            id: "x".into(),
            name: "X".into(),
            kind: KindSelector::D1,
            d1_account_id: "a".into(),
            d1_database_id: "b".into(),
            d1_base_url: "   ".into(),
            d1_token: "t".into(),
            ..Default::default()
        };
        match form.to_draft().kind {
            ConnectionKindDraft::D1 { base_url, .. } => assert!(base_url.is_none()),
            other => panic!("expected D1, got {other:?}"),
        }
        form.d1_base_url = "https://example.test".into();
        match form.to_draft().kind {
            ConnectionKindDraft::D1 { base_url, .. } => {
                assert_eq!(base_url.as_deref(), Some("https://example.test"));
            }
            other => panic!("expected D1, got {other:?}"),
        }
    }

    // --- ADR-0020 in-process connection switching ---

    #[test]
    fn new_view_has_no_pending_connect_request() {
        let mut view = ConnectionsView::new();
        assert!(view.take_pending_connect().is_none());
    }

    #[test]
    fn request_connect_records_id_then_taking_clears_it() {
        let mut view = ConnectionsView::new();
        view.request_connect("prod-pg");
        assert_eq!(view.take_pending_connect().as_deref(), Some("prod-pg"));
        // Drained: a subsequent take returns None until the next request.
        assert!(view.take_pending_connect().is_none());
    }

    // --- ADR-0036 reconnect (recovery) button ---

    #[test]
    fn active_row_offers_reconnect_and_inactive_row_offers_connect() {
        // Pins the ADR-0036 decision that reversed ADR-0020's disabled
        // active button: the live connection must expose an enabled
        // Reconnect so an expired-token adapter can be rebuilt in place.
        assert_eq!(row_connect_action(true), RowConnectAction::Reconnect);
        assert_eq!(row_connect_action(false), RowConnectAction::Connect);
    }

    #[test]
    fn reconnecting_the_active_connection_records_its_id_for_the_host() {
        // The Reconnect button funnels through the same request path as
        // Connect, so re-selecting the active id reaches the worker and
        // triggers a fresh adapter build (new IAM token).
        let mut view = ConnectionsView::new();
        view.request_connect("store-b");
        assert_eq!(view.take_pending_connect().as_deref(), Some("store-b"));
    }

    #[test]
    fn request_connect_overwrites_a_prior_unread_request() {
        // Two clicks before the host drains: only the most recent wins;
        // older intent is stale and should not be replayed.
        let mut view = ConnectionsView::new();
        view.request_connect("a");
        view.request_connect("b");
        assert_eq!(view.take_pending_connect().as_deref(), Some("b"));
        assert!(view.take_pending_connect().is_none());
    }

    // --- ADR-0038 encrypted export / import ---

    fn seed_one(admin: &mut ConnectionAdmin) {
        admin
            .add(ConnectionDraft {
                id: "local".into(),
                name: "Local".into(),
                kind: ConnectionKindDraft::Turso {
                    path: ":memory:".into(),
                },
            })
            .expect("seed");
    }

    #[test]
    fn start_export_switches_to_a_blank_export_form_and_clears_messages() {
        let mut view = ConnectionsView::new();
        view.last_error = Some(DisplayError::plain("stale"));
        view.last_info = Some("old".into());
        view.start_export();
        match view.mode() {
            Mode::Export(form) => {
                assert!(form.passphrase.is_empty());
                assert!(form.confirm.is_empty());
            }
            other => panic!("expected Export, got {other:?}"),
        }
        assert!(view.last_error().is_none());
        assert!(view.last_info().is_none());
    }

    #[test]
    fn start_import_switches_to_a_blank_import_form_and_clears_messages() {
        let mut view = ConnectionsView::new();
        view.last_info = Some("old".into());
        view.start_import();
        match view.mode() {
            Mode::Import(form) => {
                assert!(form.passphrase.is_empty());
                assert!(form.file_path.is_none());
            }
            other => panic!("expected Import, got {other:?}"),
        }
        assert!(view.last_info().is_none());
    }

    #[test]
    fn submit_export_with_mismatched_passphrases_reports_and_yields_no_blob() {
        let (_dir, _secrets, mut admin) = build_admin();
        seed_one(&mut admin);
        let mut view = ConnectionsView::new();
        view.start_export();
        if let Mode::Export(form) = &mut view.mode {
            form.passphrase = "correct horse".into();
            form.confirm = "wrong horse".into();
        }
        assert!(view.submit_export(&admin).is_none());
        assert!(view.last_error().is_some());
        // Form stays open so the user can fix the confirmation.
        assert!(matches!(view.mode(), Mode::Export(_)));
    }

    #[test]
    fn submit_export_with_matching_passphrase_yields_a_decryptable_blob() {
        let (_dir, _secrets, mut admin) = build_admin();
        seed_one(&mut admin);
        let mut view = ConnectionsView::new();
        view.start_export();
        if let Mode::Export(form) = &mut view.mode {
            form.passphrase = "correct horse battery".into();
            form.confirm = "correct horse battery".into();
        }
        let blob = view.submit_export(&admin).expect("blob");
        // The blob round-trips back through the admin importer under the
        // same passphrase, proving submit_export produced a real bundle.
        let (_dir2, _secrets2, mut fresh) = build_admin();
        let report = fresh
            .import_bundle(&blob, "correct horse battery")
            .expect("import");
        assert_eq!(report.imported, vec!["local".to_string()]);
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn submit_import_success_returns_to_list_with_a_green_summary() {
        // Export from one store, import into a fresh one via the view.
        let (_dir, _secrets, mut src) = build_admin();
        seed_one(&mut src);
        let blob = src.export_bundle("pw pw pw pw").expect("export");

        let (_dir2, _secrets2, mut dst) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_import();
        if let Mode::Import(form) = &mut view.mode {
            form.passphrase = "pw pw pw pw".into();
        }
        assert!(view.submit_import(&mut dst, &blob));
        assert!(matches!(view.mode(), Mode::List));
        assert!(view.last_error().is_none());
        let info = view.last_info().expect("summary");
        assert!(
            info.contains('1'),
            "summary should count 1 imported: {info}"
        );
        assert_eq!(dst.entries().len(), 1);
    }

    #[test]
    fn submit_import_with_wrong_passphrase_keeps_the_form_open_with_an_error() {
        let (_dir, _secrets, mut src) = build_admin();
        seed_one(&mut src);
        let blob = src.export_bundle("right pass phrase").expect("export");

        let (_dir2, _secrets2, mut dst) = build_admin();
        let mut view = ConnectionsView::new();
        view.start_import();
        if let Mode::Import(form) = &mut view.mode {
            form.passphrase = "wrong pass phrase".into();
        }
        assert!(!view.submit_import(&mut dst, &blob));
        assert!(view.last_error().is_some());
        assert!(matches!(view.mode(), Mode::Import(_)));
        assert!(dst.entries().is_empty());
    }

    #[test]
    fn submit_import_reports_skipped_ids_in_the_summary() {
        // Destination already owns "local"; importing the same bundle
        // must skip it and name it in the green summary.
        let (_dir, _secrets, mut src) = build_admin();
        seed_one(&mut src);
        let blob = src.export_bundle("pass pass pass").expect("export");

        let (_dir2, _secrets2, mut dst) = build_admin();
        seed_one(&mut dst);
        let mut view = ConnectionsView::new();
        view.start_import();
        if let Mode::Import(form) = &mut view.mode {
            form.passphrase = "pass pass pass".into();
        }
        assert!(view.submit_import(&mut dst, &blob));
        let info = view.last_info().expect("summary");
        assert!(info.contains("local"), "skipped id should appear: {info}");
        assert_eq!(dst.entries().len(), 1);
    }

    #[test]
    fn scrub_passphrases_zeroes_the_export_buffers_before_the_mode_swap() {
        // scrub_passphrases() is private, so the test can call it directly
        // and inspect the still-Export form — proving the buffers are
        // emptied rather than merely dropped (cancel() would drop them
        // regardless, giving no real regression signal).
        let mut view = ConnectionsView::new();
        view.start_export();
        if let Mode::Export(form) = &mut view.mode {
            form.passphrase = "secret material".into();
            form.confirm = "secret material".into();
        }
        view.scrub_passphrases();
        match view.mode() {
            Mode::Export(form) => {
                assert!(form.passphrase.is_empty());
                assert!(form.confirm.is_empty());
            }
            other => panic!("expected Export, got {other:?}"),
        }
    }

    #[test]
    fn cancel_from_export_returns_to_list() {
        let mut view = ConnectionsView::new();
        view.start_export();
        if let Mode::Export(form) = &mut view.mode {
            form.passphrase = "secret material".into();
            form.confirm = "secret material".into();
        }
        view.cancel();
        assert!(matches!(view.mode(), Mode::List));
    }

    #[test]
    fn submit_export_surfaces_a_weak_passphrase_error_and_yields_no_blob() {
        let (_dir, _secrets, mut admin) = build_admin();
        seed_one(&mut admin);
        let mut view = ConnectionsView::new();
        view.start_export();
        if let Mode::Export(form) = &mut view.mode {
            // Matches confirm, but below MIN_PASSPHRASE_LEN, so the crypto
            // core rejects it — exercising submit_export's Err branch.
            form.passphrase = "short".into();
            form.confirm = "short".into();
        }
        assert!(view.submit_export(&admin).is_none());
        assert!(view.last_error().is_some());
        assert!(matches!(view.mode(), Mode::Export(_)));
    }

    #[test]
    fn finish_export_returns_to_list_with_a_count_bearing_summary() {
        let mut view = ConnectionsView::new();
        view.start_export();
        if let Mode::Export(form) = &mut view.mode {
            form.passphrase = "secret material".into();
            form.confirm = "secret material".into();
        }
        view.finish_export(3);
        assert!(matches!(view.mode(), Mode::List));
        let info = view.last_info().expect("summary");
        assert!(info.contains('3'), "summary should carry the count: {info}");
    }
}
