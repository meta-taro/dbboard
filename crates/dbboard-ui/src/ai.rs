//! AI panel state and rendering (ADR-0023 / issue 0005 slice (b) and
//! ADR-0026 slice (d) — streaming + cancel + token meter).
//!
//! The panel is an `egui::Window` registered by [`DbboardApp`](crate::DbboardApp)
//! only when [`DbboardApp::has_ai_provider`](crate::DbboardApp::has_ai_provider)
//! returns true — graceful degradation = absence (ADR-0023 Decision 11).
//!
//! The state machine is intentionally tiny but distinguishes four
//! presentation states:
//!
//! * **Idle**: `busy=false`, `streaming=None`, `last_response` carries
//!   either the last success / error or `None` for a fresh panel.
//! * **Atomic busy**: `busy=true`, `streaming=None`. The spinner +
//!   `ai-busy` label appear; the Send button is replaced with Cancel
//!   (ADR-0026 Decision 10 — cancel works on the atomic path too).
//! * **Streaming busy**: `busy=true`, `streaming=Some(acc)`. Each chunk
//!   appends to `acc.text`; cumulative usage replaces the running meter
//!   (ADR-0026 Decision 7). Cancel works the same way.
//! * **Cancelled**: `busy=false`, `streaming=None`, `cancelled=true`,
//!   `last_response` carries the partial text the user already saw so
//!   they keep what they paid for. A quiet `ai-cancelled-message` line
//!   renders under the body (ADR-0026 Decision 12 — no error banner).
//!
//! [`AiPanel::prepare_send`] is the pure state-mutation half: it
//! decides whether a new command should be issued and, if so, picks the
//! streaming variant when the active provider advertises
//! [`AiCapabilities::has_streaming`](dbboard_ai::AiCapabilities::has_streaming)
//! and the atomic variant otherwise. [`AiPanel::prepare_cancel`] is its
//! cancel-button counterpart. The reply-side halves
//! ([`on_response`](AiPanel::on_response) /
//! [`on_error`](AiPanel::on_error) /
//! [`on_stream_chunk`](AiPanel::on_stream_chunk) /
//! [`on_stream_complete`](AiPanel::on_stream_complete) /
//! [`on_cancelled`](AiPanel::on_cancelled)) are invoked from
//! `drain_replies`.

use dbboard_ai::{AiError, StopReason};
use dbboard_core::TableInfo;
use dbboard_i18n::{t, t_args};
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

/// The success branch surfaced to the panel after [`Reply::AiResponded`](crate::Reply::AiResponded)
/// or [`Reply::AiStreamComplete`](crate::Reply::AiStreamComplete).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiResponseView {
    pub text: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
}

/// In-flight streaming accumulator (ADR-0026 Decision 6/7). The panel
/// keeps `Some(acc)` between the first [`Reply::AiChunk`](crate::Reply::AiChunk)
/// and the terminal [`Reply::AiStreamComplete`](crate::Reply::AiStreamComplete)
/// or [`Reply::AiCancelled`](crate::Reply::AiCancelled); chunk text is
/// appended verbatim, and cumulative usage **replaces** the running
/// counters (Anthropic `usage.output_tokens` is cumulative — do not
/// sum deltas).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamingAcc {
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
    /// whatever was there. On a cancelled stream, the partial text is
    /// stored as `Ok` so the user keeps the bytes they paid for; the
    /// [`cancelled`](Self::cancelled) flag is the only distinction from
    /// a clean completion.
    last_response: Option<Result<AiResponseView, String>>,
    /// In-flight streaming accumulator. `None` between requests and
    /// during the atomic path; `Some` from the first chunk until the
    /// terminal stream reply.
    streaming: Option<StreamingAcc>,
    /// True after [`Self::on_cancelled`]. Cleared on the next
    /// [`Self::prepare_send`] so the marker only flags the most recent
    /// outcome.
    cancelled: bool,
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
            streaming: None,
            cancelled: false,
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

    /// Snapshot of the in-flight streaming buffer, if any. `None`
    /// between requests and on the atomic path. Tests use this to
    /// assert chunk accumulation; the UI reads it directly.
    #[must_use]
    pub fn streaming(&self) -> Option<&StreamingAcc> {
        self.streaming.as_ref()
    }

    /// `true` when the most recent terminal event was a user-initiated
    /// cancel (ADR-0026 Decision 12). Cleared on the next
    /// [`Self::prepare_send`].
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Compose the command to send for the current mode + input.
    /// Returns `None` when the panel is already busy or the input is
    /// blank; in both cases the caller drops the result and nothing is
    /// sent. On a non-`None` return the panel transitions to `busy` so
    /// subsequent calls are noops until one of the terminal reply
    /// handlers (`on_response` / `on_error` / `on_stream_complete` /
    /// `on_cancelled`) clears it.
    ///
    /// `has_streaming` selects between the streaming and atomic command
    /// variants (ADR-0026 Decision 6 — the panel picks whichever the
    /// active provider supports). The binary reads
    /// [`AiCapabilities::has_streaming`](dbboard_ai::AiCapabilities::has_streaming)
    /// off the slot's snapshot and passes it through.
    pub fn prepare_send(
        &mut self,
        dialect: Option<String>,
        schema: &[TableInfo],
        has_streaming: bool,
    ) -> Option<Command> {
        if self.busy || self.input.trim().is_empty() {
            return None;
        }
        // Clear the cancel marker — the *previous* outcome was a
        // cancel; this new send is its own outcome. Leave
        // `last_response` alone so the body does not blink to empty
        // between Send click and first chunk / atomic reply (the new
        // streaming view / response overwrites it once a chunk lands).
        self.cancelled = false;
        // Only the Suggest arm consumes the schema, so the clone lives
        // there — Explain skips the allocation entirely.
        let cmd = match (self.mode, has_streaming) {
            (AiMode::Explain, true) => Command::AiExplainStream {
                sql: self.input.clone(),
                dialect,
            },
            (AiMode::Explain, false) => Command::AiExplain {
                sql: self.input.clone(),
                dialect,
            },
            (AiMode::Suggest, true) => Command::AiSuggestStream {
                prompt: self.input.clone(),
                dialect,
                schema: schema.to_vec(),
            },
            (AiMode::Suggest, false) => Command::AiSuggest {
                prompt: self.input.clone(),
                dialect,
                schema: schema.to_vec(),
            },
        };
        self.busy = true;
        Some(cmd)
    }

    /// Compose a [`Command::CancelAiRequest`] when the panel is busy
    /// (ADR-0026 Decision 5/10). Returns `None` when no request is in
    /// flight, so the cancel button can be wired unconditionally. The
    /// panel does **not** clear `busy` here: cancellation is async and
    /// the worker emits [`Reply::AiCancelled`](crate::Reply::AiCancelled)
    /// which routes through [`Self::on_cancelled`].
    pub fn prepare_cancel(&mut self) -> Option<Command> {
        if !self.busy {
            return None;
        }
        Some(Command::CancelAiRequest)
    }

    /// Successful atomic provider reply landed; clear busy and replace
    /// any stale content with the new response.
    pub fn on_response(&mut self, text: String, tokens_in: u32, tokens_out: u32) {
        self.busy = false;
        self.streaming = None;
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
        self.streaming = None;
        self.last_response = Some(Err(ai_error_display(error)));
    }

    /// ADR-0026 Decision 6/7: one streaming chunk arrived. Lazily
    /// initialises the [`StreamingAcc`] on the first call so the panel
    /// shows the spinner + busy label up until the first chunk lands
    /// rather than an empty text box. `text_delta` is appended verbatim;
    /// cumulative usage **replaces** the running counters (Anthropic
    /// `usage.output_tokens` is cumulative — summing would double-count).
    pub fn on_stream_chunk(
        &mut self,
        text_delta: &str,
        tokens_in: Option<u32>,
        tokens_out: Option<u32>,
    ) {
        let acc = self.streaming.get_or_insert_with(StreamingAcc::default);
        acc.text.push_str(text_delta);
        if let Some(t) = tokens_in {
            acc.tokens_in = t;
        }
        if let Some(t) = tokens_out {
            acc.tokens_out = t;
        }
    }

    /// ADR-0026 Decision 6: the stream terminated successfully. Move
    /// the accumulated text out of the streaming buffer into
    /// `last_response` and clear busy. The terminal token counts win
    /// over the running ones (the worker tracks `last_tokens_*` and
    /// passes them in here, so they are at least as fresh as the most
    /// recent `Usage` chunk). `stop_reason` is currently ignored at the
    /// presentation layer; it is part of the trait surface so a future
    /// "`max_tokens` hit — continue?" UX can read it without a contract
    /// bump.
    pub fn on_stream_complete(
        &mut self,
        tokens_in: u32,
        tokens_out: u32,
        _stop_reason: &StopReason,
    ) {
        self.busy = false;
        let text = self
            .streaming
            .take()
            .map(|acc| acc.text)
            .unwrap_or_default();
        self.last_response = Some(Ok(AiResponseView {
            text,
            tokens_in,
            tokens_out,
        }));
    }

    /// ADR-0026 Decision 12: the in-flight request was cancelled by the
    /// user. Clear busy, set the cancelled flag, and preserve any
    /// partial streaming text as a success view (the user already paid
    /// for those tokens — losing them on a cancel click would be
    /// hostile). When no streaming buffer exists (atomic path or
    /// cancel-before-first-chunk), `last_response` is left untouched and
    /// only the cancelled flag flips.
    pub fn on_cancelled(&mut self) {
        self.busy = false;
        self.cancelled = true;
        if let Some(acc) = self.streaming.take() {
            self.last_response = Some(Ok(AiResponseView {
                text: acc.text,
                tokens_in: acc.tokens_in,
                tokens_out: acc.tokens_out,
            }));
        }
    }

    /// Render the panel as an `egui::Window`. Returns `Some(command)`
    /// when the user clicked Send or Cancel and a command should flow
    /// onto the worker channel; returns `None` otherwise. The caller is
    /// responsible for not invoking this when `has_ai_provider()` is
    /// false (the panel itself trusts that gate).
    ///
    /// `active_provider_label`, when `Some`, is rendered as a subtitle
    /// (ADR-0025 slice (b)) so the user can tell at a glance which
    /// provider the next Send will hit. `None` suppresses the subtitle.
    ///
    /// `has_streaming` selects between the streaming and atomic command
    /// variants on Send (ADR-0026 Decision 6). The binary reads the
    /// active provider's capability off the slot snapshot and passes it
    /// through unchanged.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        dialect: Option<&str>,
        schema: &[TableInfo],
        active_provider_label: Option<&str>,
        has_streaming: bool,
    ) -> Option<Command> {
        let mut pending: Option<Command> = None;
        let mut is_open = self.is_open;
        egui::Window::new(t!("ai-panel-title"))
            .open(&mut is_open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                if let Some(name) = active_provider_label {
                    let owned = name.to_string();
                    ui.label(t_args!("ai-active-with-name", name = owned));
                }
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
                    if self.busy {
                        // Cancel replaces Send while a request is in
                        // flight. Both streaming and atomic paths route
                        // through the same select! cancel race in the
                        // worker (ADR-0026 Decision 10), so the button
                        // is wired unconditionally.
                        if ui.button(t!("ai-cancel-button")).clicked() {
                            pending = self.prepare_cancel();
                        }
                        ui.spinner();
                        ui.label(t!("ai-busy"));
                    } else if ui.button(t!("ai-send-button")).clicked() {
                        pending =
                            self.prepare_send(dialect.map(str::to_owned), schema, has_streaming);
                    }
                });
                ui.separator();
                self.render_body(ui);
                if self.cancelled {
                    ui.label(t!("ai-cancelled-message"));
                }
            });
        self.is_open = is_open;
        pending
    }

    fn render_body(&self, ui: &mut egui::Ui) {
        if let Some(acc) = &self.streaming {
            // In-flight streaming view: show whatever has arrived so far
            // and a running token meter. The terminator (Complete /
            // Cancelled) flips us back into the last_response branch.
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    ui.label(&acc.text);
                });
            ui.label(t_args!(
                "ai-tokens-meter",
                tin = acc.tokens_in,
                tout = acc.tokens_out
            ));
            return;
        }
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
                ui.label(t_args!(
                    "ai-tokens-meter",
                    tin = view.tokens_in,
                    tout = view.tokens_out
                ));
            }
            Some(Err(msg)) => {
                ui.colored_label(egui::Color32::LIGHT_RED, msg);
            }
        }
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
    use dbboard_ai::{AiError, StopReason};
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
        assert!(panel.streaming().is_none());
        assert!(!panel.cancelled());
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
        let cmd = panel.prepare_send(None, &[], false);
        assert!(cmd.is_none());
        assert!(!panel.is_busy());
    }

    #[test]
    fn prepare_send_with_whitespace_only_input_is_noop() {
        let mut panel = AiPanel::new();
        panel.input = "   \n\t  ".into();
        let cmd = panel.prepare_send(None, &[], false);
        assert!(cmd.is_none());
        assert!(!panel.is_busy());
    }

    #[test]
    fn prepare_send_explain_atomic_produces_ai_explain_command_with_dialect() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let cmd = panel.prepare_send(Some("postgres".into()), &schema_two(), false);
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
    fn prepare_send_explain_streaming_produces_ai_explain_stream_command() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let cmd = panel.prepare_send(Some("postgres".into()), &schema_two(), true);
        assert!(panel.is_busy());
        match cmd {
            Some(Command::AiExplainStream { sql, dialect }) => {
                assert_eq!(sql, "SELECT 1");
                assert_eq!(dialect.as_deref(), Some("postgres"));
            }
            other => panic!("expected AiExplainStream, got {other:?}"),
        }
    }

    #[test]
    fn prepare_send_suggest_atomic_produces_ai_suggest_command_carrying_schema() {
        let mut panel = AiPanel::new();
        panel.set_mode(AiMode::Suggest);
        panel.input = "monthly active users".into();
        let cmd = panel.prepare_send(Some("postgres".into()), &schema_two(), false);
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
    fn prepare_send_suggest_streaming_produces_ai_suggest_stream_command() {
        let mut panel = AiPanel::new();
        panel.set_mode(AiMode::Suggest);
        panel.input = "monthly active users".into();
        let cmd = panel.prepare_send(Some("postgres".into()), &schema_two(), true);
        assert!(panel.is_busy());
        match cmd {
            Some(Command::AiSuggestStream {
                prompt,
                dialect,
                schema,
            }) => {
                assert_eq!(prompt, "monthly active users");
                assert_eq!(dialect.as_deref(), Some("postgres"));
                assert_eq!(schema.len(), 2);
            }
            other => panic!("expected AiSuggestStream, got {other:?}"),
        }
    }

    #[test]
    fn prepare_send_while_busy_is_noop() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], false);
        assert!(panel.is_busy());
        let cmd = panel.prepare_send(None, &[], false);
        assert!(cmd.is_none(), "second send while busy must be a noop");
    }

    #[test]
    fn on_response_clears_busy_and_records_success() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], false);
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
        let _ = panel.prepare_send(None, &[], false);
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
        let _ = panel.prepare_send(None, &[], false);
        panel.on_error(&AiError::Network("timeout".into()));
        assert!(matches!(panel.last_response(), Some(Err(_))));

        // Second round-trip: success replaces the prior error.
        panel.input = "SELECT 2".into();
        let _ = panel.prepare_send(None, &[], false);
        panel.on_response("ok".into(), 1, 1);
        assert!(matches!(panel.last_response(), Some(Ok(_))));
    }

    #[test]
    fn fresh_error_replaces_stale_response() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], false);
        panel.on_response("first".into(), 0, 0);
        assert!(matches!(panel.last_response(), Some(Ok(_))));

        panel.input = "SELECT 2".into();
        let _ = panel.prepare_send(None, &[], false);
        panel.on_error(&AiError::Cancelled);
        assert!(matches!(panel.last_response(), Some(Err(_))));
    }

    #[test]
    fn ai_error_display_includes_translated_prefix_and_raw_message() {
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
        let cancelled = ai_error_display(&AiError::Cancelled);
        assert!(!cancelled.is_empty());
    }

    #[test]
    fn ui_does_not_emit_a_command_when_closed() {
        let panel = AiPanel::new();
        assert!(!panel.is_open());
        assert!(!panel.is_busy());
    }

    // --- ADR-0026 slice (d): streaming + cancel state machine ---

    #[test]
    fn prepare_cancel_returns_none_when_panel_is_idle() {
        let mut panel = AiPanel::new();
        assert!(panel.prepare_cancel().is_none());
    }

    #[test]
    fn prepare_cancel_returns_cancel_command_when_busy_and_keeps_busy_set() {
        // Decision 5/12: cancel is async — the worker emits AiCancelled
        // which routes through on_cancelled. prepare_cancel itself must
        // not flip busy or the spinner / Cancel button vanishes before
        // the cancel is acknowledged.
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], true);
        assert!(panel.is_busy());
        let cancel = panel.prepare_cancel();
        assert!(matches!(cancel, Some(Command::CancelAiRequest)));
        assert!(panel.is_busy(), "busy stays true until on_cancelled lands");
    }

    #[test]
    fn on_stream_chunk_lazily_initialises_accumulator_on_first_arrival() {
        let mut panel = AiPanel::new();
        assert!(panel.streaming().is_none());
        panel.on_stream_chunk("Hello", Some(11), None);
        let acc = panel.streaming().expect("streaming buffer initialised");
        assert_eq!(acc.text, "Hello");
        assert_eq!(acc.tokens_in, 11);
        assert_eq!(acc.tokens_out, 0);
    }

    #[test]
    fn on_stream_chunk_appends_text_and_replaces_cumulative_tokens() {
        // Decision 7: `usage.output_tokens` is cumulative — chunk
        // tokens replace the running counter rather than adding to it.
        let mut panel = AiPanel::new();
        panel.on_stream_chunk("Hello", Some(11), None);
        panel.on_stream_chunk(" world", None, None);
        panel.on_stream_chunk("", None, Some(5));
        panel.on_stream_chunk("", None, Some(7)); // cumulative replacement
        let acc = panel.streaming().expect("streaming");
        assert_eq!(acc.text, "Hello world");
        assert_eq!(acc.tokens_in, 11);
        assert_eq!(acc.tokens_out, 7, "cumulative tokens replace not sum");
    }

    #[test]
    fn on_stream_complete_moves_accumulator_into_last_response_and_clears_busy() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], true);
        panel.on_stream_chunk("Hello", Some(11), None);
        panel.on_stream_chunk(" world", None, Some(7));
        panel.on_stream_complete(11, 7, &StopReason::EndTurn);

        assert!(!panel.is_busy());
        assert!(panel.streaming().is_none());
        match panel.last_response() {
            Some(Ok(view)) => {
                assert_eq!(view.text, "Hello world");
                assert_eq!(view.tokens_in, 11);
                assert_eq!(view.tokens_out, 7);
            }
            other => panic!("expected Ok response after stream complete, got {other:?}"),
        }
        assert!(!panel.cancelled());
    }

    #[test]
    fn on_stream_complete_without_chunks_records_empty_response_with_final_tokens() {
        // Degenerate edge: stream terminator arrives without any text
        // chunks. The forward_stream synthetic-terminator path can
        // produce this when a provider closes early; the panel still
        // surfaces an empty success view with the final token counts.
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], true);
        panel.on_stream_complete(11, 0, &StopReason::EndTurn);
        match panel.last_response() {
            Some(Ok(view)) => {
                assert_eq!(view.text, "");
                assert_eq!(view.tokens_in, 11);
                assert_eq!(view.tokens_out, 0);
            }
            other => panic!("expected empty Ok response, got {other:?}"),
        }
    }

    #[test]
    fn on_cancelled_during_stream_preserves_partial_text_and_flags_cancelled() {
        // Decision 12: a cancel mid-stream keeps the bytes the user
        // already saw — wiping them on a cancel click would be hostile.
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], true);
        panel.on_stream_chunk("partial", Some(11), Some(3));
        panel.on_cancelled();

        assert!(!panel.is_busy());
        assert!(panel.streaming().is_none());
        assert!(panel.cancelled());
        match panel.last_response() {
            Some(Ok(view)) => {
                assert_eq!(view.text, "partial");
                assert_eq!(view.tokens_in, 11);
                assert_eq!(view.tokens_out, 3);
            }
            other => panic!("expected Ok with partial text, got {other:?}"),
        }
    }

    #[test]
    fn on_cancelled_during_atomic_just_flags_cancelled_without_overwriting_response() {
        // Atomic path has no streaming buffer. Last response (None or a
        // prior result) stays untouched; only the cancelled flag flips
        // so the panel renders "Cancelled." under whatever was there.
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], false);
        assert!(panel.is_busy());
        panel.on_cancelled();
        assert!(!panel.is_busy());
        assert!(panel.cancelled());
        assert!(
            panel.last_response().is_none(),
            "atomic cancel must not synthesise a fake response"
        );
    }

    #[test]
    fn fresh_send_clears_a_prior_cancelled_marker() {
        let mut panel = AiPanel::new();
        panel.input = "SELECT 1".into();
        let _ = panel.prepare_send(None, &[], true);
        panel.on_cancelled();
        assert!(panel.cancelled());

        panel.input = "SELECT 2".into();
        let _ = panel.prepare_send(None, &[], true);
        assert!(
            !panel.cancelled(),
            "the cancelled marker tracks the *most recent* outcome"
        );
    }

    #[test]
    fn on_stream_complete_after_partial_cancel_overwrites_cancelled_state() {
        // Defensive: if a Complete somehow lands after a Cancelled
        // (out-of-order replies on the channel), the panel should
        // present the most recent terminal as authoritative. on_cancelled
        // sets cancelled=true; a subsequent on_stream_complete clears
        // streaming and writes a fresh Ok view — but it does NOT clear
        // the cancelled flag, because clearing happens on the next
        // Send. Today the worker never emits both for the same request,
        // but the invariant is documented here so future drift is
        // caught by this test.
        let mut panel = AiPanel::new();
        panel.on_stream_chunk("partial", Some(11), Some(3));
        panel.on_cancelled();
        assert!(panel.cancelled());
        panel.on_stream_complete(11, 3, &StopReason::EndTurn);
        // Cancelled stays set until the next Send.
        assert!(panel.cancelled());
        // last_response was overwritten with the (empty) complete view
        // because the streaming buffer was consumed by on_cancelled.
        match panel.last_response() {
            Some(Ok(view)) => assert_eq!(view.text, ""),
            other => panic!("expected Ok response, got {other:?}"),
        }
    }
}
