//! Connection admin use-case (ADR-0016).
//!
//! Lives in `dbboard-config` because this crate already owns the TOML
//! surface ([`crate::store`]) and the keyring surface ([`crate::secrets`]).
//! Adding the use-case here avoids `dbboard-ui` ever touching the
//! filesystem or the OS keychain directly — the UI layer holds a
//! `ConnectionAdmin` and calls `entries()` / `add()` / `update()` /
//! `delete()` only.
//!
//! The two stores (TOML on disk, secrets in the OS keychain) must not
//! be allowed to drift. The committal order is fixed:
//!
//! - **Add:** write secrets first, then save TOML. On TOML-write
//!   failure the secret writes are rolled back so an orphan keyring
//!   entry cannot survive a half-finished add.
//! - **Update:** for every secret field the caller chose to overwrite,
//!   read the old value, write the new value, then save TOML. On
//!   TOML-write failure each updated secret is restored from the old
//!   value, again preventing keyring/TOML divergence.
//! - **Delete:** save TOML first (the file is the source of truth),
//!   then best-effort purge the keyring. An orphan keyring entry left
//!   by a purge failure is harmless: nothing references it any more.
//!
//! Kind changes are intentionally not supported on update: changing
//! kind would force migrating keyring references mid-flight, which
//! collapses the rollback story above. Users that want to change kind
//! must delete + re-add.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::bundle::{decrypt_bundle, encrypt_bundle, validate_passphrase, BundlePayload};
use crate::error::ConfigError;
use crate::secrets::{SecretError, SecretStore};
use crate::store::{load_or_empty, save_atomic, ConnectionEntry, ConnectionFile, ConnectionKind};

/// User-supplied draft for **adding** a new connection.
///
/// Unlike [`ConnectionEntry`] the secret material is carried inline
/// (e.g. `ConnectionKindDraft::D1::token`) rather than as a
/// `keyring_*_ref`. [`ConnectionAdmin::add`] derives the keyring ref
/// from the connection id and routes the inline value through the
/// configured [`SecretStore`].
#[derive(Debug, Clone)]
pub struct ConnectionDraft {
    pub id: String,
    pub name: String,
    pub kind: ConnectionKindDraft,
}

/// Add-time, inline-secret companion to [`ConnectionKind`].
#[derive(Debug, Clone)]
pub enum ConnectionKindDraft {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
        token: String,
    },
    Postgres {
        url: String,
    },
    Neon {
        url: String,
    },
    Supabase {
        url: String,
    },
    AuroraDsql {
        url: String,
    },
}

/// User-supplied draft for **editing** an existing connection.
///
/// The id is read-only on update (it is the primary key of both the
/// TOML and every keyring entry that references it); only `name` and
/// adapter-specific fields can change. Secret fields use
/// [`SecretField`] to distinguish "leave the keyring alone" from
/// "overwrite the keyring entry with this new value", because the
/// existing secret is never read back into the UI (ADR-0016).
#[derive(Debug, Clone)]
pub struct ConnectionEditDraft {
    pub name: String,
    pub kind: ConnectionKindEditDraft,
}

/// Edit-time companion to [`ConnectionKind`]. Variant must match the
/// existing entry's kind; changing kind on update is rejected with
/// [`ConfigError::KindMismatch`].
#[derive(Debug, Clone)]
pub enum ConnectionKindEditDraft {
    Turso {
        path: String,
    },
    D1 {
        account_id: String,
        database_id: String,
        base_url: Option<String>,
        token: SecretField,
    },
    Postgres {
        url: SecretField,
    },
    Neon {
        url: SecretField,
    },
    Supabase {
        url: SecretField,
    },
    AuroraDsql {
        url: SecretField,
    },
    /// Aurora DSQL IAM (ADR-0036). Carries no editable field: this kind
    /// is config-file-only in v1, so the UI never offers an editable form
    /// for it. The variant exists only so the edit state machine is
    /// total; any `update()` targeting it falls through
    /// [`ConnectionAdmin::apply_update_kind`]'s catch-all as a
    /// [`ConfigError::KindMismatch`].
    AuroraDsqlIam,
}

/// Whether an editable secret field should be left alone or rewritten.
#[derive(Debug, Clone)]
pub enum SecretField {
    /// Keep the existing keyring entry untouched. Used when the user
    /// edited a non-secret field and left the secret input blank.
    Keep,
    /// Overwrite the keyring entry with this new value.
    Set(String),
}

/// Outcome of [`ConnectionAdmin::import_bundle`] (ADR-0038).
///
/// Import is **additive and non-destructive**: an incoming id that
/// already exists in the live store is never overwritten. Instead it is
/// recorded in [`ImportReport::skipped`] so the UI can tell the user
/// exactly which connections were left untouched, while
/// [`ImportReport::imported`] lists the ids that were newly added. Both
/// preserve the order in which the bundle presented its entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Ids added to the store by this import.
    pub imported: Vec<String>,
    /// Ids present in the bundle but skipped because an entry with the
    /// same id already existed (or the bundle listed the id twice).
    pub skipped: Vec<String>,
}

/// Owns the on-disk TOML file plus an [`Arc<dyn SecretStore>`] handle
/// and exposes a small CRUD API over the pair.
///
/// Construct one per process at startup via [`ConnectionAdmin::open`]
/// (or [`ConnectionAdmin::new_with_file`] in tests), pass it down to
/// the UI as `Arc<Mutex<ConnectionAdmin>>` (or equivalent), and let
/// it route all mutations through here so the TOML and the keyring
/// stay in sync.
pub struct ConnectionAdmin {
    path: PathBuf,
    secrets: Arc<dyn SecretStore>,
    file: ConnectionFile,
}

impl ConnectionAdmin {
    /// Load `connections.toml` from `path` (an empty store is returned
    /// when the file does not exist) and pair it with `secrets`.
    ///
    /// # Errors
    ///
    /// Any error from [`load_or_empty`] — schema parse failure,
    /// unsupported version, duplicate id, or non-`NotFound` I/O.
    pub fn open(path: PathBuf, secrets: Arc<dyn SecretStore>) -> Result<Self, ConfigError> {
        let file = load_or_empty(&path)?;
        Ok(Self {
            path,
            secrets,
            file,
        })
    }

    /// Construct from an explicit in-memory file, without reading
    /// the disk. Intended for tests; production callers should use
    /// [`ConnectionAdmin::open`].
    #[must_use]
    pub fn new_with_file(
        path: PathBuf,
        secrets: Arc<dyn SecretStore>,
        file: ConnectionFile,
    ) -> Self {
        Self {
            path,
            secrets,
            file,
        }
    }

    /// Borrow the current entries. The UI uses this to render the
    /// connection list and to drive selection state.
    #[must_use]
    pub fn entries(&self) -> &[ConnectionEntry] {
        &self.file.connections
    }

    /// Add `draft` as a new connection.
    ///
    /// Writes any secret material to the [`SecretStore`] under a
    /// `dbboard.<id>.<field>` reference, then persists the updated
    /// TOML. If the TOML write fails, every secret write performed in
    /// this call is rolled back so an orphan keyring entry cannot
    /// survive.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::DuplicateId`] if `draft.id` already exists.
    /// - [`ConfigError::Secret`] if a secret write fails.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML
    ///   write; in this case any secret writes performed by this call
    ///   have already been rolled back.
    ///
    /// # Panics
    ///
    /// Never in practice: the just-pushed entry is borrowed back from
    /// the in-memory file via `last()`. A panic here would imply a bug
    /// in `Vec::push` itself.
    pub fn add(&mut self, draft: ConnectionDraft) -> Result<&ConnectionEntry, ConfigError> {
        if self.find_index(&draft.id).is_some() {
            return Err(ConfigError::DuplicateId(draft.id));
        }

        let (kind, secret_writes) = build_kind_for_add(&draft.id, draft.kind);

        for write in &secret_writes {
            self.secrets.set(&write.key_ref, &write.value)?;
        }

        let new_entry = ConnectionEntry {
            id: draft.id,
            name: draft.name,
            kind,
        };

        let mut new_file = self.file.clone();
        new_file.connections.push(new_entry);

        if let Err(err) = save_atomic(&self.path, &new_file) {
            // The secret writes succeeded but the file write did not.
            // Roll the keyring back to whatever it held before this call.
            for write in &secret_writes {
                let _ = self.secrets.delete(&write.key_ref);
            }
            return Err(err);
        }

        self.file = new_file;
        Ok(self.file.connections.last().expect("just-added entry"))
    }

    /// Update the entry whose id equals `id` with `draft`.
    ///
    /// The kind variant of `draft.kind` must match the existing entry's
    /// kind ([`ConfigError::KindMismatch`] otherwise); use delete + add
    /// to migrate between kinds.
    ///
    /// For each [`SecretField::Set`] in `draft.kind` the existing
    /// secret is read so it can be restored on TOML-write failure,
    /// then overwritten in the keyring before the TOML save. For each
    /// [`SecretField::Keep`] the keyring is untouched.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::NotFound`] if no entry has id `id`.
    /// - [`ConfigError::KindMismatch`] if `draft.kind` is a different
    ///   variant than the existing entry's kind.
    /// - [`ConfigError::Secret`] for keyring failures.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML
    ///   write; any keyring writes performed by this call are
    ///   restored to their previous values before the error is
    ///   returned.
    pub fn update(
        &mut self,
        id: &str,
        draft: ConnectionEditDraft,
    ) -> Result<&ConnectionEntry, ConfigError> {
        let idx = self
            .find_index(id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;

        let existing_kind = self.file.connections[idx].kind.clone();
        let (new_kind, applied_writes) = self.apply_update_kind(id, &existing_kind, draft.kind)?;

        let new_entry = ConnectionEntry {
            id: id.to_string(),
            name: draft.name,
            kind: new_kind,
        };

        let mut new_file = self.file.clone();
        new_file.connections[idx] = new_entry;

        if let Err(err) = save_atomic(&self.path, &new_file) {
            for write in &applied_writes {
                // Restore the old value if we had one; if we did not
                // (the keyring was empty before this update), delete
                // the just-written entry so we leave no orphan.
                let _ = match &write.old_value {
                    Some(old) => self.secrets.set(&write.key_ref, old),
                    None => self.secrets.delete(&write.key_ref),
                };
            }
            return Err(err);
        }

        self.file = new_file;
        Ok(&self.file.connections[idx])
    }

    /// Delete the entry whose id equals `id`.
    ///
    /// Persists the updated TOML first (the file is the source of
    /// truth), then best-effort purges any keyring entries the
    /// deleted entry referenced. A keyring purge failure does **not**
    /// fail the call: an orphan keyring entry is harmless because
    /// nothing references it any more.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::NotFound`] if no entry has id `id`.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML
    ///   write.
    pub fn delete(&mut self, id: &str) -> Result<(), ConfigError> {
        let idx = self
            .find_index(id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;

        let mut new_file = self.file.clone();
        let removed = new_file.connections.remove(idx);

        save_atomic(&self.path, &new_file)?;
        self.file = new_file;

        // Orphan keyring entries (either missing already, or left
        // behind by a backend purge failure) are harmless: the TOML is
        // the source of truth and nothing references them any more.
        for key_ref in keyring_refs_in(&removed.kind) {
            let _ = self.secrets.delete(&key_ref);
        }

        Ok(())
    }

    /// Encrypt the entire connection store — every entry plus every
    /// secret it references — into a passphrase-protected bundle blob
    /// (ADR-0038, slice b). The returned bytes are written verbatim to a
    /// user-chosen `.dbbx` file by the UI layer.
    ///
    /// The v1 scope is **all connections at once**: the collector handoff
    /// (#14) wants a whole machine's connection set in one artifact, and a
    /// per-connection picker adds UI without a real use case yet.
    ///
    /// Every `keyring_*_ref` on every entry is resolved through the
    /// [`SecretStore`] and packed alongside the metadata, because the TOML
    /// alone is useless on another machine (it stores only references).
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Bundle`] if `passphrase` is weaker than
    ///   [`crate::MIN_PASSPHRASE_LEN`], or the age encryptor fails.
    /// - [`ConfigError::Secret`] if a referenced secret cannot be read
    ///   from the keychain. Export fails loudly here rather than shipping
    ///   a bundle that is silently missing a secret.
    pub fn export_bundle(&self, passphrase: &str) -> Result<Vec<u8>, ConfigError> {
        // Reject a weak passphrase before touching the keychain, so a
        // typo costs nothing.
        validate_passphrase(passphrase)?;

        let mut secrets = BTreeMap::new();
        for entry in &self.file.connections {
            for key_ref in keyring_refs_in(&entry.kind) {
                let value = self.secrets.get(&key_ref)?;
                secrets.insert(key_ref, value);
            }
        }

        let payload = BundlePayload::new(self.file.clone(), secrets);
        let blob = encrypt_bundle(&payload, passphrase)?;
        Ok(blob)
    }

    /// Decrypt `blob` under `passphrase` and merge its connections into
    /// the live store (ADR-0038, slice b), returning an [`ImportReport`]
    /// of which ids were added and which were skipped.
    ///
    /// Import is **additive and id-conflict-safe**: an incoming id that
    /// already exists (or that the bundle lists twice) is skipped and
    /// reported, never overwritten — the user's current secrets and
    /// metadata are the source of truth. Newly-added entries seed their
    /// secrets into the keychain first, then the TOML is persisted; on a
    /// TOML-write failure the just-seeded secrets are rolled back so no
    /// orphan keyring entry survives, exactly as [`ConnectionAdmin::add`]
    /// does.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Bundle`] if the passphrase is wrong or the blob is
    ///   corrupt / not a dbboard bundle / a newer bundle version.
    /// - [`ConfigError::Secret`] if seeding an imported secret fails; any
    ///   secrets already seeded by this call are rolled back first.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML
    ///   write; the seeded secrets are rolled back before returning.
    pub fn import_bundle(
        &mut self,
        blob: &[u8],
        passphrase: &str,
    ) -> Result<ImportReport, ConfigError> {
        let payload = decrypt_bundle(blob, passphrase)?;

        // Ids we must not clobber: everything already in the store, plus
        // anything we accept earlier in this same bundle (so a bundle that
        // lists an id twice skips the second occurrence rather than
        // creating a duplicate entry).
        let mut seen: HashSet<String> =
            self.file.connections.iter().map(|e| e.id.clone()).collect();

        let mut report = ImportReport::default();
        let mut to_add: Vec<ConnectionEntry> = Vec::new();
        let mut secret_writes: Vec<(String, String)> = Vec::new();

        for entry in payload.connections.connections {
            if seen.contains(&entry.id) {
                report.skipped.push(entry.id);
                continue;
            }
            for key_ref in keyring_refs_in(&entry.kind) {
                // A well-formed dbboard bundle carries every secret it
                // references; if one is absent we still import the entry's
                // metadata (the user can re-enter the secret via edit)
                // rather than dropping the whole connection.
                if let Some(value) = payload.secrets.get(&key_ref) {
                    secret_writes.push((key_ref, value.clone()));
                }
            }
            seen.insert(entry.id.clone());
            report.imported.push(entry.id.clone());
            to_add.push(entry);
        }

        if to_add.is_empty() {
            return Ok(report);
        }

        // Seed secrets first (same order as `add`); track what we wrote so
        // a later failure can undo it.
        let mut written: Vec<String> = Vec::new();
        for (key_ref, value) in &secret_writes {
            if let Err(err) = self.secrets.set(key_ref, value) {
                self.rollback_secret_writes(&written);
                return Err(ConfigError::Secret(err));
            }
            written.push(key_ref.clone());
        }

        let mut new_file = self.file.clone();
        new_file.connections.extend(to_add);

        if let Err(err) = save_atomic(&self.path, &new_file) {
            self.rollback_secret_writes(&written);
            return Err(err);
        }

        self.file = new_file;
        Ok(report)
    }

    /// Best-effort delete of secrets seeded earlier in a failed
    /// [`import_bundle`]. Imported ids are new to the store, so deleting
    /// their refs cannot clobber a still-referenced secret; a delete
    /// failure is ignored because the surviving orphan is harmless.
    fn rollback_secret_writes(&self, written: &[String]) {
        for key_ref in written {
            let _ = self.secrets.delete(key_ref);
        }
    }

    fn find_index(&self, id: &str) -> Option<usize> {
        self.file.connections.iter().position(|e| e.id == id)
    }

    fn apply_update_kind(
        &self,
        id: &str,
        existing: &ConnectionKind,
        draft_kind: ConnectionKindEditDraft,
    ) -> Result<(ConnectionKind, Vec<AppliedSecretWrite>), ConfigError> {
        let mut applied = Vec::new();

        let new_kind = match (existing, draft_kind) {
            (ConnectionKind::Turso { .. }, ConnectionKindEditDraft::Turso { path }) => {
                ConnectionKind::Turso { path }
            }
            (
                ConnectionKind::D1 {
                    keyring_token_ref, ..
                },
                ConnectionKindEditDraft::D1 {
                    account_id,
                    database_id,
                    base_url,
                    token,
                },
            ) => {
                if let SecretField::Set(new_value) = token {
                    self.apply_secret_write(keyring_token_ref, &new_value, &mut applied)?;
                }
                ConnectionKind::D1 {
                    account_id,
                    database_id,
                    base_url,
                    keyring_token_ref: keyring_token_ref.clone(),
                }
            }
            (
                ConnectionKind::Postgres { keyring_url_ref },
                ConnectionKindEditDraft::Postgres { url },
            ) => {
                if let SecretField::Set(new_value) = url {
                    self.apply_secret_write(keyring_url_ref, &new_value, &mut applied)?;
                }
                ConnectionKind::Postgres {
                    keyring_url_ref: keyring_url_ref.clone(),
                }
            }
            (ConnectionKind::Neon { keyring_url_ref }, ConnectionKindEditDraft::Neon { url }) => {
                if let SecretField::Set(new_value) = url {
                    self.apply_secret_write(keyring_url_ref, &new_value, &mut applied)?;
                }
                ConnectionKind::Neon {
                    keyring_url_ref: keyring_url_ref.clone(),
                }
            }
            (
                ConnectionKind::Supabase { keyring_url_ref },
                ConnectionKindEditDraft::Supabase { url },
            ) => {
                if let SecretField::Set(new_value) = url {
                    self.apply_secret_write(keyring_url_ref, &new_value, &mut applied)?;
                }
                ConnectionKind::Supabase {
                    keyring_url_ref: keyring_url_ref.clone(),
                }
            }
            (
                ConnectionKind::AuroraDsql { keyring_url_ref },
                ConnectionKindEditDraft::AuroraDsql { url },
            ) => {
                if let SecretField::Set(new_value) = url {
                    self.apply_secret_write(keyring_url_ref, &new_value, &mut applied)?;
                }
                ConnectionKind::AuroraDsql {
                    keyring_url_ref: keyring_url_ref.clone(),
                }
            }
            (_, _) => {
                return Err(ConfigError::KindMismatch { id: id.to_string() });
            }
        };

        Ok((new_kind, applied))
    }

    fn apply_secret_write(
        &self,
        key_ref: &str,
        new_value: &str,
        applied: &mut Vec<AppliedSecretWrite>,
    ) -> Result<(), ConfigError> {
        // Read the old value first so the TOML-write rollback path can
        // restore it. NotFound is fine — the keyring may be empty if
        // this is the first time the entry has carried a real secret.
        let old_value = match self.secrets.get(key_ref) {
            Ok(value) => Some(value),
            Err(SecretError::NotFound(_)) => None,
            Err(err) => return Err(ConfigError::Secret(err)),
        };
        self.secrets.set(key_ref, new_value)?;
        applied.push(AppliedSecretWrite {
            key_ref: key_ref.to_string(),
            old_value,
        });
        Ok(())
    }
}

/// Compute the keyring ref for a given connection id and field.
fn keyring_ref(id: &str, field: &str) -> String {
    format!("dbboard.{id}.{field}")
}

/// Enumerate every keyring ref that a given [`ConnectionKind`] points
/// at. `Turso` has none; `D1`, `Postgres`, `Neon`, `Supabase`, and
/// `AuroraDsql` each carry exactly one; `AuroraDsqlIam` carries its AWS
/// secret-key ref (its other fields are non-secret and live inline).
fn keyring_refs_in(kind: &ConnectionKind) -> Vec<String> {
    match kind {
        ConnectionKind::Turso { .. } => Vec::new(),
        ConnectionKind::D1 {
            keyring_token_ref, ..
        } => vec![keyring_token_ref.clone()],
        ConnectionKind::Postgres { keyring_url_ref }
        | ConnectionKind::Neon { keyring_url_ref }
        | ConnectionKind::Supabase { keyring_url_ref }
        | ConnectionKind::AuroraDsql { keyring_url_ref } => {
            vec![keyring_url_ref.clone()]
        }
        ConnectionKind::AuroraDsqlIam {
            keyring_secret_key_ref,
            ..
        } => vec![keyring_secret_key_ref.clone()],
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
    draft: ConnectionKindDraft,
) -> (ConnectionKind, Vec<PendingSecretWrite>) {
    match draft {
        ConnectionKindDraft::Turso { path } => (ConnectionKind::Turso { path }, Vec::new()),
        ConnectionKindDraft::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } => {
            let token_ref = keyring_ref(id, "token");
            let kind = ConnectionKind::D1 {
                account_id,
                database_id,
                base_url,
                keyring_token_ref: token_ref.clone(),
            };
            let writes = vec![PendingSecretWrite {
                key_ref: token_ref,
                value: token,
            }];
            (kind, writes)
        }
        ConnectionKindDraft::Postgres { url } => {
            let url_ref = keyring_ref(id, "url");
            let kind = ConnectionKind::Postgres {
                keyring_url_ref: url_ref.clone(),
            };
            let writes = vec![PendingSecretWrite {
                key_ref: url_ref,
                value: url,
            }];
            (kind, writes)
        }
        ConnectionKindDraft::Neon { url } => {
            let url_ref = keyring_ref(id, "url");
            let kind = ConnectionKind::Neon {
                keyring_url_ref: url_ref.clone(),
            };
            let writes = vec![PendingSecretWrite {
                key_ref: url_ref,
                value: url,
            }];
            (kind, writes)
        }
        ConnectionKindDraft::Supabase { url } => {
            let url_ref = keyring_ref(id, "url");
            let kind = ConnectionKind::Supabase {
                keyring_url_ref: url_ref.clone(),
            };
            let writes = vec![PendingSecretWrite {
                key_ref: url_ref,
                value: url,
            }];
            (kind, writes)
        }
        ConnectionKindDraft::AuroraDsql { url } => {
            let url_ref = keyring_ref(id, "url");
            let kind = ConnectionKind::AuroraDsql {
                keyring_url_ref: url_ref.clone(),
            };
            let writes = vec![PendingSecretWrite {
                key_ref: url_ref,
                value: url,
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

    fn fresh_admin() -> (tempfile::TempDir, Arc<InMemorySecretStore>, ConnectionAdmin) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let admin = ConnectionAdmin::open(path, secrets.clone() as Arc<dyn SecretStore>)
            .expect("open empty admin");
        (dir, secrets, admin)
    }

    fn turso_draft(id: &str, name: &str, path: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: id.to_string(),
            name: name.to_string(),
            kind: ConnectionKindDraft::Turso {
                path: path.to_string(),
            },
        }
    }

    fn d1_draft(id: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: id.to_string(),
            name: format!("D1 {id}"),
            kind: ConnectionKindDraft::D1 {
                account_id: "acct".to_string(),
                database_id: "db".to_string(),
                base_url: None,
                token: "t0k3n".to_string(),
            },
        }
    }

    fn pg_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: id.to_string(),
            name: format!("PG {id}"),
            kind: ConnectionKindDraft::Postgres {
                url: url.to_string(),
            },
        }
    }

    fn neon_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: id.to_string(),
            name: format!("Neon {id}"),
            kind: ConnectionKindDraft::Neon {
                url: url.to_string(),
            },
        }
    }

    fn supabase_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: id.to_string(),
            name: format!("Supabase {id}"),
            kind: ConnectionKindDraft::Supabase {
                url: url.to_string(),
            },
        }
    }

    fn aurora_dsql_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            id: id.to_string(),
            name: format!("Aurora DSQL {id}"),
            kind: ConnectionKindDraft::AuroraDsql {
                url: url.to_string(),
            },
        }
    }

    #[test]
    fn open_on_missing_file_yields_an_empty_admin() {
        let (_dir, _secrets, admin) = fresh_admin();
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn add_turso_persists_the_entry_and_touches_no_secret() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Local libSQL", ":memory:"))
            .expect("add turso");
        assert_eq!(admin.entries().len(), 1);
        assert_eq!(admin.entries()[0].id, "local");
        assert_eq!(
            admin.entries()[0].kind,
            ConnectionKind::Turso {
                path: ":memory:".to_string(),
            }
        );
        // Turso has no secret fields, so the keyring stays empty.
        assert!(matches!(
            secrets.get("dbboard.local.token"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn add_d1_routes_token_through_secret_store_and_records_keyring_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("prod")).expect("add d1");
        let entry = &admin.entries()[0];
        match &entry.kind {
            ConnectionKind::D1 {
                keyring_token_ref, ..
            } => assert_eq!(keyring_token_ref, "dbboard.prod.token"),
            other => panic!("expected D1, got {other:?}"),
        }
        assert_eq!(secrets.get("dbboard.prod.token").expect("token"), "t0k3n");
    }

    #[test]
    fn add_postgres_routes_url_through_secret_store_and_records_keyring_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("neon", "postgres://example/db"))
            .expect("add pg");
        let entry = &admin.entries()[0];
        match &entry.kind {
            ConnectionKind::Postgres { keyring_url_ref } => {
                assert_eq!(keyring_url_ref, "dbboard.neon.url");
            }
            other => panic!("expected Postgres, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.neon.url").expect("url"),
            "postgres://example/db"
        );
    }

    #[test]
    fn add_neon_routes_url_through_secret_store_and_records_keyring_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(neon_draft(
                "prod-neon",
                "postgres://neon.example/db?sslmode=require",
            ))
            .expect("add neon");
        let entry = &admin.entries()[0];
        match &entry.kind {
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
    fn add_supabase_routes_url_through_secret_store_and_records_keyring_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(supabase_draft(
                "supabase-prod",
                "postgres://postgres:pw@db.example.supabase.co:5432/postgres?sslmode=require",
            ))
            .expect("add supabase");
        let entry = &admin.entries()[0];
        match &entry.kind {
            ConnectionKind::Supabase { keyring_url_ref } => {
                assert_eq!(keyring_url_ref, "dbboard.supabase-prod.url");
            }
            other => panic!("expected Supabase, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.supabase-prod.url").expect("url"),
            "postgres://postgres:pw@db.example.supabase.co:5432/postgres?sslmode=require"
        );
    }

    #[test]
    fn add_aurora_dsql_routes_url_through_secret_store_and_records_keyring_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(aurora_dsql_draft(
                "dsql-prod",
                "postgres://admin:iam-token@example.dsql.us-east-1.on.aws:5432/postgres?sslmode=require",
            ))
            .expect("add aurora-dsql");
        let entry = &admin.entries()[0];
        match &entry.kind {
            ConnectionKind::AuroraDsql { keyring_url_ref } => {
                assert_eq!(keyring_url_ref, "dbboard.dsql-prod.url");
            }
            other => panic!("expected AuroraDsql, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.dsql-prod.url").expect("url"),
            "postgres://admin:iam-token@example.dsql.us-east-1.on.aws:5432/postgres?sslmode=require"
        );
    }

    #[test]
    fn update_aurora_dsql_with_secret_set_overwrites_the_keyring_entry() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(aurora_dsql_draft(
                "dsql",
                "postgres://admin:old-token@example.dsql.us-east-1.on.aws/postgres",
            ))
            .expect("add");

        // IAM tokens expire ~15 min after issue (ADR-0021); rotating the
        // URL is the expected hot path for this kind.
        admin
            .update(
                "dsql",
                ConnectionEditDraft {
                    name: "Aurora DSQL dsql".to_string(),
                    kind: ConnectionKindEditDraft::AuroraDsql {
                        url: SecretField::Set(
                            "postgres://admin:new-token@example.dsql.us-east-1.on.aws/postgres"
                                .to_string(),
                        ),
                    },
                },
            )
            .expect("update with set");

        assert_eq!(
            secrets.get("dbboard.dsql.url").expect("url"),
            "postgres://admin:new-token@example.dsql.us-east-1.on.aws/postgres"
        );
    }

    #[test]
    fn update_aurora_dsql_with_secret_keep_does_not_touch_the_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(aurora_dsql_draft(
                "dsql",
                "postgres://admin:tok@example.dsql.us-east-1.on.aws/postgres",
            ))
            .expect("add");

        admin
            .update(
                "dsql",
                ConnectionEditDraft {
                    name: "Renamed Aurora DSQL".to_string(),
                    kind: ConnectionKindEditDraft::AuroraDsql {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update with keep");

        assert_eq!(
            secrets.get("dbboard.dsql.url").expect("url"),
            "postgres://admin:tok@example.dsql.us-east-1.on.aws/postgres"
        );
        assert_eq!(admin.entries()[0].name, "Renamed Aurora DSQL");
    }

    #[test]
    fn update_postgres_to_aurora_dsql_kind_is_rejected() {
        // Kind changes stay forbidden (ADR-0016 rule, carried by 0018,
        // 0019, and now 0021). Switching Postgres → Aurora DSQL requires
        // delete + re-add even though the keyring shape is identical.
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("pg", "postgres://example/db"))
            .expect("add");
        let err = admin
            .update(
                "pg",
                ConnectionEditDraft {
                    name: "pg".to_string(),
                    kind: ConnectionKindEditDraft::AuroraDsql {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect_err("kind change must be rejected");
        match &err {
            ConfigError::KindMismatch { id } => assert_eq!(id, "pg"),
            other => panic!("expected KindMismatch, got {other:?}"),
        }
    }

    #[test]
    fn delete_aurora_dsql_removes_entry_and_purges_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(aurora_dsql_draft(
                "dsql",
                "postgres://admin:tok@example.dsql.us-east-1.on.aws/postgres",
            ))
            .expect("add");
        assert_eq!(
            secrets.get("dbboard.dsql.url").expect("seeded"),
            "postgres://admin:tok@example.dsql.us-east-1.on.aws/postgres"
        );

        admin.delete("dsql").expect("delete");

        assert!(admin.entries().is_empty());
        assert!(matches!(
            secrets.get("dbboard.dsql.url"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn delete_aurora_dsql_iam_purges_the_secret_key_ref() {
        // The IAM kind is config-file-only in v1 (no add/edit draft), so
        // seed it directly and verify delete still purges its secret-key
        // keyring ref via `keyring_refs_in`.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        secrets
            .set("dbboard.dsql-iam.secret_key", "AWS_SECRET")
            .expect("seed secret");
        let file = ConnectionFile {
            version: crate::store::CONFIG_VERSION,
            connections: vec![ConnectionEntry {
                id: "dsql-iam".to_string(),
                name: "Aurora DSQL (IAM)".to_string(),
                kind: ConnectionKind::AuroraDsqlIam {
                    endpoint: "abc.dsql.ap-northeast-1.on.aws".to_string(),
                    region: "ap-northeast-1".to_string(),
                    database: "postgres".to_string(),
                    username: "admin".to_string(),
                    access_key_id: "AKIAEXAMPLE".to_string(),
                    keyring_secret_key_ref: "dbboard.dsql-iam.secret_key".to_string(),
                },
            }],
        };
        let mut admin =
            ConnectionAdmin::new_with_file(path, secrets.clone() as Arc<dyn SecretStore>, file);

        admin.delete("dsql-iam").expect("delete");

        assert!(admin.entries().is_empty());
        assert!(matches!(
            secrets.get("dbboard.dsql-iam.secret_key"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn update_aurora_dsql_iam_kind_is_rejected_as_mismatch() {
        // There is no IAM edit-draft, so any update targeting an IAM entry
        // falls through `apply_update_kind`'s catch-all as a KindMismatch
        // — v1 requires delete + re-add (hand-edit the TOML) to change it.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let file = ConnectionFile {
            version: crate::store::CONFIG_VERSION,
            connections: vec![ConnectionEntry {
                id: "dsql-iam".to_string(),
                name: "Aurora DSQL (IAM)".to_string(),
                kind: ConnectionKind::AuroraDsqlIam {
                    endpoint: "abc.dsql.ap-northeast-1.on.aws".to_string(),
                    region: "ap-northeast-1".to_string(),
                    database: "postgres".to_string(),
                    username: "admin".to_string(),
                    access_key_id: "AKIAEXAMPLE".to_string(),
                    keyring_secret_key_ref: "dbboard.dsql-iam.secret_key".to_string(),
                },
            }],
        };
        let mut admin = ConnectionAdmin::new_with_file(path, secrets as Arc<dyn SecretStore>, file);

        let err = admin
            .update(
                "dsql-iam",
                ConnectionEditDraft {
                    name: "renamed".to_string(),
                    kind: ConnectionKindEditDraft::AuroraDsql {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect_err("IAM update must be rejected");
        match &err {
            ConfigError::KindMismatch { id } => assert_eq!(id, "dsql-iam"),
            other => panic!("expected KindMismatch, got {other:?}"),
        }
    }

    #[test]
    fn update_supabase_with_secret_set_overwrites_the_keyring_entry() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(supabase_draft(
                "supabase",
                "postgres://postgres:old@db.example.supabase.co/postgres",
            ))
            .expect("add");

        admin
            .update(
                "supabase",
                ConnectionEditDraft {
                    name: "Supabase supabase".to_string(),
                    kind: ConnectionKindEditDraft::Supabase {
                        url: SecretField::Set(
                            "postgres://postgres:new@db.example.supabase.co/postgres".to_string(),
                        ),
                    },
                },
            )
            .expect("update with set");

        assert_eq!(
            secrets.get("dbboard.supabase.url").expect("url"),
            "postgres://postgres:new@db.example.supabase.co/postgres"
        );
    }

    #[test]
    fn update_supabase_with_secret_keep_does_not_touch_the_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(supabase_draft(
                "supabase",
                "postgres://postgres:pw@db.example.supabase.co/postgres",
            ))
            .expect("add");

        admin
            .update(
                "supabase",
                ConnectionEditDraft {
                    name: "Renamed Supabase".to_string(),
                    kind: ConnectionKindEditDraft::Supabase {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update with keep");

        assert_eq!(
            secrets.get("dbboard.supabase.url").expect("url"),
            "postgres://postgres:pw@db.example.supabase.co/postgres"
        );
        assert_eq!(admin.entries()[0].name, "Renamed Supabase");
    }

    #[test]
    fn update_postgres_to_supabase_kind_is_rejected() {
        // Kind changes are not supported on update (ADR-0019 keeps the
        // ADR-0016 rule, same as ADR-0018 for Neon). Switching from
        // Postgres to Supabase requires delete + re-add even though the
        // keyring shape is identical.
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("pg", "postgres://example/db"))
            .expect("add");
        let err = admin
            .update(
                "pg",
                ConnectionEditDraft {
                    name: "pg".to_string(),
                    kind: ConnectionKindEditDraft::Supabase {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect_err("kind change must be rejected");
        match &err {
            ConfigError::KindMismatch { id } => assert_eq!(id, "pg"),
            other => panic!("expected KindMismatch, got {other:?}"),
        }
    }

    #[test]
    fn delete_supabase_removes_entry_and_purges_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(supabase_draft(
                "supabase",
                "postgres://postgres:pw@db.example.supabase.co/postgres",
            ))
            .expect("add");
        assert_eq!(
            secrets.get("dbboard.supabase.url").expect("seeded"),
            "postgres://postgres:pw@db.example.supabase.co/postgres"
        );

        admin.delete("supabase").expect("delete");

        assert!(admin.entries().is_empty());
        assert!(matches!(
            secrets.get("dbboard.supabase.url"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn update_neon_with_secret_set_overwrites_the_keyring_entry() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(neon_draft("neon", "postgres://neon.example/old"))
            .expect("add");

        admin
            .update(
                "neon",
                ConnectionEditDraft {
                    name: "Neon neon".to_string(),
                    kind: ConnectionKindEditDraft::Neon {
                        url: SecretField::Set("postgres://neon.example/new".to_string()),
                    },
                },
            )
            .expect("update with set");

        assert_eq!(
            secrets.get("dbboard.neon.url").expect("url"),
            "postgres://neon.example/new"
        );
    }

    #[test]
    fn update_neon_with_secret_keep_does_not_touch_the_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(neon_draft("neon", "postgres://neon.example/db"))
            .expect("add");

        admin
            .update(
                "neon",
                ConnectionEditDraft {
                    name: "Renamed Neon".to_string(),
                    kind: ConnectionKindEditDraft::Neon {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update with keep");

        assert_eq!(
            secrets.get("dbboard.neon.url").expect("url"),
            "postgres://neon.example/db"
        );
        assert_eq!(admin.entries()[0].name, "Renamed Neon");
    }

    #[test]
    fn update_postgres_to_neon_kind_is_rejected() {
        // Kind changes are not supported on update (ADR-0018 keeps the
        // ADR-0016 rule). Switching from Postgres to Neon requires
        // delete + re-add even though the keyring shape is identical.
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("pg", "postgres://example/db"))
            .expect("add");
        let err = admin
            .update(
                "pg",
                ConnectionEditDraft {
                    name: "pg".to_string(),
                    kind: ConnectionKindEditDraft::Neon {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect_err("kind change must be rejected");
        match &err {
            ConfigError::KindMismatch { id } => assert_eq!(id, "pg"),
            other => panic!("expected KindMismatch, got {other:?}"),
        }
    }

    #[test]
    fn delete_neon_removes_entry_and_purges_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(neon_draft("neon", "postgres://neon.example/db"))
            .expect("add");
        assert_eq!(
            secrets.get("dbboard.neon.url").expect("seeded"),
            "postgres://neon.example/db"
        );

        admin.delete("neon").expect("delete");

        assert!(admin.entries().is_empty());
        assert!(matches!(
            secrets.get("dbboard.neon.url"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn add_persists_to_disk_so_reopen_reads_back_the_same_entries() {
        let (dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "L", ":memory:"))
            .expect("add");

        let path = dir.path().join("connections.toml");
        let reopen_secrets = Arc::new(InMemorySecretStore::new());
        let reopened =
            ConnectionAdmin::open(path, reopen_secrets as Arc<dyn SecretStore>).expect("reopen");
        assert_eq!(reopened.entries().len(), 1);
        assert_eq!(reopened.entries()[0].id, "local");
    }

    #[test]
    fn add_with_duplicate_id_is_rejected_and_does_not_touch_secrets() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("dup")).expect("first add");
        secrets
            .set("dbboard.dup.token", "first")
            .expect("seed via first add");
        let err = admin
            .add(d1_draft("dup"))
            .expect_err("second add must fail");
        match &err {
            ConfigError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
        // The first-add secret is untouched: the duplicate add must
        // not have overwritten it (it bailed before any secret write).
        assert_eq!(secrets.get("dbboard.dup.token").expect("token"), "first");
    }

    #[test]
    fn add_rolls_back_secret_writes_when_the_toml_save_fails() {
        // We force `save_atomic` to fail by pointing the admin at a
        // path whose parent is an existing **file** (not a directory),
        // which makes `create_dir_all` reject creating that parent.
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"i am a file, not a dir").expect("seed blocker");
        let path = blocker.join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut admin = ConnectionAdmin {
            path,
            secrets: secrets.clone() as Arc<dyn SecretStore>,
            file: ConnectionFile::empty(),
        };

        let err = admin
            .add(d1_draft("rolled-back"))
            .expect_err("save must fail when parent is a file");
        assert!(
            matches!(err, ConfigError::Io(_)),
            "expected Io error, got {err:?}"
        );
        // The keyring rollback ran, so the orphan token is gone.
        assert!(matches!(
            secrets.get("dbboard.rolled-back.token"),
            Err(SecretError::NotFound(_))
        ));
        // The in-memory entry list is unchanged.
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn update_turso_changes_path_and_name() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Old", ":memory:"))
            .expect("add");

        admin
            .update(
                "local",
                ConnectionEditDraft {
                    name: "New".to_string(),
                    kind: ConnectionKindEditDraft::Turso {
                        path: "/tmp/x.db".to_string(),
                    },
                },
            )
            .expect("update");

        let entry = &admin.entries()[0];
        assert_eq!(entry.name, "New");
        assert_eq!(
            entry.kind,
            ConnectionKind::Turso {
                path: "/tmp/x.db".to_string(),
            }
        );
    }

    #[test]
    fn update_with_secret_keep_does_not_touch_the_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("prod")).expect("add");
        assert_eq!(secrets.get("dbboard.prod.token").expect("seeded"), "t0k3n");

        admin
            .update(
                "prod",
                ConnectionEditDraft {
                    name: "Renamed".to_string(),
                    kind: ConnectionKindEditDraft::D1 {
                        account_id: "acct".to_string(),
                        database_id: "db".to_string(),
                        base_url: Some("https://example.test".to_string()),
                        token: SecretField::Keep,
                    },
                },
            )
            .expect("update with keep");

        // The secret is unchanged.
        assert_eq!(secrets.get("dbboard.prod.token").expect("token"), "t0k3n");
        // But the TOML-side fields did change.
        match &admin.entries()[0].kind {
            ConnectionKind::D1 { base_url, .. } => {
                assert_eq!(base_url.as_deref(), Some("https://example.test"));
            }
            other => panic!("expected D1, got {other:?}"),
        }
        assert_eq!(admin.entries()[0].name, "Renamed");
    }

    #[test]
    fn update_with_secret_set_overwrites_the_keyring_entry() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("prod")).expect("add");

        admin
            .update(
                "prod",
                ConnectionEditDraft {
                    name: "D1 prod".to_string(),
                    kind: ConnectionKindEditDraft::D1 {
                        account_id: "acct".to_string(),
                        database_id: "db".to_string(),
                        base_url: None,
                        token: SecretField::Set("new-token".to_string()),
                    },
                },
            )
            .expect("update with set");

        assert_eq!(
            secrets.get("dbboard.prod.token").expect("token"),
            "new-token"
        );
    }

    #[test]
    fn update_unknown_id_returns_not_found() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let err = admin
            .update(
                "missing",
                ConnectionEditDraft {
                    name: "X".to_string(),
                    kind: ConnectionKindEditDraft::Turso {
                        path: ":memory:".to_string(),
                    },
                },
            )
            .expect_err("missing id must error");
        match &err {
            ConfigError::NotFound(id) => assert_eq!(id, "missing"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn update_with_mismatched_kind_is_rejected() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "L", ":memory:"))
            .expect("add");
        let err = admin
            .update(
                "local",
                ConnectionEditDraft {
                    name: "L".to_string(),
                    kind: ConnectionKindEditDraft::D1 {
                        account_id: "a".to_string(),
                        database_id: "b".to_string(),
                        base_url: None,
                        token: SecretField::Set("t".to_string()),
                    },
                },
            )
            .expect_err("kind change must be rejected");
        match &err {
            ConfigError::KindMismatch { id } => assert_eq!(id, "local"),
            other => panic!("expected KindMismatch, got {other:?}"),
        }
    }

    #[test]
    fn update_restores_old_secret_when_toml_save_fails() {
        // Add a D1 entry via a working admin first so the keyring is
        // seeded, then move the admin to a write-failing path before
        // attempting the update.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut admin =
            ConnectionAdmin::open(path, secrets.clone() as Arc<dyn SecretStore>).expect("open");
        admin.add(d1_draft("prod")).expect("seed");
        assert_eq!(secrets.get("dbboard.prod.token").expect("seeded"), "t0k3n");

        // Re-point the admin at a guaranteed-unwritable path.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"file-not-dir").expect("seed blocker");
        admin.path = blocker.join("connections.toml");

        let err = admin
            .update(
                "prod",
                ConnectionEditDraft {
                    name: "Renamed".to_string(),
                    kind: ConnectionKindEditDraft::D1 {
                        account_id: "acct".to_string(),
                        database_id: "db".to_string(),
                        base_url: None,
                        token: SecretField::Set("about-to-fail".to_string()),
                    },
                },
            )
            .expect_err("save must fail");
        assert!(
            matches!(err, ConfigError::Io(_)),
            "expected Io error, got {err:?}"
        );

        // The keyring is restored to the pre-update value.
        assert_eq!(secrets.get("dbboard.prod.token").expect("token"), "t0k3n");
        // The in-memory entry is also restored (we never replaced it).
        assert_eq!(admin.entries()[0].name, "D1 prod");
    }

    #[test]
    fn delete_removes_entry_and_purges_keyring() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("prod")).expect("add");
        assert_eq!(secrets.get("dbboard.prod.token").expect("seeded"), "t0k3n");

        admin.delete("prod").expect("delete");

        assert!(admin.entries().is_empty());
        assert!(matches!(
            secrets.get("dbboard.prod.token"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn delete_unknown_id_returns_not_found() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let err = admin.delete("missing").expect_err("missing id must error");
        match &err {
            ConfigError::NotFound(id) => assert_eq!(id, "missing"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn delete_succeeds_even_when_the_keyring_entry_is_already_gone() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("prod")).expect("add");
        // Simulate a keyring already cleared by some other process.
        secrets
            .delete("dbboard.prod.token")
            .expect("pre-clear keyring");

        admin.delete("prod").expect("delete must still succeed");
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn delete_turso_succeeds_with_no_keyring_traffic() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "L", ":memory:"))
            .expect("add");
        admin.delete("local").expect("delete");
        assert!(admin.entries().is_empty());
    }

    // --- Bundle export / import (ADR-0038 slice b) --------------------

    const BUNDLE_PASS: &str = "correct horse battery";

    /// Build a source admin holding a D1 + Supabase + Turso mix and return
    /// its encrypted bundle blob.
    fn source_bundle() -> Vec<u8> {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("store-a")).expect("add d1");
        admin
            .add(supabase_draft(
                "store-c",
                "postgres://postgres:pw@db.example.supabase.co/postgres",
            ))
            .expect("add supabase");
        admin
            .add(turso_draft("local", "Local", ":memory:"))
            .expect("add turso");
        admin.export_bundle(BUNDLE_PASS).expect("export")
    }

    #[test]
    fn export_then_import_into_empty_store_restores_entries_and_secrets() {
        let blob = source_bundle();

        let (_dir, secrets, mut target) = fresh_admin();
        let report = target.import_bundle(&blob, BUNDLE_PASS).expect("import");

        assert_eq!(report.imported, vec!["store-a", "store-c", "local"]);
        assert!(report.skipped.is_empty());
        assert_eq!(target.entries().len(), 3);
        // Secret-bearing entries are seeded into the target keychain.
        assert_eq!(
            secrets.get("dbboard.store-a.token").expect("token"),
            "t0k3n"
        );
        assert_eq!(
            secrets.get("dbboard.store-c.url").expect("url"),
            "postgres://postgres:pw@db.example.supabase.co/postgres"
        );
        // Turso carries no secret, so nothing is written for it.
        assert!(matches!(
            secrets.get("dbboard.local.token"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn import_skips_conflicting_ids_and_leaves_existing_secret_untouched() {
        let blob = source_bundle();

        // Target already has `store-a` with a *different* token.
        let (_dir, secrets, mut target) = fresh_admin();
        target
            .add(ConnectionDraft {
                id: "store-a".to_string(),
                name: "pre-existing".to_string(),
                kind: ConnectionKindDraft::D1 {
                    account_id: "acct".to_string(),
                    database_id: "db".to_string(),
                    base_url: None,
                    token: "local-token".to_string(),
                },
            })
            .expect("seed conflicting entry");

        let report = target.import_bundle(&blob, BUNDLE_PASS).expect("import");

        // The conflict is reported, the two fresh ids are imported.
        assert_eq!(report.skipped, vec!["store-a"]);
        assert_eq!(report.imported, vec!["store-c", "local"]);
        assert_eq!(target.entries().len(), 3);
        // The pre-existing secret was NOT overwritten by the bundle's.
        assert_eq!(
            secrets.get("dbboard.store-a.token").expect("token"),
            "local-token"
        );
        assert_eq!(target.entries()[0].name, "pre-existing");
    }

    #[test]
    fn import_of_an_all_conflict_bundle_imports_nothing() {
        let blob = source_bundle();

        // Re-import the same bundle into a store that already holds all
        // three ids (import it once, then again).
        let (_dir, _secrets, mut target) = fresh_admin();
        target
            .import_bundle(&blob, BUNDLE_PASS)
            .expect("first import");
        let report = target
            .import_bundle(&blob, BUNDLE_PASS)
            .expect("second import");

        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, vec!["store-a", "store-c", "local"]);
        assert_eq!(target.entries().len(), 3);
    }

    #[test]
    fn imported_entries_persist_to_disk() {
        let blob = source_bundle();

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut target = ConnectionAdmin::open(path.clone(), secrets as Arc<dyn SecretStore>)
            .expect("open target");
        target.import_bundle(&blob, BUNDLE_PASS).expect("import");

        // Re-open from disk: the imported metadata survived the TOML save.
        let reopen_secrets = Arc::new(InMemorySecretStore::new());
        let reopened =
            ConnectionAdmin::open(path, reopen_secrets as Arc<dyn SecretStore>).expect("reopen");
        assert_eq!(reopened.entries().len(), 3);
    }

    #[test]
    fn export_refuses_a_weak_passphrase() {
        let (_dir, _secrets, admin) = fresh_admin();
        let err = admin.export_bundle("short").expect_err("must refuse");
        assert!(matches!(err, ConfigError::Bundle(_)), "got {err:?}");
    }

    #[test]
    fn import_with_wrong_passphrase_is_a_bundle_error() {
        let blob = source_bundle();
        let (_dir, _secrets, mut target) = fresh_admin();
        let err = target
            .import_bundle(&blob, "the wrong passphrase")
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::Bundle(_)), "got {err:?}");
        // A failed import leaves the store empty.
        assert!(target.entries().is_empty());
    }

    #[test]
    fn import_of_garbage_bytes_is_a_bundle_error_not_a_panic() {
        let (_dir, _secrets, mut target) = fresh_admin();
        let err = target
            .import_bundle(b"not an age file", BUNDLE_PASS)
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::Bundle(_)), "got {err:?}");
    }

    #[test]
    fn export_fails_loudly_when_a_referenced_secret_is_missing() {
        // Seed an entry but then clear its secret so export cannot resolve
        // the reference — we must fail rather than ship an incomplete
        // bundle.
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(d1_draft("store-a")).expect("add");
        secrets
            .delete("dbboard.store-a.token")
            .expect("clear secret");
        let err = admin.export_bundle(BUNDLE_PASS).expect_err("must fail");
        assert!(matches!(err, ConfigError::Secret(_)), "got {err:?}");
    }
}
