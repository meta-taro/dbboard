//! Unified error display (ADR-0039).
//!
//! Every app-side error the UI surfaces is rendered the same way: a
//! message in the active locale, the original English text beneath it,
//! and both selectable and copyable. A non-technical user can drag-select
//! (Ctrl+C) or press the Copy button and paste the English half straight
//! into a web search or an AI assistant.
//!
//! [`DisplayError`] is the value the UI stores. The `*_display` producers
//! translate each error taxonomy into one: the localized half comes from
//! Fluent (`t!` / `t_args!`), the original half from the error type's own
//! `Display`. DB/SQL error *bodies* stay verbatim — they are provided by
//! the remote server (ADR-0009 / ADR-0015) — so only their category
//! prefix is localized.

use dbboard_ai::AiError;
use dbboard_config::{AiSettingsError, BundleError, ConfigError, SecretError, MIN_PASSPHRASE_LEN};
use dbboard_config::{AI_CONFIG_VERSION, CONFIG_VERSION};
use dbboard_core::DbError;
use dbboard_i18n::{t, t_args};
use eframe::egui;

/// A user-facing error split into its localized message and the original
/// (English / source-provided) text.
///
/// Both halves are shown by [`render_error`] and both are copyable so the
/// English can be pasted into a search box or an AI assistant. When the
/// two halves are identical (a UI-side validation with no lower-layer
/// origin) only one line is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayError {
    localized: String,
    original: String,
}

impl DisplayError {
    /// A localized message paired with its original English text.
    pub fn new(localized: impl Into<String>, original: impl Into<String>) -> Self {
        Self {
            localized: localized.into(),
            original: original.into(),
        }
    }

    /// A message with no separate original — a UI-side validation (e.g.
    /// "passphrases do not match") that never travelled up from a lower
    /// layer. The two halves are identical, so [`render_error`] shows it
    /// once and [`Self::clipboard_text`] does not duplicate it.
    pub fn plain(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            localized: text.clone(),
            original: text,
        }
    }

    #[must_use]
    pub fn localized(&self) -> &str {
        &self.localized
    }

    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Whether there is a distinct original English line to show beneath
    /// the localized message.
    #[must_use]
    fn has_distinct_original(&self) -> bool {
        self.original != self.localized
    }

    /// Text placed on the clipboard by the Copy button: the localized line
    /// and, when different, the original English on a second line.
    #[must_use]
    pub fn clipboard_text(&self) -> String {
        if self.has_distinct_original() {
            format!("{}\n{}", self.localized, self.original)
        } else {
            self.localized.clone()
        }
    }
}

/// Translate a [`SecretError`] into a localized string. Shared by the
/// connection and AI-provider stores, which both wrap it.
fn secret_error_localized(err: &SecretError) -> String {
    match err {
        SecretError::NotFound(reference) => {
            t_args!("secret-error-not-found", reference = reference.clone())
        }
        SecretError::Backend { key_ref, source } => t_args!(
            "secret-error-backend",
            reference = key_ref.clone(),
            detail = source.to_string()
        ),
    }
}

/// Translate a [`BundleError`] into a localized string (ADR-0038 bundle
/// export / import).
fn bundle_error_localized(err: &BundleError) -> String {
    match err {
        BundleError::WeakPassphrase => {
            t_args!(
                "config-error-bundle-passphrase-short",
                min = MIN_PASSPHRASE_LEN
            )
        }
        BundleError::Serialize(e) => {
            t_args!("config-error-bundle-serialize", detail = e.to_string())
        }
        BundleError::IncorrectPassphrase => t!("config-error-bundle-incorrect-passphrase"),
        BundleError::Corrupt => t!("config-error-bundle-corrupt"),
        BundleError::UnsupportedVersion(found) => {
            t_args!("config-error-bundle-unsupported-version", found = *found)
        }
        BundleError::Parse(e) => {
            t_args!(
                "config-error-bundle-invalid-payload",
                detail = e.to_string()
            )
        }
        BundleError::Io(e) => t_args!("config-error-bundle-io", detail = e.to_string()),
    }
}

/// Translate a [`ConfigError`] (connection store). The original English
/// half is the error's own `Display`.
#[must_use]
pub fn config_error_display(err: &ConfigError) -> DisplayError {
    let localized = match err {
        ConfigError::Parse(e) => t_args!("config-error-parse", detail = e.to_string()),
        ConfigError::UnsupportedVersion(found) => t_args!(
            "config-error-unsupported-version",
            found = *found,
            expected = CONFIG_VERSION
        ),
        ConfigError::DuplicateId(id) => t_args!("config-error-duplicate-id", id = id.clone()),
        ConfigError::Io(e) => t_args!("config-error-io", detail = e.to_string()),
        ConfigError::Serialize(e) => t_args!("config-error-serialize", detail = e.to_string()),
        ConfigError::NoConfigDir => t!("config-error-no-config-dir"),
        ConfigError::Secret(e) => secret_error_localized(e),
        ConfigError::NotFound(id) => t_args!("config-error-not-found", id = id.clone()),
        ConfigError::KindMismatch { id } => {
            t_args!("config-error-kind-mismatch", id = id.clone())
        }
        ConfigError::Bundle(e) => bundle_error_localized(e),
    };
    DisplayError::new(localized, err.to_string())
}

/// Translate an [`AiSettingsError`] (AI-provider store). The original
/// English half is the error's own `Display`.
#[must_use]
pub fn ai_settings_error_display(err: &AiSettingsError) -> DisplayError {
    let localized = match err {
        AiSettingsError::Parse(e) => t_args!("ai-settings-error-parse", detail = e.to_string()),
        AiSettingsError::UnsupportedVersion(found) => t_args!(
            "ai-settings-error-unsupported-version",
            found = *found,
            expected = AI_CONFIG_VERSION
        ),
        AiSettingsError::DuplicateId(id) => {
            t_args!("ai-settings-error-duplicate-id", id = id.clone())
        }
        AiSettingsError::UnknownActiveId(id) => {
            t_args!("ai-settings-error-unknown-active-id", id = id.clone())
        }
        AiSettingsError::Io(e) => t_args!("ai-settings-error-io", detail = e.to_string()),
        AiSettingsError::Serialize(e) => {
            t_args!("ai-settings-error-serialize", detail = e.to_string())
        }
        AiSettingsError::NoConfigDir => t!("ai-settings-error-no-config-dir"),
        AiSettingsError::Secret(e) => secret_error_localized(e),
        AiSettingsError::NotFound(id) => t_args!("ai-settings-error-not-found", id = id.clone()),
        AiSettingsError::KindMismatch { id } => {
            t_args!("ai-settings-error-kind-mismatch", id = id.clone())
        }
    };
    DisplayError::new(localized, err.to_string())
}

/// Translate a [`DbError`] into a [`DisplayError`]. Only the category
/// prefix is localized; the body is the server-returned string kept
/// verbatim (ADR-0009 / ADR-0015). The original English half is the
/// error's own `Display`.
#[must_use]
pub fn db_error_display(err: &DbError) -> DisplayError {
    let prefix = match err.category() {
        "connection" => t!("error-prefix-connection"),
        "schema" => t!("error-prefix-schema"),
        "type_conversion" => t!("error-prefix-type-conversion"),
        "capability" => t!("error-prefix-capability"),
        // Includes "query" and any future category that reached the UI
        // before it was taught the new name — degrade visibly rather than
        // dropping the prefix.
        _ => t!("error-prefix-query"),
    };
    DisplayError::new(format!("{prefix}: {}", err.message()), err.to_string())
}

/// Translate an [`AiError`] into a [`DisplayError`]. AI errors have their
/// own taxonomy (they never cross the desktop ↔ web HTTP contract, ADR-0023
/// Decision 8); only the prefix is localized, the provider-returned body
/// stays verbatim. The original English half is the error's own `Display`.
#[must_use]
pub fn ai_error_display(err: &AiError) -> DisplayError {
    let localized = match err {
        AiError::Configuration(msg) => format!("{}: {msg}", t!("ai-error-prefix-configuration")),
        AiError::Network(msg) => format!("{}: {msg}", t!("ai-error-prefix-network")),
        AiError::Provider(msg) => format!("{}: {msg}", t!("ai-error-prefix-provider")),
        AiError::Quota(msg) => format!("{}: {msg}", t!("ai-error-prefix-quota")),
        AiError::Cancelled => t!("ai-error-prefix-cancelled"),
    };
    DisplayError::new(localized, err.to_string())
}

/// Render an error inline: the localized message in red, the original
/// English beneath it (dimmed, only when it differs), and a Copy button.
/// Both text lines are selectable so Ctrl+C works too (ADR-0039).
pub fn render_error(ui: &mut egui::Ui, err: Option<&DisplayError>) {
    let Some(err) = err else {
        return;
    };
    ui.vertical(|ui| {
        // The Copy button sits on its own row so a long message is free to
        // wrap to the full available width below it. Keeping the button
        // inline (as it once was) forced the message onto a single
        // horizontal line, and a lengthy provider body — e.g. an OpenAI
        // 429 `insufficient_quota` error — overflowed the AI panel to the
        // right instead of wrapping.
        if ui
            .button(t!("error-copy-button"))
            .on_hover_text(t!("error-copy-hint"))
            .clicked()
        {
            ui.ctx().copy_text(err.clipboard_text());
        }
        ui.add(
            egui::Label::new(egui::RichText::new(err.localized()).color(egui::Color32::LIGHT_RED))
                .selectable(true)
                .wrap(),
        );
        if err.has_distinct_original() {
            ui.add(
                egui::Label::new(egui::RichText::new(err.original()).weak().small())
                    .selectable(true)
                    .wrap(),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io_err() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied")
    }

    #[test]
    fn plain_error_has_no_distinct_original_and_no_duplicate_clipboard() {
        let e = DisplayError::plain("パスフレーズが違います。");
        assert!(!e.has_distinct_original());
        // A plain error must not repeat itself on the clipboard.
        assert_eq!(e.clipboard_text(), "パスフレーズが違います。");
    }

    #[test]
    fn new_error_joins_both_halves_on_the_clipboard() {
        let e = DisplayError::new("訳", "original english");
        assert!(e.has_distinct_original());
        assert_eq!(e.clipboard_text(), "訳\noriginal english");
    }

    #[test]
    fn config_error_keeps_the_english_display_as_the_original() {
        // The original half must be the error's own Display verbatim so it
        // stays searchable / pasteable regardless of locale.
        let err = ConfigError::DuplicateId("store-a".to_string());
        let shown = config_error_display(&err);
        assert_eq!(shown.original(), err.to_string());
        assert!(shown.original().contains("store-a"));
    }

    #[test]
    fn config_error_localized_half_resolves_a_real_key() {
        // A missing Fluent key echoes the key verbatim; assert we did not
        // ship a dangling reference for a representative set of variants.
        let cases = [
            ConfigError::NoConfigDir,
            ConfigError::DuplicateId("x".into()),
            ConfigError::Io(io_err()),
            ConfigError::NotFound("y".into()),
            ConfigError::KindMismatch { id: "z".into() },
            ConfigError::Secret(SecretError::NotFound("dbboard.a.token".into())),
            ConfigError::Bundle(BundleError::IncorrectPassphrase),
            ConfigError::Bundle(BundleError::WeakPassphrase),
        ];
        for err in cases {
            let shown = config_error_display(&err);
            assert!(!shown.localized().is_empty());
            assert!(
                !shown.localized().starts_with("config-error-")
                    && !shown.localized().starts_with("secret-error-"),
                "localized half echoed the raw key for {err:?}: {}",
                shown.localized()
            );
        }
    }

    #[test]
    fn ai_settings_error_localized_half_resolves_a_real_key() {
        let cases = [
            AiSettingsError::NoConfigDir,
            AiSettingsError::DuplicateId("p".into()),
            AiSettingsError::UnknownActiveId("q".into()),
            AiSettingsError::NotFound("r".into()),
            AiSettingsError::KindMismatch { id: "s".into() },
            AiSettingsError::Secret(SecretError::NotFound("dbboard.ai.key".into())),
        ];
        for err in cases {
            let shown = ai_settings_error_display(&err);
            assert_eq!(shown.original(), err.to_string());
            assert!(
                !shown.localized().starts_with("ai-settings-error-")
                    && !shown.localized().starts_with("secret-error-"),
                "localized half echoed the raw key for {err:?}: {}",
                shown.localized()
            );
        }
    }

    #[test]
    fn db_error_prefixes_a_translated_category_and_preserves_english_original() {
        let err = DbError::Connection("host unreachable".to_string());
        let shown = db_error_display(&err);
        // Body preserved in both halves; original is the full English Display.
        assert!(shown.localized().contains("host unreachable"));
        assert_eq!(shown.original(), err.to_string());
        assert!(!shown.localized().starts_with("error-prefix-"));
    }
}
