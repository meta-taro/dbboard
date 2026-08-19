//! Local user configuration for dbboard (ADR-0013, ADR-0025).
//!
//! Owns the persistence halves that must never blur into one another:
//!
//! - [`store`] — the on-disk shape of `connections.toml`, plus the
//!   filesystem layer (`load_or_empty`, `save_atomic`) and the
//!   `default_history_path()` helper the client uses to find
//!   `history.jsonl` (ADR-0017) under the same config dir.
//! - [`ai_store`] — the on-disk shape of `ai-providers.toml`, the
//!   companion file added in ADR-0025 Phase 4 Stage 2 Group A. Same
//!   filesystem posture (`secure_fs`-backed atomic writes, Unix
//!   `0o600` / Windows inherited DACL) as `store`.
//! - [`annotations`] — the on-disk shape + admin API for local
//!   table/column notes (`annotations.toml`, ADR-0045). Same
//!   `secure_fs` posture as the stores above, but carries *no* secret
//!   and never writes to any database — the notes are documentation the
//!   engines (SQLite/D1 especially) can't hold themselves.
//! - [`secrets`] — `SecretStore` trait with an OS-keychain backend
//!   ([`secrets::KeyringStore`]) and an in-memory fallback
//!   ([`secrets::InMemorySecretStore`]) for tests and CI. Used by
//!   both `admin` (connection secrets at `dbboard.<id>.<field>`) and
//!   `ai_settings` (AI api keys at `dbboard.ai.<id>.api_key`); the
//!   `ai.` infix keeps the two namespaces collision-free.
//! - [`error`] — crate-local error type for the connection halves.
//!   [`ai_store::AiSettingsError`] is the sibling for the AI halves;
//!   both happen at process startup or in-process during a settings
//!   mutation, so neither reaches the HTTP envelope.
//!
//! The crate exists because `dbboard-core` is "no I/O" (ADR-0002 /
//! ADR-0009) and the client binary is wiring only — neither is the right
//! home for filesystem + OS-keychain persistence.

pub mod admin;
pub mod ai_settings;
pub mod ai_store;
pub mod annotations;
pub mod bundle;
pub mod dsn;
pub mod error;
pub mod secrets;
pub mod secure_fs;
pub mod store;
pub mod ui_command;
pub mod ui_settings;

pub use admin::{
    ConnectionAdmin, ConnectionDraft, ConnectionEditDraft, ConnectionKindDraft,
    ConnectionKindEditDraft, FirestoreCredentialField, ImportMode, ImportReport, RefusedEntry,
    SecretField, SshAuthDraft, SshAuthEditDraft, SshEditField, SshHostKeyDraft, SshPassphraseField,
    SshTunnelDraft, SshTunnelEditDraft,
};
pub use ai_settings::{
    AiProviderDraft, AiProviderEditDraft, AiProviderKindDraft, AiProviderKindEditDraft,
    AiSettingsAdmin,
};
pub use ai_store::{
    default_ai_providers_path, AiProviderEntry, AiProviderFile, AiProviderKind, AiSettingsError,
    AI_CONFIG_VERSION,
};
pub use annotations::{
    default_annotations_path, table_key, AnnotationsAdmin, AnnotationsError, AnnotationsFile,
    ColumnAnnotation, ConnectionAnnotations, TableAnnotations, ANNOTATIONS_VERSION,
};
pub use bundle::{
    decrypt_bundle, encrypt_bundle, validate_passphrase, BundleError, BundlePayload,
    BUNDLE_VERSION, MIN_PASSPHRASE_LEN,
};
pub use dsn::{parse_dsn, with_password, DsnParts};
pub use error::ConfigError;
pub use secrets::{InMemorySecretStore, KeyringStore, SecretError, SecretStore, KEYRING_SERVICE};
pub use store::{
    default_history_path, default_path, ConnectionEntry, ConnectionFile, ConnectionKind,
    SshTunnelToml, CONFIG_VERSION,
};
pub use ui_command::{
    default_ui_command_path, default_ui_result_path, load_command_or_default,
    load_result_or_default, next_seq, pending_command, save_command_atomic, save_result_atomic,
    UiCommand, UiCommandFile, UiResultFile, UI_COMMAND_VERSION,
};
pub use ui_settings::{
    default_ui_settings_path, is_supported_locale, load_or_default as load_ui_settings,
    save_atomic as save_ui_settings, ThemePreference, UiSettingsFile, SUPPORTED_LOCALES,
    UI_SETTINGS_VERSION,
};
