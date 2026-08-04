//! On-disk shape of the AI provider store (ADR-0025).
//!
//! Sibling to [`crate::store`]: same `ProjectDirs` config dir, same
//! `secure_fs` at-rest posture (Unix `0o600` / Windows inherited DACL),
//! same parse-and-validate / `load_or_empty` / `save_atomic` shape.
//!
//! Secrets are *referenced* here (`keyring_api_key_ref`) but never
//! *stored* here. The actual API key round-trips through the same
//! [`crate::secrets::SecretStore`] the connection store uses, under
//! the `dbboard.ai.<id>.api_key` namespace so it cannot collide with
//! connection-id keyring entries even when the two ids happen to
//! match.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secrets::SecretError;
use crate::secure_fs;

/// The single TOML schema version this build understands.
///
/// Future schema evolutions will bump this constant and add an
/// explicit in-place migration; until then an unknown version is a
/// hard error so a forward- or backward-incompatible file cannot be
/// silently round-tripped.
pub const AI_CONFIG_VERSION: u32 = 1;

/// Top-level shape of `ai-providers.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiProviderFile {
    pub version: u32,
    /// The id of the entry that should be loaded as the active provider
    /// at startup, when no env-var override is in play. `None` means
    /// "no AI provider is active" — the panel degrades to hidden, same
    /// as the env-var-absent path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<AiProviderEntry>,
}

/// A single `[[providers]]` entry. `id` is the stable primary key the
/// switcher uses; `name` is the human label shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiProviderEntry {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub kind: AiProviderKind,
}

/// Provider-specific fields. `serde(tag = "kind")` puts the
/// discriminator inline with the entry so the TOML stays flat — same
/// shape as [`crate::store::ConnectionKind`].
///
/// ```toml
/// [[providers]]
/// id   = "anthropic-main"
/// name = "Claude Sonnet"
/// kind = "anthropic"
/// model = "claude-sonnet-5"
/// keyring_api_key_ref = "dbboard.ai.anthropic-main.api_key"
/// ```
///
/// New providers land additively as new variants. ADR-0025 shipped
/// `Anthropic`; ADR-0052 adds `OpenAi`. `ollama`, … remain deferred to
/// follow-up ADRs (ADR-0025 §Out-of-scope).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiProviderKind {
    /// Anthropic Claude provider (ADR-0023). `model` is the request-time
    /// model name; when absent the crate-side default
    /// (`claude-sonnet-5` at the time of writing) is used.
    Anthropic {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        keyring_api_key_ref: String,
    },
    /// `OpenAI` `ChatGPT` provider (ADR-0052). Same shape as `Anthropic` —
    /// an optional `model` (crate-side default `gpt-4o` when absent) and
    /// a keyring reference. Explicitly renamed to `kind = "openai"` so
    /// the TOML value matches the provider id rather than the
    /// `snake_case` default (`open_ai`).
    #[serde(rename = "openai")]
    OpenAi {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        keyring_api_key_ref: String,
    },
}

impl AiProviderFile {
    /// Parse and validate an `ai-providers.toml` payload.
    ///
    /// Validates the schema version, that ids are unique, and — when
    /// `active_id` is `Some` — that it references an existing entry.
    /// Unknown `kind` values and unknown versions are surfaced as hard
    /// errors; a dangling `active_id` would otherwise silently degrade
    /// the panel to "no provider".
    ///
    /// # Errors
    ///
    /// - [`AiSettingsError::Parse`] if the TOML is malformed or
    ///   contains an unknown `kind`.
    /// - [`AiSettingsError::UnsupportedVersion`] if `version` is not
    ///   [`AI_CONFIG_VERSION`].
    /// - [`AiSettingsError::DuplicateId`] if two entries share the same
    ///   `id`.
    /// - [`AiSettingsError::UnknownActiveId`] if `active_id` references
    ///   an id no entry has.
    pub fn parse(input: &str) -> Result<Self, AiSettingsError> {
        let file: AiProviderFile = toml::from_str(input)?;
        if file.version != AI_CONFIG_VERSION {
            return Err(AiSettingsError::UnsupportedVersion(file.version));
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(file.providers.len());
        for entry in &file.providers {
            if !seen.insert(entry.id.as_str()) {
                return Err(AiSettingsError::DuplicateId(entry.id.clone()));
            }
        }
        if let Some(active) = file.active_id.as_deref() {
            if !seen.contains(active) {
                return Err(AiSettingsError::UnknownActiveId(active.to_string()));
            }
        }
        Ok(file)
    }

    /// Convenience constructor for an empty store at the current schema
    /// version. Used by [`load_or_empty`] and by tests.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            version: AI_CONFIG_VERSION,
            active_id: None,
            providers: Vec::new(),
        }
    }
}

/// The default per-user path for `ai-providers.toml`, resolved via the
/// same `directories` lookup as [`crate::store::default_path`] so it
/// lives next to `connections.toml` and `history.jsonl`:
///
/// - Windows: `%APPDATA%\dbboard\dbboard\config\ai-providers.toml`
/// - macOS:   `~/Library/Application Support/dev.dbboard.dbboard/ai-providers.toml`
/// - Linux:   `$XDG_CONFIG_HOME/dbboard/ai-providers.toml`
///   (default `~/.config/dbboard/ai-providers.toml`)
///
/// # Errors
///
/// Returns [`AiSettingsError::NoConfigDir`] when the OS reports no
/// usable per-user config directory (no `$HOME`, no `%APPDATA%`).
pub fn default_ai_providers_path() -> Result<PathBuf, AiSettingsError> {
    let dirs =
        ProjectDirs::from("dev", "dbboard", "dbboard").ok_or(AiSettingsError::NoConfigDir)?;
    Ok(dirs.config_dir().join("ai-providers.toml"))
}

/// Read and parse `ai-providers.toml` at `path`.
///
/// A missing file is **not** an error: it yields an empty store at the
/// current schema version. The file is created lazily by
/// [`save_atomic`] when the user adds the first entry. Any other I/O
/// error is propagated.
///
/// # Errors
///
/// - [`AiSettingsError::Io`] for non-`NotFound` I/O failures.
/// - [`AiSettingsError::Parse`], [`AiSettingsError::UnsupportedVersion`],
///   [`AiSettingsError::DuplicateId`], or
///   [`AiSettingsError::UnknownActiveId`] from the underlying
///   [`AiProviderFile::parse`].
pub fn load_or_empty(path: &Path) -> Result<AiProviderFile, AiSettingsError> {
    match fs::read_to_string(path) {
        Ok(contents) => AiProviderFile::parse(&contents),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(AiProviderFile::empty()),
        Err(err) => Err(AiSettingsError::Io(err)),
    }
}

/// Write `file` to `path` atomically: serialize to a sibling `*.tmp`
/// file (created via [`secure_fs::create_new_user_only`] so it lands as
/// `0o600` on Unix and inherits the user-only DACL on Windows
/// `%APPDATA%\Roaming\`) and then `rename` it into place. Parent
/// directories are created if necessary.
///
/// # Errors
///
/// - [`AiSettingsError::Serialize`] if re-serializing the in-memory
///   store to TOML fails.
/// - [`AiSettingsError::Io`] for any filesystem failure (creating
///   parent dirs, opening the temp file, writing, syncing, renaming).
pub fn save_atomic(path: &Path, file: &AiProviderFile) -> Result<(), AiSettingsError> {
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
        return Err(AiSettingsError::Io(err));
    }
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".ai-providers.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

// `create_new_user_only` rejects a stale temp left behind by an
// interrupted save — better to fail loudly than to clobber. Same
// posture as the connection store (ADR-0024).
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = secure_fs::create_new_user_only(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

/// Errors that can occur while loading, validating, or saving the AI
/// provider store, or while routing secrets through a
/// [`crate::SecretStore`].
///
/// Independent of `dbboard_core::DbError` and `dbboard_ai::AiError`:
/// AI settings errors live below the HTTP surface (raised at process
/// startup or in-process during a Settings UI mutation), so they
/// never reach the `{category, message}` envelope defined in
/// `docs/api-contract.md`.
#[derive(Debug, Error)]
pub enum AiSettingsError {
    /// The TOML payload could not be parsed at all.
    #[error("ai settings parse failed: {0}")]
    Parse(#[from] toml::de::Error),

    /// `version` does not equal the single supported value
    /// ([`AI_CONFIG_VERSION`]). We refuse to guess at a forward- or
    /// backward-incompatible shape.
    #[error("unsupported ai settings version: {0} (only version {expected} is supported)", expected = AI_CONFIG_VERSION)]
    UnsupportedVersion(u32),

    /// Two `[[providers]]` entries share the same `id`.
    #[error("duplicate ai provider id: {0}")]
    DuplicateId(String),

    /// `active_id` references an id no `[[providers]]` entry has.
    /// Surfaced loudly so the user sees the problem in the Settings UI
    /// instead of the panel silently degrading to "no provider".
    #[error("active_id refers to an unknown provider id: {0}")]
    UnknownActiveId(String),

    /// Filesystem read or write failed. Path is not embedded so the
    /// message is safe to surface in logs; callers attach the path
    /// when they have it.
    #[error("ai settings io failed: {0}")]
    Io(#[from] std::io::Error),

    /// Re-serializing the in-memory store back to TOML failed.
    #[error("ai settings serialize failed: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// The OS reported no usable per-user config directory.
    #[error("could not resolve a per-user config directory")]
    NoConfigDir,

    /// The keyring / in-memory secret store reported a failure while
    /// resolving the API key for an active provider.
    #[error("ai settings secret failed: {0}")]
    Secret(#[from] SecretError),

    /// `AiSettingsAdmin::{update, delete, set_active}` was called with
    /// an id that no entry in the store matches. Surfaced loudly
    /// because the caller is almost certainly using a stale view of
    /// the entries vector (same posture as
    /// [`crate::ConfigError::NotFound`]).
    #[error("no ai provider entry with id: {0}")]
    NotFound(String),

    /// `AiSettingsAdmin::update` was called with a draft whose
    /// `AiProviderKind` variant differs from the existing entry's
    /// kind. Kind changes are intentionally not supported on edit
    /// (same posture as [`crate::ConfigError::KindMismatch`]): they
    /// would require migrating the keyring reference mid-flight,
    /// which collapses the rollback story.
    #[error("ai provider {id} kind cannot change on update")]
    KindMismatch { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_constructor_uses_the_current_schema_version() {
        let file = AiProviderFile::empty();
        assert_eq!(file.version, AI_CONFIG_VERSION);
        assert!(file.providers.is_empty());
        assert!(file.active_id.is_none());
    }

    #[test]
    fn version_only_file_parses_with_no_providers() {
        let toml_src = "version = 1\n";
        let file = AiProviderFile::parse(toml_src).expect("version-only file parses");
        assert_eq!(file.version, 1);
        assert!(file.providers.is_empty());
        assert!(file.active_id.is_none());
    }

    #[test]
    fn parses_a_minimal_anthropic_entry_without_model() {
        let toml_src = r#"
version = 1

[[providers]]
id                  = "anthropic-main"
name                = "Claude Sonnet"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.anthropic-main.api_key"
"#;
        let file = AiProviderFile::parse(toml_src).expect("anthropic entry parses");
        assert_eq!(file.providers.len(), 1);
        let entry = &file.providers[0];
        assert_eq!(entry.id, "anthropic-main");
        assert_eq!(entry.name, "Claude Sonnet");
        assert_eq!(
            entry.kind,
            AiProviderKind::Anthropic {
                model: None,
                keyring_api_key_ref: "dbboard.ai.anthropic-main.api_key".to_string(),
            }
        );
        assert!(file.active_id.is_none());
    }

    #[test]
    fn parses_an_anthropic_entry_with_model_override() {
        let toml_src = r#"
version = 1
active_id = "anthropic-main"

[[providers]]
id                  = "anthropic-main"
name                = "Claude Sonnet (override)"
kind                = "anthropic"
model               = "claude-opus-4-8"
keyring_api_key_ref = "dbboard.ai.anthropic-main.api_key"
"#;
        let file = AiProviderFile::parse(toml_src).expect("anthropic with model parses");
        assert_eq!(file.active_id.as_deref(), Some("anthropic-main"));
        assert_eq!(
            file.providers[0].kind,
            AiProviderKind::Anthropic {
                model: Some("claude-opus-4-8".to_string()),
                keyring_api_key_ref: "dbboard.ai.anthropic-main.api_key".to_string(),
            }
        );
    }

    #[test]
    fn parses_an_openai_entry_with_model_override() {
        // ADR-0052: the second provider variant. `kind = "openai"`
        // (explicit rename, not the `open_ai` snake_case default) and
        // the same optional-model + keyring-ref shape as Anthropic.
        let toml_src = r#"
version = 1
active_id = "openai-main"

[[providers]]
id                  = "openai-main"
name                = "ChatGPT"
kind                = "openai"
model               = "gpt-4o-mini"
keyring_api_key_ref = "dbboard.ai.openai-main.api_key"
"#;
        let file = AiProviderFile::parse(toml_src).expect("openai with model parses");
        assert_eq!(file.active_id.as_deref(), Some("openai-main"));
        assert_eq!(
            file.providers[0].kind,
            AiProviderKind::OpenAi {
                model: Some("gpt-4o-mini".to_string()),
                keyring_api_key_ref: "dbboard.ai.openai-main.api_key".to_string(),
            }
        );
    }

    #[test]
    fn openai_entry_round_trips_through_serialization() {
        // The `kind = "openai"` value must survive a save/parse cycle —
        // a regression here would silently rewrite it to `open_ai` and
        // break the next load.
        let file = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: Some("openai-main".to_string()),
            providers: vec![AiProviderEntry {
                id: "openai-main".to_string(),
                name: "ChatGPT".to_string(),
                kind: AiProviderKind::OpenAi {
                    model: None,
                    keyring_api_key_ref: "dbboard.ai.openai-main.api_key".to_string(),
                },
            }],
        };
        let serialized = toml::to_string(&file).expect("serialize");
        assert!(
            serialized.contains("kind = \"openai\""),
            "kind must serialize as openai, not open_ai: {serialized}"
        );
        let reparsed = AiProviderFile::parse(&serialized).expect("reparse");
        assert_eq!(reparsed, file);
    }

    #[test]
    fn parses_multiple_providers_and_active_pointer() {
        let toml_src = r#"
version = 1
active_id = "secondary"

[[providers]]
id                  = "primary"
name                = "Primary"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.primary.api_key"

[[providers]]
id                  = "secondary"
name                = "Secondary"
kind                = "anthropic"
model               = "claude-haiku-4-5-20251001"
keyring_api_key_ref = "dbboard.ai.secondary.api_key"
"#;
        let file = AiProviderFile::parse(toml_src).expect("multi-entry parses");
        assert_eq!(file.providers.len(), 2);
        assert_eq!(file.active_id.as_deref(), Some("secondary"));
    }

    #[test]
    fn unknown_kind_is_a_parse_error() {
        let toml_src = r#"
version = 1

[[providers]]
id                  = "x"
name                = "X"
kind                = "gemini"
keyring_api_key_ref = "dbboard.ai.x.api_key"
"#;
        let err = AiProviderFile::parse(toml_src).expect_err("unknown kind must fail");
        assert!(matches!(err, AiSettingsError::Parse(_)));
    }

    #[test]
    fn duplicate_id_is_rejected_loudly() {
        let toml_src = r#"
version = 1

[[providers]]
id                  = "dup"
name                = "First"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.dup.api_key"

[[providers]]
id                  = "dup"
name                = "Second"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.dup.api_key"
"#;
        let err = AiProviderFile::parse(toml_src).expect_err("duplicate id must fail");
        match err {
            AiSettingsError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let toml_src = r#"
version = 2

[[providers]]
id                  = "x"
name                = "X"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.x.api_key"
"#;
        let err = AiProviderFile::parse(toml_src).expect_err("v2 must be rejected");
        match err {
            AiSettingsError::UnsupportedVersion(v) => assert_eq!(v, 2),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn missing_version_field_is_a_parse_error() {
        let toml_src = r#"
[[providers]]
id                  = "x"
name                = "X"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.x.api_key"
"#;
        let err = AiProviderFile::parse(toml_src).expect_err("missing version must fail");
        assert!(matches!(err, AiSettingsError::Parse(_)));
    }

    #[test]
    fn dangling_active_id_is_rejected() {
        let toml_src = r#"
version = 1
active_id = "nope"

[[providers]]
id                  = "real"
name                = "Real"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.real.api_key"
"#;
        let err = AiProviderFile::parse(toml_src).expect_err("dangling active_id must fail");
        match err {
            AiSettingsError::UnknownActiveId(id) => assert_eq!(id, "nope"),
            other => panic!("expected UnknownActiveId, got {other:?}"),
        }
    }

    #[test]
    fn active_id_pointing_at_a_valid_entry_parses() {
        let toml_src = r#"
version = 1
active_id = "real"

[[providers]]
id                  = "real"
name                = "Real"
kind                = "anthropic"
keyring_api_key_ref = "dbboard.ai.real.api_key"
"#;
        let file = AiProviderFile::parse(toml_src).expect("valid active_id parses");
        assert_eq!(file.active_id.as_deref(), Some("real"));
    }

    #[test]
    fn serialize_then_parse_is_identity_with_active_id_present() {
        let original = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: Some("primary".to_string()),
            providers: vec![
                AiProviderEntry {
                    id: "primary".to_string(),
                    name: "Primary".to_string(),
                    kind: AiProviderKind::Anthropic {
                        model: None,
                        keyring_api_key_ref: "dbboard.ai.primary.api_key".to_string(),
                    },
                },
                AiProviderEntry {
                    id: "secondary".to_string(),
                    name: "Secondary".to_string(),
                    kind: AiProviderKind::Anthropic {
                        model: Some("claude-haiku-4-5-20251001".to_string()),
                        keyring_api_key_ref: "dbboard.ai.secondary.api_key".to_string(),
                    },
                },
            ],
        };
        let serialized = toml::to_string(&original).expect("serialize");
        let reparsed = AiProviderFile::parse(&serialized).expect("re-parse");
        assert_eq!(original, reparsed);
    }

    #[test]
    fn serialize_then_parse_is_identity_with_no_active_id() {
        let original = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: None,
            providers: vec![AiProviderEntry {
                id: "only".to_string(),
                name: "Only".to_string(),
                kind: AiProviderKind::Anthropic {
                    model: None,
                    keyring_api_key_ref: "dbboard.ai.only.api_key".to_string(),
                },
            }],
        };
        let serialized = toml::to_string(&original).expect("serialize");
        let reparsed = AiProviderFile::parse(&serialized).expect("re-parse");
        assert_eq!(original, reparsed);
    }

    #[test]
    fn absent_active_id_is_not_emitted_during_serialization() {
        let file = AiProviderFile::empty();
        let serialized = toml::to_string(&file).expect("serialize");
        assert!(
            !serialized.contains("active_id"),
            "absent active_id must not be emitted: {serialized}"
        );
    }

    #[test]
    fn absent_model_is_not_emitted_during_serialization() {
        let file = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: None,
            providers: vec![AiProviderEntry {
                id: "p".to_string(),
                name: "P".to_string(),
                kind: AiProviderKind::Anthropic {
                    model: None,
                    keyring_api_key_ref: "dbboard.ai.p.api_key".to_string(),
                },
            }],
        };
        let serialized = toml::to_string(&file).expect("serialize");
        assert!(
            !serialized.contains("model"),
            "absent model must not be emitted: {serialized}"
        );
    }

    #[test]
    fn serialized_toml_has_no_secret_value_keys() {
        // Even if the caller wedged something secret-shaped into the
        // non-secret fields, the schema never exposes a key called
        // `api_key`, `token`, `password`, or `secret` directly — only
        // `keyring_api_key_ref`, which is a reference, not material.
        let file = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: Some("p".to_string()),
            providers: vec![AiProviderEntry {
                id: "p".to_string(),
                name: "P".to_string(),
                kind: AiProviderKind::Anthropic {
                    model: Some("claude-sonnet-4-6".to_string()),
                    keyring_api_key_ref: "dbboard.ai.p.api_key".to_string(),
                },
            }],
        };
        let serialized = toml::to_string(&file).expect("serialize");
        for forbidden in ["api_key =", "token =", "password =", "secret ="] {
            assert!(
                !serialized.contains(forbidden),
                "serialized TOML must not expose a `{forbidden}` field: {serialized}"
            );
        }
        assert!(serialized.contains("keyring_api_key_ref ="));
    }

    #[test]
    fn load_or_empty_on_missing_file_returns_empty_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ai-providers.toml");
        let file = load_or_empty(&path).expect("missing -> empty");
        assert_eq!(file, AiProviderFile::empty());
    }

    #[test]
    fn save_atomic_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ai-providers.toml");
        let original = AiProviderFile {
            version: AI_CONFIG_VERSION,
            active_id: Some("p".to_string()),
            providers: vec![AiProviderEntry {
                id: "p".to_string(),
                name: "P".to_string(),
                kind: AiProviderKind::Anthropic {
                    model: Some("claude-sonnet-4-6".to_string()),
                    keyring_api_key_ref: "dbboard.ai.p.api_key".to_string(),
                },
            }],
        };
        save_atomic(&path, &original).expect("save");
        let reloaded = load_or_empty(&path).expect("load");
        assert_eq!(original, reloaded);
    }

    #[test]
    fn save_atomic_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir
            .path()
            .join("nested")
            .join("more")
            .join("ai-providers.toml");
        save_atomic(&path, &AiProviderFile::empty()).expect("save creates parents");
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn save_atomic_lands_with_user_only_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ai-providers.toml");
        save_atomic(&path, &AiProviderFile::empty()).expect("save");
        let mode = fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "ai-providers.toml must land as 0o600 on Unix");
    }

    #[test]
    fn default_ai_providers_path_ends_with_the_expected_filename() {
        // We do not assert on the parent path because `directories`
        // honours `$XDG_CONFIG_HOME` and platform-specific env vars; we
        // only pin the filename to keep the schema stable.
        let path = default_ai_providers_path().expect("project dirs resolve");
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("ai-providers.toml")
        );
    }
}
