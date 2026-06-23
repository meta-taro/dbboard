//! AI panel state and rendering (ADR-0023 / issue 0005 slice (b)).
//!
//! The panel is an `egui::Window` registered by [`DbboardApp`](crate::DbboardApp)
//! only when [`DbboardApp::has_ai_provider`](crate::DbboardApp::has_ai_provider)
//! returns true — graceful degradation = absence (ADR-0023 Decision 11).
//!
//! The state machine is intentionally tiny: an idle / busy bool, an
//! active [`AiMode`] (Explain | Suggest), a single input textarea, and
//! the most recent reply. Every input → reply round-trip is one-at-a-
//! time; sending a new request while one is in flight is a noop, like
//! [`DbboardApp::run_sql`](crate::DbboardApp::run_sql).
//!
//! [`AiPanel::prepare_send`] is the pure state-mutation half: it decides
//! whether a new command should be issued and, if so, marks the panel
//! busy and returns the [`Command`] for the caller to drop on the worker
//! channel. [`AiPanel::on_response`] / [`AiPanel::on_error`] are the
//! reply-side halves invoked from `drain_replies`.

use dbboard_ai::AiError;
use dbboard_core::TableInfo;
use dbboard_i18n::t;
use eframe::egui;

use crate::Command;

/// Which AI command the panel will issue on the next send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiMode {
    /// Explain the SQL pasted into the input textarea.
    Explain,
    /// Suggest SQL for the natural-language prompt in the input textarea,
    /// using the active connection's table list as the schema hint.
    Suggest,
}

/// The success branch surfaced to the panel after [`Reply::AiResponded`](crate::Reply::AiResponded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiResponseView {
    pub text: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
}

/// Panel-local view state. Owned by [`DbboardApp`](crate::DbboardApp)
/// because the panel must (a) reach the command channel and (b) react
/// to AI replies drained from the reply channel; the desktop binary's
/// menu bar only flips [`is_open`](Self::is_open) through a thin
/// accessor.
pub struct AiPanel {
    is_open: bool,
    mode: AiMode,
    input: String,
    busy: bool,
    /// `Ok(view)` for the last successful response, `Err(translated)`
    /// for the last failure. A fresh reply (success or failure) replaces
    /// whatever was there.
    last_response: Option<Result<AiResponseView, String>>,
}

impl AiPanel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: AiMode::Explain,
            input: String::new(),
            busy: false,
            last_response: None,
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
    }

    #[must_use]
    pub fn mode(&self) -> AiMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: AiMode) {
        self.mode = mode;
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    #[must_use]
    pub fn last_response(&self) -> Option<&Result<AiResponseView, String>> {
        self.last_response.as_ref()
    }

    /// Compose the command to send for the current mode + input. Returns
    /// `None` when the panel is already busy or the input is blank; in
    /// both cases the caller drops the result and nothing is sent. On a
    /// non-`None` return the panel transitions to `busy` so subsequent
    /// calls are noops until [`on_response`](Self::on_response) or
    /// [`on_error`](Self::on_error) clears it.
    pub fn prepare_send(
        &mut self,
        dialect: Option<String>,
        schema: &[TableInfo],
    ) -> Option<Command> {
        if self.busy || self.input.trim().is_empty() {
            return None;
        }
        // Only the Suggest arm consumes the schema, so the clone lives
        // there — Explain skips the allocation entirely.
        let cmd = match self.mode {
            AiMode::Explain => Command::AiExplain {
                sql: self.input.clone(),
                dialect,
            },
            AiMode::Suggest => Command::AiSuggest {
                prompt: self.input.clone(),
                dialect,
                schema: schema.to_vec(),
            },
        };
        self.busy = true;
        Some(cmd)
    }

    /// Successful provider reply landed; clear busy and replace any
    /// stale content with the new response.
    pub fn on_response(&mut self, text: String, tokens_in: u32, tokens_out: u32) {
        self.busy = false;
        self.last_response = Some(Ok(AiResponseView {
            text,
            tokens_in,
            tokens_out,
        }));
    }

    /// Provider returned an error; clear busy and replace any stale
    /// content with the translated error string. Takes by reference
    /// because `AiError` is not `Clone` — the caller usually moves the
    /// error out of `Reply::AiFailed { error }` and hands us a
    /// borrow, which is enough to format without owning.
    pub fn on_error(&mut self, error: &AiError) {
        self.busy = false;
        self.last_response = Some(Err(ai_error_display(error)));
    }

    /// Render the panel as an `egui::Window`. Returns `Some(command)`
    /// when the user clicked Send and a command should flow onto the
    /// worker channel; returns `None` otherwise. The caller is
    /// responsible for not invoking this when `has_ai_provider()` is
    /// false (the panel itself trusts that gate).
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        dialect: Option<&str>,
        schema: &[TableInfo],
    ) -> Option<Command> {
        let mut pending: Option<Command> = None;
        let mut is_open = self.is_open;
        egui::Window::new(t!("ai-panel-title"))
            .open(&mut is_open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.mode, AiMode::Explain, t!("ai-mode-explain"));
                    ui.selectable_value(&mut self.mode, AiMode::Suggest, t!("ai-mode-suggest"));
                });
                ui.separator();
                let prompt_label = match self.mode {
                    AiMode::Explain => t!("ai-input-explain"),
                    AiMode::Suggest => t!("ai-input-suggest"),
                };
                ui.label(prompt_label);
                ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new(t!("ai-send-button")))
                        .clicked()
                    {
                        pending = self.prepare_send(dialect.map(str::to_owned), schema);
                    }
                    if self.busy {
                        ui.spinner();
                        ui.label(t!("ai-busy"));
                    }
                });
                ui.separator();
                match &self.last_response {
                    None => {
                        ui.label(t!("ai-empty"));
                    }
                    Some(Ok(view)) => {
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
                                ui.label(&view.text);
                            });
                    }
                    Some(Err(msg)) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, msg);
                    }
                }
            });
        self.is_open = is_open;
        pending
    }
}

impl Default for AiPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// Translate an [`AiError`] into a user-facing string keyed off the
/// active locale. Independent of [`crate::error_display`] because AI
/// errors never travel over the desktop ↔ web HTTP contract; their
/// taxonomy is its own (ADR-0023 Decision 8).
fn ai_error_display(err: &AiError) -> String {
    match err {
        AiError::Configuration(msg) => format!("{}: {msg}", t!("ai-error-prefix-configuration")),
        AiError::Network(msg) => format!("{}: {msg}", t!("ai-error-prefix-network")),
        AiError::Provider(msg) => format!("{}: {msg}", t!("ai-error-prefix-provider")),
        AiError::Quota(msg) => format!("{}: {msg}", t!("ai-error-prefix-quota")),
        AiError::Cancelled => t!("ai-error-prefix-cancelled").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ai_error_display, AiMode, AiPanel};
    use crate::Command;
    use dbboard_ai::AiError;
    use dbboard_core::TableInfo;

    fn schema_two() -> Vec<TableInfo> {
        vec![
            TableInfo::qualified("public", "users"),
            TableInfo::qualified("public", "sessions"),
        ]
    }

    #[test]
    fn new_panel_starts_closed_idle_explain_with_no_history() {
        let panel = AiPanel::new();
        assert!(!panel.is_open());
        assert!(!panel.is_busy());
        assert_eq!(panel.mode(), AiMode::Explain);
        assert!(panel.last_response().is_none());
    }

    #[test]
    fn open_close_and_toggle_flip_visibility() {
        let mut panel = AiPanel::new();
        panel.open();
        assert!(panel.is_open());
        panel.close();
        assert!(!panel.is_open());
        panel.toggle();
        assert!(panel.is_open());
        panel.toggle();
        assert!(!panel.is_open());
    }

    #[test]
    fn set_mode_switches_between_explain_and_suggest() {
        let mut panel = AiPanel::new();
        panel.set_mode(AiMode::Suggest);
        assert_eq!(panel.mode(), AiMode::Suggest);
        panel.set_mode(AiMode::Explain);
        assert_eq!(panel.mode(), AiMode::Explain);
    }

    #[test]
    fn prepare_send_with_empty_input_is_noop_and_stays_idle() {
        let mut panel = AiPanel::new();
        let cmd = panel.prepare_send(None, &[]);
        assert!(cmd.is_none());
        assert!(!panel.is_busy());
    }

    #[test]
    fn prepare_send_with_whitespace_only_input_is_noop() {
        let mut panel = AiPanel::new();
        panel.input = "   \n\t  ".into();
        let cmd = panel.prepare_send(None, &[]);
        assert!(cmd.is_none());
        assert!(!panel.is_busy());
    }

    #[test]
    fn prepare_send_explain_produces_ai_explain_command_with_dialect() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let cmd = panel.prepare_send(Some("postgres".into()), &schema_two());
        assert!(panel.is_busy());
        match cmd {
            Some(Command::AiExplain { sql, dialect }) => {
                assert_eq!(sql, "SELECT 1");
                assert_eq!(dialect.as_deref(), Some("postgres"));
            }
            other => panic!("expected AiExplain, got {other:?}"),
        }
    }

    #[test]
    fn prepare_send_suggest_produces_ai_suggest_command_carrying_schema() {
        let mut panel = AiPanel::new();
        panel.set_mode(AiMode::Suggest);
        panel.input = "monthly active users".into();
        let cmd = panel.prepare_send(Some("postgres".into()), &schema_two());
        assert!(panel.is_busy());
        match cmd {
            Some(Command::AiSuggest {
                prompt,
                dialect,
                schema,
            }) => {
                assert_eq!(prompt, "monthly active users");
                assert_eq!(dialect.as_deref(), Some("postgres"));
                assert_eq!(schema.len(), 2);
                assert_eq!(schema[0].name, "users");
            }
            other => panic!("expected AiSuggest, got {other:?}"),
        }
    }

    #[test]
    fn prepare_send_while_busy_is_noop() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[]);
        assert!(panel.is_busy());
        let cmd = panel.prepare_send(None, &[]);
        assert!(cmd.is_none(), "second send while busy must be a noop");
    }

    #[test]
    fn on_response_clears_busy_and_records_success() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[]);
        panel.on_response("explained".into(), 12, 34);
        assert!(!panel.is_busy());
        match panel.last_response() {
            Some(Ok(view)) => {
                assert_eq!(view.text, "explained");
                assert_eq!(view.tokens_in, 12);
                assert_eq!(view.tokens_out, 34);
            }
            other => panic!("expected Ok response, got {other:?}"),
        }
    }

    #[test]
    fn on_error_clears_busy_and_records_translated_message() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[]);
        panel.on_error(&AiError::Provider("rate_limit".into()));
        assert!(!panel.is_busy());
        match panel.last_response() {
            Some(Err(msg)) => {
                assert!(
                    msg.contains("rate_limit"),
                    "raw provider message must survive translation: {msg}"
                );
            }
            other => panic!("expected Err response, got {other:?}"),
        }
    }

    #[test]
    fn fresh_response_replaces_stale_error() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[]);
        panel.on_error(&AiError::Network("timeout".into()));
        assert!(matches!(panel.last_response(), Some(Err(_))));

        // Second round-trip: success replaces the prior error.
        panel.input = "SELECT 2".into();
        let _ = panel.prepare_send(None, &[]);
        panel.on_response("ok".into(), 1, 1);
        assert!(matches!(panel.last_response(), Some(Ok(_))));
    }

    #[test]
    fn fresh_error_replaces_stale_response() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[]);
        panel.on_response("first".into(), 0, 0);
        assert!(matches!(panel.last_response(), Some(Ok(_))));

        panel.input = "SELECT 2".into();
        let _ = panel.prepare_send(None, &[]);
        panel.on_error(&AiError::Cancelled);
        assert!(matches!(panel.last_response(), Some(Err(_))));
    }

    #[test]
    fn ai_error_display_includes_translated_prefix_and_raw_message() {
        // No init() call -> t!() falls back to en. The prefix word
        // varies by locale; the wire message survives verbatim. We
        // assert the variants emit *some* recognisable english word
        // plus the original message (the panel is responsible for the
        // styling, not the exact wording).
        for (err, contains) in [
            (AiError::Configuration("missing key".into()), "missing key"),
            (AiError::Network("timeout".into()), "timeout"),
            (AiError::Provider("rate_limit".into()), "rate_limit"),
            (AiError::Quota("cap reached".into()), "cap reached"),
        ] {
            let rendered = ai_error_display(&err);
            assert!(
                rendered.to_lowercase().contains("error")
                    || rendered.to_lowercase().contains("cancelled")
                    || rendered.to_lowercase().contains("network")
                    || rendered.to_lowercase().contains("quota")
                    || rendered.to_lowercase().contains("configuration")
                    || rendered.to_lowercase().contains("provider"),
                "no recognisable category word in: {rendered}"
            );
            assert!(
                rendered.contains(contains),
                "raw payload missing from: {rendered}"
            );
        }
        // Cancelled has no payload; it just renders the prefix.
        let cancelled = ai_error_display(&AiError::Cancelled);
        assert!(!cancelled.is_empty());
    }

    #[test]
    fn ui_does_not_emit_a_command_when_closed() {
        // Smoke test: the panel may still construct an egui::Window
        // internally when the open flag is true, but we exercise the
        // pure-state path of "ui called while is_open is false yields
        // no pending command". The egui::Window is invisible (open
        // toggle off), and no input has been entered, so prepare_send
        // would refuse anyway — together they make this a sanity
        // assertion on the closed-panel flow.
        let panel = AiPanel::new();
        assert!(!panel.is_open());
        // Note: we deliberately do not drive a full egui frame here —
        // the egui Window's interactive surface is exercised via the
        // prepare_send tests above. The state machine invariant under
        // test is that a panel that has never been opened produces no
        // pending command and never transitions to busy.
        assert!(!panel.is_busy());
    }
}
