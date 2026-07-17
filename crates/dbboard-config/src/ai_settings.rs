//! AI provider admin use-case (ADR-0025 Phase 4 Stage 2 Group A).
//!
//! Sibling of [`crate::admin`]. Pairs the `ai-providers.toml` file with
//! the same [`crate::SecretStore`] the connection admin uses, exposes a
//! CRUD surface (`add` / `update` / `delete` / `set_active`), and
//! enforces the same TOML-vs-keyring commit discipline so the two
//! stores cannot drift:
//!
//! - **Add:** write the API-key secret first, then save the TOML. On
//!   TOML-write failure the keyring write is rolled back so an orphan
//!   secret cannot survive a half-finished add.
//! - **Update:** for an [`SecretField::Set`] api-key, read the old
//!   value so it can be restored, write the new value, then save the
//!   TOML. On TOML-write failure restore the old value (or delete the
//!   just-written entry if there was no old value).
//! - **Delete:** save the TOML first (the file is the source of truth),
//!   then best-effort purge the keyring entry. An orphan keyring entry
//!   left by a purge failure is harmless because nothing references it
//!   any more.
//! - **`set_active`:** TOML-only mutation. The keyring is never read or
//!   written; the active pointer is just a label.
//!
//! Kind changes are intentionally rejected on update (same posture as
//! [`crate::ConnectionAdmin::update`]). When a second `AiProviderKind`
//! variant lands, switching between them must be `delete` + `add` so
//! the keyring reference is re-derived rather than migrated mid-flight.
//!
//! The AI keyring namespace is `dbboard.ai.<id>.api_key` — distinct
//! from the connection namespace `dbboard.<id>.<field>` so an AI
//! provider and a connection sharing an `id` (e.g. both called
//! `"prod"`) cannot collide.

use std::path::PathBuf;
use std::sync::Arc;

use crate::admin::SecretField;
use crate::ai_store::{
    load_or_empty, save_atomic, AiProviderEntry, AiProviderFile, AiProviderKind, AiSettingsError,
};
use crate::secrets::{SecretError, SecretStore};

/// User-supplied draft for **adding** a new AI provider entry.
///
/// Unlike [`AiProviderEntry`] the api-key material is carried inline
/// rather than as a `keyring_api_key_ref`. [`AiSettingsAdmin::add`]
/// derives the keyring ref from the id (`dbboard.ai.<id>.api_key`) and
/// routes the inline value through the configured [`SecretStore`].
#[derive(Debug, Clone)]
pub struct AiProviderDraft {
    pub id: String,
    pub name: String,
    pub kind: AiProviderKindDraft,
}

/// Add-time, inline-secret companion to [`AiProviderKind`].
#[derive(Debug, Clone)]
pub enum AiProviderKindDraft {
    Anthropic {
        model: Option<String>,
        api_key: String,
    },
}

/// User-supplied draft for **editing** an existing AI provider entry.
///
/// The id is read-only on update (it is the primary key of both the
/// TOML and the keyring entry that references it); only `name`, the
/// model override, and — when the user explicitly opts in via
/// [`SecretField::Set`] — the api-key can change.
#[derive(Debug, Clone)]
pub struct AiProviderEditDraft {
    pub name: String,
    pub kind: AiProviderKindEditDraft,
}

/// Edit-time companion to [`AiProviderKind`]. Variant must match the
/// existing entry's kind; changing kind on update is rejected with
/// [`AiSettingsError::KindMismatch`].
#[derive(Debug, Clone)]
pub enum AiProviderKindEditDraft {
    Anthropic {
        model: Option<String>,
        api_key: SecretField,
    },
}

/// Owns the on-disk `ai-providers.toml` file plus an
/// [`Arc<dyn SecretStore>`] handle and exposes the CRUD API the
/// Settings UI calls into. Construct one per process at startup via
/// [`AiSettingsAdmin::open`] and pass it down to the UI; let it route
/// all mutations through here so the TOML and the keyring stay in
/// sync.
pub struct AiSettingsAdmin {
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    file: AiProviderFile,
}

impl AiSettingsAdmin {
    /// Load `ai-providers.toml` from `path` (an empty store is returned
    /// when the file does not exist) and pair it with `secrets`.
    ///
    /// # Errors
    ///
    /// Any error from [`load_or_empty`] — schema parse failure,
    /// unsupported version, duplicate id, dangling `active_id`, or
    /// non-`NotFound` I/O.
    pub fn open(path: PathBuf, secrets: Arc<dyn SecretStore>) -> Result<Self, AiSettingsError> {
        let file = load_or_empty(&path)?;
        Ok(Self {
            path,
            secrets,
            file,
        })
    }

    /// Construct from an explicit in-memory file, without reading the
    /// disk. Intended for tests; production callers should use
    /// [`AiSettingsAdmin::open`].
    #[must_use]
    pub fn new_with_file(
        path: PathBuf,
        secrets: Arc<dyn SecretStore>,
        file: AiProviderFile,
    ) -> Self {
        Self {
            path,
            secrets,
            file,
        }
    }

    /// Borrow the current entries. The UI uses this to render the
    /// provider list and to drive the active-id radio selection.
    #[must_use]
    pub fn entries(&self) -> &[AiProviderEntry] {
        &self.file.providers
    }

    /// Borrow the currently active provider id, if any. `None` means
    /// "no AI provider is active" — equivalent to the env-var-absent
    /// path: the panel degrades to hidden.
    #[must_use]
    pub fn active_id(&self) -> Option<&str> {
        self.file.active_id.as_deref()
    }

    /// Add `draft` as a new provider.
    ///
    /// Writes the API-key secret to the [`SecretStore`] under the
    /// reference `dbboard.ai.<draft.id>.api_key`, then persists the
    /// updated TOML. If the TOML write fails, the keyring write is
    /// rolled back so an orphan secret cannot survive.
    ///
    /// # Errors
    ///
    /// - [`AiSettingsError::DuplicateId`] if `draft.id` already exists.
    /// - [`AiSettingsError::Secret`] if the keyring write fails.
    /// - [`AiSettingsError::Io`] / [`AiSettingsError::Serialize`] from
    ///   the TOML write; the keyring write has already been rolled
    ///   back when these are returned.
    ///
    /// # Panics
    ///
    /// Never in practice: the just-pushed entry is borrowed back from
    /// the in-memory file via `last()`. A panic here would imply a bug
    /// in `Vec::push` itself.
    pub fn add(&mut self, draft: AiProviderDraft) -> Result<&AiProviderEntry, AiSettingsError> {
        if self.find_index(&draft.id).is_some() {
            return Err(AiSettingsError::DuplicateId(draft.id));
        }

        let (kind, secret_writes) = build_kind_for_add(&draft.id, draft.kind);

        for write in &secret_writes {
            self.secrets.set(&write.key_ref, &write.value)?;
        }

        let new_entry = AiProviderEntry {
            id: draft.id,
            name: draft.name,
            kind,
        };

        let mut new_file = self.file.clone();
        new_file.providers.push(new_entry);

        if let Err(err) = save_atomic(&self.path, &new_file) {
            for write in &secret_writes {
                let _ = self.secrets.delete(&write.key_ref);
            }
            return Err(err);
        }

        self.file = new_file;
        Ok(self.file.providers.last().expect("just-added entry"))
    }

    /// Update the entry whose id equals `id` with `draft`.
    ///
    /// The kind variant of `draft.kind` must match the existing entry's
    /// kind ([`AiSettingsError::KindMismatch`] otherwise); use
    /// `delete` + `add` to migrate between kinds.
    ///
    /// For an [`SecretField::Set`] api-key the existing secret is read
    /// (so it can be restored on TOML-write failure), the new value is
    /// written to the keyring, then the TOML is saved. For
    /// [`SecretField::Keep`] the keyring is untouched.
    ///
    /// # Errors
    ///
    /// - [`AiSettingsError::NotFound`] if no entry has id `id`.
    /// - [`AiSettingsError::KindMismatch`] if `draft.kind` is a
    ///   different variant than the existing entry's kind.
    /// - [`AiSettingsError::Secret`] for keyring failures.
    /// - [`AiSettingsError::Io`] / [`AiSettingsError::Serialize`] from
    ///   the TOML write; any keyring write performed by this call is
    ///   restored to its previous value before the error is returned.
    pub fn update(
        &mut self,
        id: &str,
        draft: AiProviderEditDraft,
    ) -> Result<&AiProviderEntry, AiSettingsError> {
        let idx = self
            .find_index(id)
            .ok_or_else(|| AiSettingsError::NotFound(id.to_string()))?;

        let existing_kind = self.file.providers[idx].kind.clone();
        let (new_kind, applied_writes) = self.apply_update_kind(id, &existing_kind, draft.kind)?;

        let new_entry = AiProviderEntry {
            id: id.to_string(),
            name: draft.name,
            kind: new_kind,
        };

        let mut new_file = self.file.clone();
        new_file.providers[idx] = new_entry;

        if let Err(err) = save_atomic(&self.path, &new_file) {
            for write in &applied_writes {
                let _ = match &write.old_value {
                    Some(old) => self.secrets.set(&write.key_ref, old),
                    None => self.secrets.delete(&write.key_ref),
                };
            }
            return Err(err);
        }

        self.file = new_file;
        Ok(&self.file.providers[idx])
    }

    /// Delete the entry whose id equals `id`.
    ///
    /// If the deleted entry was the active one, `active_id` is cleared
    /// in the same write so the file never lands with a dangling
    /// pointer.
    ///
    /// Persists the updated TOML first (the file is the source of
    /// truth), then best-effort purges the keyring entry the deleted
    /// entry referenced. A keyring purge failure does **not** fail the
    /// call: an orphan keyring entry is harmless because nothing
    /// references it any more.
    ///
    /// # Errors
    ///
    /// - [`AiSettingsError::NotFound`] if no entry has id `id`.
    /// - [`AiSettingsError::Io`] / [`AiSettingsError::Serialize`] from
    ///   the TOML write.
    pub fn delete(&mut self, id: &str) -> Result<(), AiSettingsError> {
        let idx = self
            .find_index(id)
            .ok_or_else(|| AiSettingsError::NotFound(id.to_string()))?;

        let mut new_file = self.file.clone();
        let removed = new_file.providers.remove(idx);
        if new_file.active_id.as_deref() == Some(id) {
            new_file.active_id = None;
        }

        save_atomic(&self.path, &new_file)?;
        self.file = new_file;

        for key_ref in keyring_refs_in(&removed.kind) {
            let _ = self.secrets.delete(&key_ref);
        }

        Ok(())
    }

    /// Set (or clear) the active provider pointer.
    ///
    /// Passing `Some(id)` for an id that no entry has fails with
    /// [`AiSettingsError::NotFound`] — better than the panel silently
    /// degrading to "no provider" on next startup.
    ///
    /// Passing `None` clears the active pointer; the panel degrades to
    /// hidden until the user picks one again. This is a TOML-only
    /// mutation — the keyring is never touched.
    ///
    /// # Errors
    ///
    /// - [`AiSettingsError::NotFound`] if `Some(id)` references no
    ///   existing entry.
    /// - [`AiSettingsError::Io`] / [`AiSettingsError::Serialize`] from
    ///   the TOML write.
    pub fn set_active(&mut self, id: Option<String>) -> Result<(), AiSettingsError> {
        if let Some(target) = id.as_deref() {
            if self.find_index(target).is_none() {
                return Err(AiSettingsError::NotFound(target.to_string()));
            }
        }
        let mut new_file = self.file.clone();
        new_file.active_id = id;
        save_atomic(&self.path, &new_file)?;
        self.file = new_file;
        Ok(())
    }

    fn find_index(&self, id: &str) -> Option<usize> {
        self.file.providers.iter().position(|e| e.id == id)
    }

    fn apply_update_kind(
        &self,
        id: &str,
        existing: &AiProviderKind,
        draft_kind: AiProviderKindEditDraft,
    ) -> Result<(AiProviderKind, Vec<AppliedSecretWrite>), AiSettingsError> {
        let mut applied = Vec::new();

        let new_kind = match (existing, draft_kind) {
            (
                AiProviderKind::Anthropic {
                    keyring_api_key_ref,
                    ..
                },
                AiProviderKindEditDraft::Anthropic { model, api_key },
            ) => {
                if let SecretField::Set(new_value) = api_key {
                    self.apply_secret_write(keyring_api_key_ref, &new_value, &mut applied)?;
                }
                AiProviderKind::Anthropic {
                    model,
                    keyring_api_key_ref: keyring_api_key_ref.clone(),
                }
            }
        };

        // Discriminant changes between variants would fall through to
        // this arm once a second variant lands. The current single-
        // variant enum means rustc proves the match exhaustive without
        // needing it, but `(_, _)` here would make it dead code.
        // Documenting the intent so the next variant can add the
        // mismatch branch without thinking about it again.
        let _ = id;
        Ok((new_kind, applied))
    }

    fn apply_secret_write(
        &self,
        key_ref: &str,
        new_value: &str,
        applied: &mut Vec<AppliedSecretWrite>,
    ) -> Result<(), AiSettingsError> {
        let old_value = match self.secrets.get(key_ref) {
            Ok(value) => Some(value),
            Err(SecretError::NotFound(_)) => None,
            Err(err) => return Err(AiSettingsError::Secret(err)),
        };
        self.secrets.set(key_ref, new_value)?;
        applied.push(AppliedSecretWrite {
            key_ref: key_ref.to_string(),
            old_value,
        });
        Ok(())
    }
}

/// Compute the keyring ref for a given AI provider id and field.
///
/// The `ai.` infix is what keeps this namespace from colliding with
/// the connection store's `dbboard.<id>.<field>` namespace.
fn keyring_ref(id: &str, field: &str) -> String {
    format!("dbboard.ai.{id}.{field}")
}

/// Enumerate every keyring ref that a given [`AiProviderKind`] points
/// at. `Anthropic` carries exactly one (`api_key`).
fn keyring_refs_in(kind: &AiProviderKind) -> Vec<String> {
    match kind {
        AiProviderKind::Anthropic {
            keyring_api_key_ref,
            ..
        } => vec![keyring_api_key_ref.clone()],
    }
}

/// Pending secret write computed for an `add` call.
struct PendingSecretWrite {
    key_ref: String,
    value: String,
}

/// Record of an already-committed secret write performed for an
/// `update` call. The `old_value` is kept so we can restore it if the
/// follow-up TOML write fails.
struct AppliedSecretWrite {
    key_ref: String,
    old_value: Option<String>,
}

fn build_kind_for_add(
    id: &str,
    draft: AiProviderKindDraft,
) -> (AiProviderKind, Vec<PendingSecretWrite>) {
    match draft {
        AiProviderKindDraft::Anthropic { model, api_key } => {
            let api_key_ref = keyring_ref(id, "api_key");
            let kind = AiProviderKind::Anthropic {
                model,
                keyring_api_key_ref: api_key_ref.clone(),
            };
            let writes = vec![PendingSecretWrite {
                key_ref: api_key_ref,
                value: api_key,
            }];
            (kind, writes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::InMemorySecretStore;
    use tempfile::tempdir;

    fn fresh_admin() -> (tempfile::TempDir, Arc<InMemorySecretStore>, AiSettingsAdmin) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ai-providers.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let admin = AiSettingsAdmin::open(path, secrets.clone() as Arc<dyn SecretStore>)
            .expect("open empty admin");
        (dir, secrets, admin)
    }

    fn anthropic_draft(id: &str, name: &str, api_key: &str) -> AiProviderDraft {
        AiProviderDraft {
            id: id.to_string(),
            name: name.to_string(),
            kind: AiProviderKindDraft::Anthropic {
                model: None,
                api_key: api_key.to_string(),
            },
        }
    }

    #[test]
    fn open_on_missing_file_yields_an_empty_admin() {
        let (_dir, _secrets, admin) = fresh_admin();
        assert!(admin.entries().is_empty());
        assert!(admin.active_id().is_none());
    }

    #[test]
    fn add_anthropic_routes_api_key_through_secret_store() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test-1"))
            .expect("add");
        let entry = &admin.entries()[0];
        match &entry.kind {
            AiProviderKind::Anthropic {
                keyring_api_key_ref,
                model,
            } => {
                assert_eq!(keyring_api_key_ref, "dbboard.ai.main.api_key");
                assert!(model.is_none());
            }
        }
        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("api key"),
            "sk-test-1"
        );
    }

    #[test]
    fn keyring_namespace_does_not_collide_with_the_connection_namespace() {
        // A connection with id "main" would use `dbboard.main.token` /
        // `dbboard.main.url`; an AI provider with the same id uses
        // `dbboard.ai.main.api_key`. The two must coexist.
        let (_dir, secrets, mut admin) = fresh_admin();
        secrets
            .set("dbboard.main.token", "connection-token")
            .expect("seed conn token");
        admin
            .add(anthropic_draft("main", "Claude", "sk-ai"))
            .expect("add ai");
        assert_eq!(
            secrets.get("dbboard.main.token").expect("conn token"),
            "connection-token"
        );
        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("ai key"),
            "sk-ai"
        );
    }

    #[test]
    fn add_persists_to_disk_so_reopen_reads_back_the_same_entries() {
        let (dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");

        let path = dir.path().join("ai-providers.toml");
        let reopen_secrets = Arc::new(InMemorySecretStore::new());
        let reopened =
            AiSettingsAdmin::open(path, reopen_secrets as Arc<dyn SecretStore>).expect("reopen");
        assert_eq!(reopened.entries().len(), 1);
        assert_eq!(reopened.entries()[0].id, "main");
    }

    #[test]
    fn add_with_duplicate_id_is_rejected_and_does_not_overwrite_secrets() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("dup", "First", "first-key"))
            .expect("first add");
        let err = admin
            .add(anthropic_draft("dup", "Second", "second-key"))
            .expect_err("second add must fail");
        match &err {
            AiSettingsError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.ai.dup.api_key").expect("api key"),
            "first-key"
        );
    }

    #[test]
    fn add_rolls_back_the_secret_write_when_the_toml_save_fails() {
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"i am a file, not a dir").expect("seed blocker");
        let path = blocker.join("ai-providers.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut admin = AiSettingsAdmin::new_with_file(
            path,
            secrets.clone() as Arc<dyn SecretStore>,
            AiProviderFile::empty(),
        );

        let err = admin
            .add(anthropic_draft("rolled-back", "X", "secret-value"))
            .expect_err("save must fail when parent is a file");
        assert!(
            matches!(err, AiSettingsError::Io(_)),
            "expected Io error, got {err:?}"
        );
        assert!(matches!(
            secrets.get("dbboard.ai.rolled-back.api_key"),
            Err(SecretError::NotFound(_))
        ));
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn update_anthropic_with_secret_keep_does_not_touch_the_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-original"))
            .expect("add");

        admin
            .update(
                "main",
                AiProviderEditDraft {
                    name: "Renamed Claude".to_string(),
                    kind: AiProviderKindEditDraft::Anthropic {
                        model: Some("claude-opus-4-8".to_string()),
                        api_key: SecretField::Keep,
                    },
                },
            )
            .expect("update with keep");

        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("api key"),
            "sk-original"
        );
        assert_eq!(admin.entries()[0].name, "Renamed Claude");
        match &admin.entries()[0].kind {
            AiProviderKind::Anthropic { model, .. } => {
                assert_eq!(model.as_deref(), Some("claude-opus-4-8"));
            }
        }
    }

    #[test]
    fn update_anthropic_with_secret_set_overwrites_the_keyring_entry() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-old"))
            .expect("add");

        admin
            .update(
                "main",
                AiProviderEditDraft {
                    name: "Claude".to_string(),
                    kind: AiProviderKindEditDraft::Anthropic {
                        model: None,
                        api_key: SecretField::Set("sk-new".to_string()),
                    },
                },
            )
            .expect("update with set");

        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("api key"),
            "sk-new"
        );
    }

    #[test]
    fn update_unknown_id_returns_not_found() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let err = admin
            .update(
                "missing",
                AiProviderEditDraft {
                    name: "X".to_string(),
                    kind: AiProviderKindEditDraft::Anthropic {
                        model: None,
                        api_key: SecretField::Keep,
                    },
                },
            )
            .expect_err("missing id must error");
        match &err {
            AiSettingsError::NotFound(id) => assert_eq!(id, "missing"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn update_restores_old_secret_when_the_toml_save_fails() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ai-providers.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut admin =
            AiSettingsAdmin::open(path, secrets.clone() as Arc<dyn SecretStore>).expect("open");
        admin
            .add(anthropic_draft("main", "Claude", "sk-original"))
            .expect("seed");
        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("seeded"),
            "sk-original"
        );

        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"file-not-dir").expect("seed blocker");
        admin.path = blocker.join("ai-providers.toml");

        let err = admin
            .update(
                "main",
                AiProviderEditDraft {
                    name: "About to fail".to_string(),
                    kind: AiProviderKindEditDraft::Anthropic {
                        model: None,
                        api_key: SecretField::Set("sk-about-to-fail".to_string()),
                    },
                },
            )
            .expect_err("save must fail");
        assert!(
            matches!(err, AiSettingsError::Io(_)),
            "expected Io error, got {err:?}"
        );

        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("api key"),
            "sk-original"
        );
        assert_eq!(admin.entries()[0].name, "Claude");
    }

    #[test]
    fn delete_removes_entry_and_purges_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");
        assert_eq!(
            secrets.get("dbboard.ai.main.api_key").expect("seeded"),
            "sk-test"
        );

        admin.delete("main").expect("delete");

        assert!(admin.entries().is_empty());
        assert!(matches!(
            secrets.get("dbboard.ai.main.api_key"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn delete_clears_active_id_when_the_active_entry_is_removed() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");
        admin
            .set_active(Some("main".to_string()))
            .expect("set active");
        assert_eq!(admin.active_id(), Some("main"));

        admin.delete("main").expect("delete");

        assert!(admin.active_id().is_none());
    }

    #[test]
    fn delete_keeps_active_id_when_a_different_entry_is_removed() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("primary", "Primary", "sk-p"))
            .expect("add primary");
        admin
            .add(anthropic_draft("secondary", "Secondary", "sk-s"))
            .expect("add secondary");
        admin
            .set_active(Some("primary".to_string()))
            .expect("set active");

        admin.delete("secondary").expect("delete other");

        assert_eq!(admin.active_id(), Some("primary"));
    }

    #[test]
    fn delete_unknown_id_returns_not_found() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let err = admin.delete("missing").expect_err("missing id must error");
        match &err {
            AiSettingsError::NotFound(id) => assert_eq!(id, "missing"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn delete_succeeds_even_when_the_keyring_entry_is_already_gone() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");
        secrets
            .delete("dbboard.ai.main.api_key")
            .expect("pre-clear keyring");

        admin.delete("main").expect("delete must still succeed");
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn set_active_to_a_valid_id_persists_the_pointer() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");

        admin
            .set_active(Some("main".to_string()))
            .expect("set active");
        assert_eq!(admin.active_id(), Some("main"));
    }

    #[test]
    fn set_active_to_an_unknown_id_returns_not_found_and_does_not_persist() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");

        let err = admin
            .set_active(Some("nope".to_string()))
            .expect_err("unknown id must error");
        match &err {
            AiSettingsError::NotFound(id) => assert_eq!(id, "nope"),
            other => panic!("expected NotFound, got {other:?}"),
        }
        assert!(admin.active_id().is_none());
    }

    #[test]
    fn set_active_to_none_clears_the_pointer() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");
        admin
            .set_active(Some("main".to_string()))
            .expect("set active");

        admin.set_active(None).expect("clear active");
        assert!(admin.active_id().is_none());
    }

    #[test]
    fn set_active_persists_through_reopen() {
        let (dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(anthropic_draft("main", "Claude", "sk-test"))
            .expect("add");
        admin
            .set_active(Some("main".to_string()))
            .expect("set active");

        let path = dir.path().join("ai-providers.toml");
        let reopen_secrets = Arc::new(InMemorySecretStore::new());
        let reopened =
            AiSettingsAdmin::open(path, reopen_secrets as Arc<dyn SecretStore>).expect("reopen");
        assert_eq!(reopened.active_id(), Some("main"));
    }
}
