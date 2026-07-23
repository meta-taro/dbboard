//! Persisted UI preferences (ADR-0041): currently just the colour theme.
//!
//! Sibling to [`crate::store`] and [`crate::ai_store`]: same `ProjectDirs`
//! config dir, same atomic sibling-`*.tmp`-then-rename write via
//! [`crate::secure_fs`]. Unlike those, a corrupt or version-incompatible
//! file here is **non-fatal** — UI chrome must never block startup — so
//! [`load_or_default`] falls back to defaults (logged) instead of erroring,
//! and the next [`save_atomic`] rewrites the file cleanly.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::secure_fs;

/// The single TOML schema version this build understands. A future
/// evolution bumps this and adds an explicit migration; until then an
/// unknown version is treated as "unreadable" and replaced with defaults.
pub const UI_SETTINGS_VERSION: u32 = 1;

/// The user's colour-theme choice. `Auto` follows the OS light/dark
/// setting at runtime (mapped to `egui::ThemePreference::System` by the
/// binary); `Light` / `Dark` pin it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    /// Dark mode.
    Dark,
    /// Light mode.
    Light,
    /// Follow the operating system's light/dark preference.
    #[default]
    Auto,
}

/// Top-level shape of `ui-settings.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSettingsFile {
    pub version: u32,
    /// A missing `theme` key defaults to [`ThemePreference::Auto`], so an
    /// older file that predates a future added field still loads.
    #[serde(default)]
    pub theme: ThemePreference,
    /// The backup large-database warn threshold in total rows (ADR-0050).
    /// `None` means "not configured" — the app falls back to its built-in
    /// default (`dbboard_core::DEFAULT_BACKUP_WARN_ROWS`) rather than this
    /// crate duplicating a domain constant it has no dependency on. A file
    /// written before ADR-0050 has no key, so `#[serde(default)]` reads it
    /// back as `None` and the fallback applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_warn_rows: Option<u64>,
}

impl Default for UiSettingsFile {
    fn default() -> Self {
        Self {
            version: UI_SETTINGS_VERSION,
            theme: ThemePreference::default(),
            backup_warn_rows: None,
        }
    }
}

impl UiSettingsFile {
    /// A settings file pinning `theme` at the current schema version, with
    /// the backup threshold left unset. Convenience for call sites that only
    /// touch the theme; to change one field while preserving the others,
    /// load-modify-save the whole struct instead (see
    /// [`crate::load_ui_settings`]).
    #[must_use]
    pub fn with_theme(theme: ThemePreference) -> Self {
        Self {
            version: UI_SETTINGS_VERSION,
            theme,
            backup_warn_rows: None,
        }
    }

    /// Parse from TOML, enforcing the schema version.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Parse`] on malformed TOML.
    /// - [`ConfigError::UnsupportedVersion`] when `version` is not
    ///   [`UI_SETTINGS_VERSION`].
    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let file: UiSettingsFile = toml::from_str(contents)?;
        if file.version != UI_SETTINGS_VERSION {
            return Err(ConfigError::UnsupportedVersion(file.version));
        }
        Ok(file)
    }
}

/// Default per-user path for `ui-settings.toml`, alongside the other
/// config files (`connections.toml`, `ai-providers.toml`, `history.jsonl`).
///
/// # Errors
///
/// Returns [`ConfigError::NoConfigDir`] when the OS reports no usable
/// per-user config directory (no `$HOME`, no `%APPDATA%`).
pub fn default_ui_settings_path() -> Result<PathBuf, ConfigError> {
    let dirs = ProjectDirs::from("dev", "dbboard", "dbboard").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().join("ui-settings.toml"))
}

/// Load `ui-settings.toml`, falling back to defaults on **any** problem.
///
/// UI chrome must not be able to break startup, so unlike the connection
/// store this never returns an error: a missing file is the default, and a
/// malformed or version-incompatible file is logged and replaced with the
/// default in memory (the next [`save_atomic`] rewrites it cleanly).
#[must_use]
pub fn load_or_default(path: &Path) -> UiSettingsFile {
    match fs::read_to_string(path) {
        Ok(contents) => UiSettingsFile::parse(&contents).unwrap_or_else(|e| {
            eprintln!("dbboard: ignoring unreadable ui-settings.toml ({e}); using defaults");
            UiSettingsFile::default()
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => UiSettingsFile::default(),
        Err(err) => {
            eprintln!("dbboard: could not read ui-settings.toml ({err}); using defaults");
            UiSettingsFile::default()
        }
    }
}

/// Write `file` to `path` atomically: serialize to a sibling `*.tmp`
/// (created user-only) then `rename` it into place, creating parent dirs
/// as needed. Mirrors [`crate::store::save_atomic`].
///
/// # Errors
///
/// - [`ConfigError::Serialize`] if TOML serialization fails.
/// - [`ConfigError::Io`] for any filesystem failure.
pub fn save_atomic(path: &Path, file: &UiSettingsFile) -> Result<(), ConfigError> {
    let serialized = toml::to_string(file)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path_for(path);
    write_new_file(&tmp, serialized.as_bytes())?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::Io(err));
    }
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".ui-settings.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

// Same user-only create + fail-on-stale-temp posture as the connection
// store (ADR-0024): on Unix the temp lands `0o600`, on Windows it inherits
// the user-only DACL of `%APPDATA%`.
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = secure_fs::create_new_user_only(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_is_auto_at_current_version() {
        let file = UiSettingsFile::default();
        assert_eq!(file.version, UI_SETTINGS_VERSION);
        assert_eq!(file.theme, ThemePreference::Auto);
    }

    #[test]
    fn theme_serializes_lowercase_and_round_trips() {
        for theme in [
            ThemePreference::Light,
            ThemePreference::Dark,
            ThemePreference::Auto,
        ] {
            let file = UiSettingsFile::with_theme(theme);
            let toml = toml::to_string(&file).expect("serialize");
            let back = UiSettingsFile::parse(&toml).expect("parse");
            assert_eq!(back, file);
        }
        // Lowercase on the wire, matching the other stores' flat TOML style.
        let toml = toml::to_string(&UiSettingsFile::with_theme(ThemePreference::Light)).unwrap();
        assert!(toml.contains("theme = \"light\""), "got: {toml}");
    }

    #[test]
    fn parse_rejects_an_unknown_version() {
        let toml = "version = 999\ntheme = \"dark\"\n";
        let err = UiSettingsFile::parse(toml).expect_err("version guard");
        assert!(matches!(err, ConfigError::UnsupportedVersion(999)));
    }

    #[test]
    fn parse_defaults_theme_when_key_absent() {
        // A file with only a version still loads (forward-compatible read).
        let file = UiSettingsFile::parse("version = 1\n").expect("parse");
        assert_eq!(file.theme, ThemePreference::Auto);
    }

    #[test]
    fn backup_warn_rows_defaults_to_none_and_is_omitted_when_unset() {
        // Default carries no threshold (the app supplies its own fallback).
        let file = UiSettingsFile::default();
        assert_eq!(file.backup_warn_rows, None);
        // An unset threshold is skipped on the wire so a pre-ADR-0050 file
        // stays byte-identical after a theme-only save.
        let toml = toml::to_string(&file).expect("serialize");
        assert!(!toml.contains("backup_warn_rows"), "got: {toml}");
    }

    #[test]
    fn backup_warn_rows_absent_in_an_older_file_reads_back_as_none() {
        // A file written before ADR-0050 (theme only) still loads.
        let file = UiSettingsFile::parse("version = 1\ntheme = \"dark\"\n").expect("parse");
        assert_eq!(file.theme, ThemePreference::Dark);
        assert_eq!(file.backup_warn_rows, None);
    }

    #[test]
    fn backup_warn_rows_round_trips_when_set() {
        let file = UiSettingsFile {
            backup_warn_rows: Some(1_000_000),
            ..UiSettingsFile::with_theme(ThemePreference::Light)
        };
        let toml = toml::to_string(&file).expect("serialize");
        let back = UiSettingsFile::parse(&toml).expect("parse");
        assert_eq!(back, file);
        assert_eq!(back.backup_warn_rows, Some(1_000_000));
    }

    #[test]
    fn load_modify_save_preserves_the_sibling_field() {
        // Regression guard for the clobber footgun: changing the theme via
        // load-modify-save must not drop a persisted backup threshold, and
        // vice versa. `with_theme` deliberately does NOT preserve siblings,
        // so callers that need to must go through load-modify-save.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-settings.toml");

        // Seed a file that has a threshold set.
        save_atomic(
            &path,
            &UiSettingsFile {
                backup_warn_rows: Some(250_000),
                ..UiSettingsFile::with_theme(ThemePreference::Auto)
            },
        )
        .expect("seed");

        // Load-modify-save the theme only.
        let mut file = load_or_default(&path);
        file.theme = ThemePreference::Dark;
        save_atomic(&path, &file).expect("save theme");

        let after = load_or_default(&path);
        assert_eq!(after.theme, ThemePreference::Dark);
        assert_eq!(after.backup_warn_rows, Some(250_000), "threshold clobbered");
    }

    #[test]
    fn load_or_default_returns_default_for_a_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-settings.toml");
        assert_eq!(load_or_default(&path), UiSettingsFile::default());
    }

    #[test]
    fn load_or_default_recovers_from_a_corrupt_file() {
        // A malformed file must not break startup — it degrades to default.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-settings.toml");
        fs::write(&path, "this is not = valid = toml").unwrap();
        assert_eq!(load_or_default(&path), UiSettingsFile::default());
    }

    #[test]
    fn load_or_default_recovers_from_an_incompatible_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-settings.toml");
        fs::write(&path, "version = 999\ntheme = \"dark\"\n").unwrap();
        // Version mismatch is unreadable → default (not the file's theme).
        assert_eq!(load_or_default(&path), UiSettingsFile::default());
    }

    #[test]
    fn save_then_load_round_trips_the_theme() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-settings.toml");
        save_atomic(&path, &UiSettingsFile::with_theme(ThemePreference::Light)).expect("save");
        assert_eq!(load_or_default(&path).theme, ThemePreference::Light);

        // Overwrite in place: the atomic rename replaces the prior file.
        save_atomic(&path, &UiSettingsFile::with_theme(ThemePreference::Dark)).expect("save");
        assert_eq!(load_or_default(&path).theme, ThemePreference::Dark);
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir
            .path()
            .join("nested")
            .join("deeper")
            .join("ui-settings.toml");
        save_atomic(&path, &UiSettingsFile::with_theme(ThemePreference::Dark)).expect("save");
        assert!(path.exists());
    }
}
