//! Connection admin use-case (ADR-0016).
//!
//! Lives in `dbboard-config` because this crate already owns the TOML
//! surface ([`crate::store`]) and the keyring surface ([`crate::secrets`]).
//! Adding the use-case here avoids the presentation layer ever touching
//! the filesystem or the OS keychain directly — the UI layer holds a
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

use zeroize::Zeroize;

use crate::bundle::{decrypt_bundle, encrypt_bundle, validate_passphrase, BundlePayload};
use crate::dsn::{parse_dsn, with_password, DsnParts};
use crate::error::ConfigError;
use crate::secrets::{SecretError, SecretStore};
use crate::store::{
    load_or_empty, save_atomic, ConnectionEntry, ConnectionFile, ConnectionKind, SshTunnelToml,
};

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
    /// Optional SSH local-forward tunnel (ADR-0069). Cross-cutting: it fronts
    /// the connection regardless of `kind`, so it lives here beside `kind`
    /// rather than inside it. `add` rejects it for a kind that cannot tunnel
    /// ([`ConnectionKind::supports_ssh_tunnel`]).
    pub ssh: Option<SshTunnelDraft>,
    /// Whether the MCP server may write to this connection (ADR-0087).
    /// Cross-cutting like `ssh`, and `false` for anything that does not
    /// deliberately ask for it — the gate exists to be off by default.
    pub mcp_write: bool,
    /// Optional agent-facing name that hides this entry's `id` and `name` from
    /// `dbboard-mcp` (ADR-0088). `None` — the default — keeps both as they are.
    /// Trimmed, and a blank string is treated as `None`.
    pub mcp_alias: Option<String>,
}

/// Add-time SSH tunnel draft: bastion coordinates plus **inline** secrets (the
/// key passphrase or the SSH password). The add-path companion to the stored
/// [`SshTunnelToml`], whose secrets live behind `keyring_*_ref`s;
/// [`ConnectionAdmin::add`] derives those refs from the connection id and
/// routes the inline values through the [`SecretStore`].
#[derive(Debug, Clone)]
pub struct SshTunnelDraft {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuthDraft,
    pub host_key: SshHostKeyDraft,
}

/// Inline-secret SSH auth for an add draft.
#[derive(Debug, Clone)]
pub enum SshAuthDraft {
    /// Private-key auth. `passphrase` is `None` for an unencrypted key, or the
    /// inline passphrase secret to seed into the keyring.
    Key {
        key_path: String,
        passphrase: Option<String>,
    },
    /// Password auth; the inline password secret to seed into the keyring.
    Password(String),
}

/// Host-key verification policy chosen in the UI. A tunnel must verify the
/// server key, so there is no "accept any" — exactly one of these is set.
#[derive(Debug, Clone)]
pub enum SshHostKeyDraft {
    Fingerprint(String),
    KnownHosts(String),
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
    MySql {
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
    /// Firestore (ADR-0093). `service_account` is `None` for the local
    /// emulator, which authenticates with a fixed `Bearer owner` and therefore
    /// has no credential to seed into the keychain.
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        service_account: Option<String>,
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
    /// How the update should treat the entry's SSH tunnel (ADR-0069). Three
    /// states, because "no tunnel" and "don't touch the tunnel" are different:
    /// an editor that does not render the tunnel must be able to change a
    /// name without dropping it, so it sends [`SshEditField::Keep`].
    pub ssh: SshEditField,
    /// How the update should treat the MCP write gate (ADR-0087). `None`
    /// keeps whatever is stored, for the same reason [`SshEditField::Keep`]
    /// exists: the gate is normally set by hand in `connections.toml`, and a
    /// caller with no toggle — a rename, a URL rotation — must not revoke a
    /// permission it never showed the operator.
    pub mcp_write: Option<bool>,
    /// How the update should treat the agent-facing alias (ADR-0088). Same
    /// three states as `mcp_write`, expressed through one `Option`: `None`
    /// keeps whatever is stored, `Some(alias)` sets it, and `Some("")` — a
    /// blank or whitespace-only string, which is what an emptied text input
    /// sends — clears it.
    pub mcp_alias: Option<String>,
}

/// Top-level SSH edit intent. Distinct from a plain `Option` so a caller that
/// does not render the tunnel can leave it untouched rather than
/// silently removing it.
#[derive(Debug, Clone)]
pub enum SshEditField {
    /// Leave the stored tunnel (block and secrets) exactly as it is.
    Keep,
    /// Remove the tunnel; [`ConnectionAdmin::update`] purges its secrets.
    Disable,
    /// Replace the tunnel with this configuration.
    Set(SshTunnelEditDraft),
}

/// Edit-time SSH tunnel draft. Non-secret fields carry their new values
/// verbatim; only the passphrase / password distinguish "keep the stored
/// secret" from "overwrite it".
#[derive(Debug, Clone)]
pub struct SshTunnelEditDraft {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuthEditDraft,
    pub host_key: SshHostKeyDraft,
}

/// Edit-time SSH auth. Mirrors [`SshAuthDraft`] but the secrets are
/// keep-or-overwrite rather than always-inline.
#[derive(Debug, Clone)]
pub enum SshAuthEditDraft {
    Key {
        key_path: String,
        passphrase: SshPassphraseField,
    },
    Password(SecretField),
}

/// Three-state edit control for a key passphrase: a key may be unencrypted, so
/// unlike a mandatory secret it also has an explicit "no passphrase" state
/// distinct from "keep the stored one".
#[derive(Debug, Clone)]
pub enum SshPassphraseField {
    /// Reuse the stored passphrase (the key is encrypted and unchanged).
    Keep,
    /// Overwrite the passphrase with this new value.
    Set(String),
    /// The key is unencrypted; drop any stored passphrase.
    Unencrypted,
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
    MySql {
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
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        service_account: FirestoreCredentialField,
    },
}

/// Three-state edit control for a Firestore service account. Like
/// [`SshPassphraseField`], and for the same reason: "no credential" is a real,
/// reachable state — a connection pointed at the local emulator — distinct
/// from "keep the stored one". A mandatory [`SecretField`] could only express
/// the emulator as an empty string, which reads as a real credential and makes
/// a later `Keep` ambiguous.
#[derive(Debug, Clone)]
pub enum FirestoreCredentialField {
    /// Reuse whatever is stored (a service account, or nothing at all).
    Keep,
    /// Overwrite the stored service-account JSON with this new value.
    Set(String),
    /// Point at the emulator; drop any stored service account.
    Emulator,
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

    /// The non-secret parts of the DSN stored for `id`, for prefilling the
    /// edit form (ADR-0080).
    ///
    /// `Ok(None)` means "nothing to prefill": the kind stores no DSN (Turso,
    /// D1, Aurora DSQL IAM), the keychain entry is gone, or the stored value
    /// does not parse as a URL. Prefill is best-effort on purpose — a missing
    /// secret should open an empty form the user can retype, not block the
    /// edit dialog. The save path ([`ConnectionAdmin::dsn_with_stored_password`])
    /// is the strict half of the pair.
    ///
    /// The password is never part of the return value: [`DsnParts`] has no
    /// field for one.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NotFound`] if no entry has id `id`.
    pub fn dsn_prefill(&self, id: &str) -> Result<Option<DsnParts>, ConfigError> {
        let Some(key_ref) = self.dsn_key_ref(id)? else {
            return Ok(None);
        };
        let Ok(stored) = self.secrets.get(key_ref) else {
            return Ok(None);
        };
        Ok(parse_dsn(&stored))
    }

    /// `url` with the password from `id`'s stored DSN grafted back on
    /// (ADR-0080).
    ///
    /// This is the "leave the password blank to keep the stored one" path.
    /// The UI rebuilds the DSN from the parts it was shown — which never
    /// included the password — and the credential is re-attached here, inside
    /// the process that already holds it, so it never crosses into the webview.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::NotFound`] if no entry has id `id`, or the entry's
    ///   kind stores no DSN.
    /// - [`ConfigError::Secret`] if the keychain read fails.
    /// - [`ConfigError::DsnUnparseable`] if either the stored DSN or `url`
    ///   does not parse. Failing loudly beats saving the connection back
    ///   without its password.
    pub fn dsn_with_stored_password(&self, id: &str, url: &str) -> Result<String, ConfigError> {
        let key_ref = self
            .dsn_key_ref(id)?
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        let stored = self.secrets.get(key_ref)?;
        with_password(url, &stored)
            .ok_or_else(|| ConfigError::DsnUnparseable { id: id.to_string() })
    }

    /// The keyring reference holding `id`'s DSN, or `None` for a kind that
    /// stores no DSN.
    fn dsn_key_ref(&self, id: &str) -> Result<Option<&str>, ConfigError> {
        let entry = self
            .file
            .connections
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        Ok(match &entry.kind {
            ConnectionKind::Postgres { keyring_url_ref }
            | ConnectionKind::MySql { keyring_url_ref }
            | ConnectionKind::Neon { keyring_url_ref }
            | ConnectionKind::Supabase { keyring_url_ref }
            | ConnectionKind::AuroraDsql { keyring_url_ref } => Some(keyring_url_ref.as_str()),
            _ => None,
        })
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
    /// - [`ConfigError::DuplicateAlias`] if `draft.mcp_alias`, or `draft.id`
    ///   itself, collides with another entry's alias (ADR-0088).
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

        // Both the id and the alias are handles an agent may hand back, and the
        // resolver tries aliases before ids — so neither may shadow an existing
        // entry's alias (ADR-0088). Checked before any secret is written.
        let mcp_alias = normalize_alias(draft.mcp_alias);
        self.ensure_handle_is_free(&draft.id, &draft.id)?;
        if let Some(alias) = mcp_alias.as_deref() {
            self.ensure_handle_is_free(alias, &draft.id)?;
        }

        let (kind, mut secret_writes) = build_kind_for_add(&draft.id, draft.kind);

        // Reject a tunnel on a kind that cannot forward a TCP port before any
        // secret is written, so a bad combo costs nothing.
        let ssh = match draft.ssh {
            Some(_) if !kind.supports_ssh_tunnel() => {
                return Err(ConfigError::SshUnsupportedKind {
                    id: draft.id,
                    kind: kind.adapter_label(),
                });
            }
            Some(ssh) => {
                let (toml, mut writes) = build_ssh_for_add(&draft.id, ssh);
                secret_writes.append(&mut writes);
                Some(toml)
            }
            None => None,
        };

        // A connection can now carry two secrets (kind url/token + ssh
        // passphrase/password); if the second write fails, roll the first back
        // so no orphan keyring entry survives a partial add.
        let mut written: Vec<&PendingSecretWrite> = Vec::new();
        for write in &secret_writes {
            if let Err(err) = self.secrets.set(&write.key_ref, &write.value) {
                for done in &written {
                    let _ = self.secrets.delete(&done.key_ref);
                }
                return Err(ConfigError::Secret(err));
            }
            written.push(write);
        }

        let new_entry = ConnectionEntry {
            mcp_write: draft.mcp_write,
            mcp_alias,
            ssh,
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

        let existing = self.file.connections[idx].clone();

        // `None` keeps the stored alias, a blank string clears it (ADR-0088).
        // Resolved and checked before any keyring write, so a collision costs
        // nothing to roll back.
        let mcp_alias = match draft.mcp_alias {
            None => existing.mcp_alias.clone(),
            Some(raw) => normalize_alias(Some(raw)),
        };
        if let Some(alias) = mcp_alias.as_deref() {
            self.ensure_handle_is_free(alias, id)?;
        }

        let (new_kind, mut applied_writes) =
            self.apply_update_kind(id, &existing.kind, draft.kind)?;

        // Kind can't change on update, so a tunnel is legal here iff the
        // existing kind can forward a TCP port. Only a `Set` introduces a new
        // tunnel; Keep/Disable are always fine. Reject before writing any ssh
        // secret, restoring the kind writes already applied above.
        if matches!(draft.ssh, SshEditField::Set(_)) && !new_kind.supports_ssh_tunnel() {
            self.restore_applied(&applied_writes);
            return Err(ConfigError::SshUnsupportedKind {
                id: id.to_string(),
                kind: new_kind.adapter_label(),
            });
        }

        let new_ssh = match self.apply_update_ssh(
            id,
            draft.ssh,
            existing.ssh.as_ref(),
            &mut applied_writes,
        ) {
            Ok(ssh) => ssh,
            Err(err) => {
                self.restore_applied(&applied_writes);
                return Err(err);
            }
        };

        let new_entry = ConnectionEntry {
            mcp_write: draft.mcp_write.unwrap_or(existing.mcp_write),
            mcp_alias,
            ssh: new_ssh,
            id: id.to_string(),
            name: draft.name,
            kind: new_kind,
        };

        let mut new_file = self.file.clone();
        new_file.connections[idx] = new_entry;

        if let Err(err) = save_atomic(&self.path, &new_file) {
            self.restore_applied(&applied_writes);
            return Err(err);
        }

        self.file = new_file;

        // Best-effort purge of secrets the old entry referenced but the new one
        // no longer does — the tunnel was removed, auth switched
        // key<->password, or a Firestore connection was pointed back at the
        // emulator. Mirrors `delete`: once unreferenced, an orphan secret is
        // harmless and a purge failure must not fail an otherwise-saved update.
        self.purge_orphaned_secrets(&existing, &self.file.connections[idx]);

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
        for key_ref in entry_keyring_refs(&removed) {
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
            for key_ref in entry_keyring_refs(entry) {
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
    /// Import is **additive and conflict-safe**: an incoming id that
    /// already exists (or that the bundle lists twice) is skipped and
    /// reported, never overwritten — the user's current secrets and
    /// metadata are the source of truth. An incoming entry whose
    /// `keyring_*_ref` points at a keychain slot **another** connection
    /// already owns is also skipped: `keyring_*_ref` is free-form JSON in
    /// the bundle, so a crafted bundle could otherwise carry a brand-new id
    /// but a ref aimed at an existing connection's slot and silently
    /// overwrite that connection's live secret (ADR-0038 threat model).
    /// Newly-added entries seed their secrets into the keychain first, then
    /// the TOML is persisted; on a TOML-write failure the just-seeded
    /// secrets are rolled back so no orphan keyring entry survives, exactly
    /// as [`ConnectionAdmin::add`] does.
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
        let mut payload = decrypt_bundle(blob, passphrase)?;
        // Take the incoming entries out of the payload so we can iterate
        // them by value while still borrowing `payload.secrets` below.
        // `payload` implements `Drop` (it zeroizes its secret values), so
        // it cannot be partially moved out of — `mem::take` leaves an empty
        // vec behind and the payload still scrubs its secrets on drop.
        let incoming = std::mem::take(&mut payload.connections.connections);

        // Ids we must not clobber: everything already in the store, plus
        // anything we accept earlier in this same bundle (so a bundle that
        // lists an id twice skips the second occurrence rather than
        // creating a duplicate entry).
        let mut seen: HashSet<String> =
            self.file.connections.iter().map(|e| e.id.clone()).collect();
        // Keyring refs already owned by an existing entry (or claimed by an
        // entry accepted earlier in this bundle). Guards the secret store
        // against a bundle whose ref aims at someone else's slot.
        let mut claimed_refs: HashSet<String> = self
            .file
            .connections
            .iter()
            .flat_map(entry_keyring_refs)
            .collect();

        let mut report = ImportReport::default();
        let mut to_add: Vec<ConnectionEntry> = Vec::new();
        let mut secret_writes: Vec<(String, String)> = Vec::new();

        for entry in incoming {
            if seen.contains(&entry.id) {
                report.skipped.push(entry.id);
                continue;
            }
            let refs = entry_keyring_refs(&entry);
            if refs.iter().any(|r| claimed_refs.contains(r)) {
                // Ref collides with a slot another connection owns; refuse
                // rather than overwrite that connection's secret.
                report.skipped.push(entry.id);
                continue;
            }
            for key_ref in refs {
                // A well-formed dbboard bundle carries every secret it
                // references; if one is absent we still import the entry's
                // metadata (the user can re-enter the secret via edit)
                // rather than dropping the whole connection.
                if let Some(value) = payload.secrets.get(&key_ref) {
                    secret_writes.push((key_ref.clone(), value.clone()));
                }
                claimed_refs.insert(key_ref);
            }
            seen.insert(entry.id.clone());
            report.imported.push(entry.id.clone());
            to_add.push(entry);
        }

        if to_add.is_empty() {
            return Ok(report);
        }

        // Seed secrets first (same order as `add`); track what we wrote so
        // a later failure can undo it. Each cloned secret value is scrubbed
        // as soon as it has been handed to the keychain (ADR-0038).
        let mut written: Vec<String> = Vec::new();
        for i in 0..secret_writes.len() {
            let (key_ref, value) = &secret_writes[i];
            if let Err(err) = self.secrets.set(key_ref, value) {
                self.rollback_secret_writes(&written);
                zeroize_secret_writes(&mut secret_writes);
                return Err(ConfigError::Secret(err));
            }
            written.push(key_ref.clone());
        }
        zeroize_secret_writes(&mut secret_writes);

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

    /// Reject `candidate` as an agent-facing handle if some *other* entry
    /// already answers to it, by alias or by id (ADR-0088).
    ///
    /// `owner` is the id of the entry the candidate belongs to, so an entry
    /// re-sending its own alias — which a form does on every save — and an
    /// alias equal to the entry's own id are both fine.
    fn ensure_handle_is_free(&self, candidate: &str, owner: &str) -> Result<(), ConfigError> {
        let taken = self.file.connections.iter().any(|entry| {
            entry.id != owner
                && (entry.id == candidate || entry.mcp_alias.as_deref() == Some(candidate))
        });
        if taken {
            return Err(ConfigError::DuplicateAlias(candidate.to_string()));
        }
        Ok(())
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
            (ConnectionKind::MySql { keyring_url_ref }, ConnectionKindEditDraft::MySql { url }) => {
                if let SecretField::Set(new_value) = url {
                    self.apply_secret_write(keyring_url_ref, &new_value, &mut applied)?;
                }
                ConnectionKind::MySql {
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
            (
                ConnectionKind::Firestore {
                    keyring_service_account_ref,
                    ..
                },
                draft @ ConnectionKindEditDraft::Firestore { .. },
            ) => self.apply_firestore_edit(
                id,
                keyring_service_account_ref.as_deref(),
                draft,
                &mut applied,
            )?,
            (_, _) => {
                return Err(ConfigError::KindMismatch { id: id.to_string() });
            }
        };

        Ok((new_kind, applied))
    }

    /// The Firestore arm of [`Self::apply_update_kind`], lifted out because it
    /// is the only kind whose credential has three states rather than two
    /// (ADR-0094) and so does not fit the one-line shape of the others.
    fn apply_firestore_edit(
        &self,
        id: &str,
        existing_ref: Option<&str>,
        draft: ConnectionKindEditDraft,
        applied: &mut Vec<AppliedSecretWrite>,
    ) -> Result<ConnectionKind, ConfigError> {
        let ConnectionKindEditDraft::Firestore {
            project_id,
            database_id,
            base_url,
            service_account,
        } = draft
        else {
            return Err(ConfigError::KindMismatch { id: id.to_string() });
        };

        let keyring_service_account_ref = match service_account {
            // Unlike an SSH "keep" with nothing stored, this is not an error: a
            // Firestore connection with no credential is the emulator, so
            // keeping "nothing" keeps a valid state.
            FirestoreCredentialField::Keep => existing_ref.map(ToOwned::to_owned),
            FirestoreCredentialField::Set(new_value) => {
                // An emulator connection has no ref yet, so derive one the same
                // way `add` does rather than failing.
                let key_ref = existing_ref.map_or_else(
                    || keyring_ref(id, FIRESTORE_SERVICE_ACCOUNT_FIELD),
                    ToOwned::to_owned,
                );
                self.apply_secret_write(&key_ref, &new_value, applied)?;
                Some(key_ref)
            }
            // The post-save purge deletes the now-unreferenced secret.
            FirestoreCredentialField::Emulator => None,
        };

        Ok(ConnectionKind::Firestore {
            project_id,
            database_id,
            base_url,
            keyring_service_account_ref,
        })
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

    /// Undo the keyring writes recorded in `applied` (restoring the previous
    /// value, or deleting the entry if the keyring was empty before). Used on
    /// every failure path after some secrets have already been written.
    fn restore_applied(&self, applied: &[AppliedSecretWrite]) {
        for write in applied {
            let _ = match &write.old_value {
                Some(old) => self.secrets.set(&write.key_ref, old),
                None => self.secrets.delete(&write.key_ref),
            };
        }
    }

    /// Delete the keyring secrets `old` referenced that `new` no longer does.
    /// Best-effort: an unreferenced secret is harmless (the TOML is the source
    /// of truth) so a delete failure is ignored.
    ///
    /// Covers the kind's own secret as well as the tunnel's, because a kind can
    /// stop referencing one without being deleted: a Firestore connection moved
    /// back to the emulator drops its service-account ref, and the credential
    /// behind it would otherwise outlive every pointer to it.
    fn purge_orphaned_secrets(&self, old: &ConnectionEntry, new: &ConnectionEntry) {
        let kept: HashSet<String> = entry_keyring_refs(new).into_iter().collect();
        for old_ref in entry_keyring_refs(old) {
            if !kept.contains(&old_ref) {
                let _ = self.secrets.delete(&old_ref);
            }
        }
    }

    /// Resolve an [`SshEditField`] into the entry's new tunnel block.
    ///
    /// `Keep` returns the existing block untouched (no keyring write), so an
    /// editor with no tunnel UI never disturbs a stored tunnel; `Disable`
    /// returns `None` (the caller purges the orphaned secrets); `Set` builds
    /// the block, writing any overwritten passphrase/password to the keyring
    /// (recorded in `applied` for rollback). A
    /// [`SecretField::Keep`]/[`SshPassphraseField::Keep`] reuses the secret the
    /// *existing* block already points at — resolved from `existing`, not
    /// re-derived from the id — so a "keep" with nothing stored to keep (e.g.
    /// switching auth method, or marking a previously-unencrypted key encrypted
    /// while leaving the passphrase blank) is rejected rather than persisting a
    /// ref to a keyring entry that was never written (ADR-0069 / ADR-0016).
    fn apply_update_ssh(
        &self,
        id: &str,
        field: SshEditField,
        existing: Option<&SshTunnelToml>,
        applied: &mut Vec<AppliedSecretWrite>,
    ) -> Result<Option<SshTunnelToml>, ConfigError> {
        let draft = match field {
            SshEditField::Keep => return Ok(existing.cloned()),
            SshEditField::Disable => return Ok(None),
            SshEditField::Set(draft) => draft,
        };

        let (key_path, keyring_passphrase_ref, keyring_password_ref) = match draft.auth {
            SshAuthEditDraft::Key {
                key_path,
                passphrase,
            } => {
                let pass_ref = match passphrase {
                    SshPassphraseField::Unencrypted => None,
                    SshPassphraseField::Keep => {
                        match existing.and_then(|e| e.keyring_passphrase_ref.clone()) {
                            Some(existing_ref) => Some(existing_ref),
                            None => {
                                return Err(ConfigError::SshInvalid {
                                    id: id.to_string(),
                                    reason: "the SSH key is marked encrypted but no \
                                             passphrase was provided and none is stored to keep"
                                        .to_string(),
                                })
                            }
                        }
                    }
                    SshPassphraseField::Set(value) => {
                        let key_ref = keyring_ref(id, SSH_PASSPHRASE_FIELD);
                        self.apply_secret_write(&key_ref, &value, applied)?;
                        Some(key_ref)
                    }
                };
                (Some(key_path), pass_ref, None)
            }
            SshAuthEditDraft::Password(field) => {
                let pass_ref = match field {
                    SecretField::Set(value) => {
                        let key_ref = keyring_ref(id, SSH_PASSWORD_FIELD);
                        self.apply_secret_write(&key_ref, &value, applied)?;
                        key_ref
                    }
                    SecretField::Keep => {
                        match existing.and_then(|e| e.keyring_password_ref.clone()) {
                            Some(existing_ref) => existing_ref,
                            None => {
                                return Err(ConfigError::SshInvalid {
                                    id: id.to_string(),
                                    reason: "switching to SSH password auth requires a password"
                                        .to_string(),
                                })
                            }
                        }
                    }
                };
                (None, None, Some(pass_ref))
            }
        };

        let (fingerprint, known_hosts) = split_host_key(draft.host_key);

        let tunnel = SshTunnelToml {
            host: draft.host,
            port: draft.port,
            user: draft.user,
            key_path,
            keyring_passphrase_ref,
            keyring_password_ref,
            fingerprint,
            known_hosts,
        };
        // Belt-and-suspenders: the tagged draft enums make an invalid combination
        // (two auth methods, a passphrase ref without a key, no host-key policy)
        // unrepresentable, but re-validate before it can reach disk so any future
        // drift in the draft types fails here rather than at the next load.
        tunnel
            .validate()
            .map_err(|reason| ConfigError::SshInvalid {
                id: id.to_string(),
                reason,
            })?;
        Ok(Some(tunnel))
    }
}

/// Normalise an alias draft: trim it, and treat blank as absent (ADR-0088).
///
/// A cleared text input sends `Some("")`, which is the only way a form can say
/// "no alias" — and a stored alias of `"  "` would be a handle nobody can type.
fn normalize_alias(raw: Option<String>) -> Option<String> {
    raw.map(|alias| alias.trim().to_string())
        .filter(|alias| !alias.is_empty())
}

/// Compute the keyring ref for a given connection id and field.
fn keyring_ref(id: &str, field: &str) -> String {
    format!("dbboard.{id}.{field}")
}

/// Keyring field names for the two SSH secrets. Kept as consts so the add and
/// update paths derive the exact same ref for a given id.
const SSH_PASSPHRASE_FIELD: &str = "ssh_passphrase";
const SSH_PASSWORD_FIELD: &str = "ssh_password";

/// Keyring field name for a Firestore service account, for the same reason as
/// the SSH consts above: `add` and `update` must derive the identical ref.
const FIRESTORE_SERVICE_ACCOUNT_FIELD: &str = "service_account";

/// Split a host-key draft into the `(fingerprint, known_hosts)` pair
/// [`SshTunnelToml`] stores — exactly one is `Some`.
fn split_host_key(host_key: SshHostKeyDraft) -> (Option<String>, Option<String>) {
    match host_key {
        SshHostKeyDraft::Fingerprint(fingerprint) => (Some(fingerprint), None),
        SshHostKeyDraft::KnownHosts(path) => (None, Some(path)),
    }
}

/// Build the stored [`SshTunnelToml`] for an `add`, deriving keyring refs from
/// the id and returning the inline secrets as pending writes.
fn build_ssh_for_add(id: &str, draft: SshTunnelDraft) -> (SshTunnelToml, Vec<PendingSecretWrite>) {
    let mut writes = Vec::new();
    let (key_path, keyring_passphrase_ref, keyring_password_ref) = match draft.auth {
        SshAuthDraft::Key {
            key_path,
            passphrase,
        } => {
            let pass_ref = passphrase.map(|value| {
                let key_ref = keyring_ref(id, SSH_PASSPHRASE_FIELD);
                writes.push(PendingSecretWrite {
                    key_ref: key_ref.clone(),
                    value,
                });
                key_ref
            });
            (Some(key_path), pass_ref, None)
        }
        SshAuthDraft::Password(value) => {
            let key_ref = keyring_ref(id, SSH_PASSWORD_FIELD);
            writes.push(PendingSecretWrite {
                key_ref: key_ref.clone(),
                value,
            });
            (None, None, Some(key_ref))
        }
    };
    let (fingerprint, known_hosts) = split_host_key(draft.host_key);
    let toml = SshTunnelToml {
        host: draft.host,
        port: draft.port,
        user: draft.user,
        key_path,
        keyring_passphrase_ref,
        keyring_password_ref,
        fingerprint,
        known_hosts,
    };
    (toml, writes)
}

/// Every keyring ref an entry owns — its kind's secret plus any SSH tunnel
/// secret. The bundle export/import and delete paths use this so a tunneled
/// connection carries (and cleans up) its ssh secret too.
fn entry_keyring_refs(entry: &ConnectionEntry) -> Vec<String> {
    let mut refs = keyring_refs_in(&entry.kind);
    if let Some(ssh) = &entry.ssh {
        refs.extend(ssh.keyring_refs().into_iter().map(str::to_string));
    }
    refs
}

/// Scrub the plaintext secret values held in an import's pending-write
/// buffer (ADR-0038). The keys are non-secret keyring refs; only the
/// values carry secret material, so only they are zeroized.
fn zeroize_secret_writes(writes: &mut [(String, String)]) {
    for (_key_ref, value) in writes.iter_mut() {
        value.zeroize();
    }
}

/// Enumerate every keyring ref that a given [`ConnectionKind`] points
/// at. `Turso` has none; `D1`, `Postgres`, `MySql`, `Neon`, `Supabase`,
/// and `AuroraDsql` each carry exactly one; `AuroraDsqlIam` carries its
/// AWS secret-key ref (its other fields are non-secret and live inline);
/// `Firestore` carries one only when it is not pointed at the emulator.
fn keyring_refs_in(kind: &ConnectionKind) -> Vec<String> {
    match kind {
        ConnectionKind::Turso { .. } => Vec::new(),
        ConnectionKind::D1 {
            keyring_token_ref, ..
        } => vec![keyring_token_ref.clone()],
        ConnectionKind::Postgres { keyring_url_ref }
        | ConnectionKind::MySql { keyring_url_ref }
        | ConnectionKind::Neon { keyring_url_ref }
        | ConnectionKind::Supabase { keyring_url_ref }
        | ConnectionKind::AuroraDsql { keyring_url_ref } => {
            vec![keyring_url_ref.clone()]
        }
        ConnectionKind::AuroraDsqlIam {
            keyring_secret_key_ref,
            ..
        } => vec![keyring_secret_key_ref.clone()],
        ConnectionKind::Firestore {
            keyring_service_account_ref,
            ..
        } => keyring_service_account_ref.clone().into_iter().collect(),
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
        ConnectionKindDraft::MySql { url } => {
            let url_ref = keyring_ref(id, "url");
            let kind = ConnectionKind::MySql {
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
        ConnectionKindDraft::Firestore {
            project_id,
            database_id,
            base_url,
            service_account,
        } => {
            // No service account means the emulator, which has no credential
            // to store — so no ref is minted and no keychain entry is written.
            let mut writes = Vec::new();
            let keyring_service_account_ref = service_account.map(|value| {
                let key_ref = keyring_ref(id, FIRESTORE_SERVICE_ACCOUNT_FIELD);
                writes.push(PendingSecretWrite {
                    key_ref: key_ref.clone(),
                    value,
                });
                key_ref
            });
            let kind = ConnectionKind::Firestore {
                project_id,
                database_id,
                base_url,
                keyring_service_account_ref,
            };
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
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: name.to_string(),
            kind: ConnectionKindDraft::Turso {
                path: path.to_string(),
            },
        }
    }

    fn d1_draft(id: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
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
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("PG {id}"),
            kind: ConnectionKindDraft::Postgres {
                url: url.to_string(),
            },
        }
    }

    fn mysql_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("MySQL {id}"),
            kind: ConnectionKindDraft::MySql {
                url: url.to_string(),
            },
        }
    }

    fn neon_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("Neon {id}"),
            kind: ConnectionKindDraft::Neon {
                url: url.to_string(),
            },
        }
    }

    fn supabase_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("Supabase {id}"),
            kind: ConnectionKindDraft::Supabase {
                url: url.to_string(),
            },
        }
    }

    fn aurora_dsql_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("Aurora DSQL {id}"),
            kind: ConnectionKindDraft::AuroraDsql {
                url: url.to_string(),
            },
        }
    }

    /// `service_account: None` is the emulator: no credential exists to store.
    fn firestore_draft(id: &str, service_account: Option<&str>) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("Firestore {id}"),
            kind: ConnectionKindDraft::Firestore {
                project_id: "example-project".to_string(),
                database_id: None,
                base_url: None,
                service_account: service_account.map(str::to_string),
            },
        }
    }

    fn firestore_edit(
        name: &str,
        service_account: FirestoreCredentialField,
    ) -> ConnectionEditDraft {
        ConnectionEditDraft {
            name: name.to_string(),
            kind: ConnectionKindEditDraft::Firestore {
                project_id: "example-project".to_string(),
                database_id: None,
                base_url: None,
                service_account,
            },
            ssh: SshEditField::Keep,
            mcp_write: None,
            mcp_alias: None,
        }
    }

    fn firestore_service_account_ref(entry: &ConnectionEntry) -> Option<&str> {
        match &entry.kind {
            ConnectionKind::Firestore {
                keyring_service_account_ref,
                ..
            } => keyring_service_account_ref.as_deref(),
            other => panic!("expected Firestore, got {other:?}"),
        }
    }

    #[test]
    fn add_firestore_routes_the_service_account_through_the_secret_store() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(firestore_draft(
                "fs-prod",
                Some(r#"{"type":"service_account"}"#),
            ))
            .expect("add firestore");
        assert_eq!(
            firestore_service_account_ref(&admin.entries()[0]),
            Some("dbboard.fs-prod.service_account")
        );
        assert_eq!(
            secrets
                .get("dbboard.fs-prod.service_account")
                .expect("service account"),
            r#"{"type":"service_account"}"#
        );
    }

    /// An emulator connection has no credential — not a blank one. Writing an
    /// empty secret would leave a keychain entry that reads as real, and would
    /// make `Keep` on a later edit ambiguous.
    #[test]
    fn add_firestore_for_the_emulator_stores_no_credential_at_all() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(firestore_draft("fs-local", None))
            .expect("add firestore emulator");
        assert_eq!(firestore_service_account_ref(&admin.entries()[0]), None);
        assert!(matches!(
            secrets.get("dbboard.fs-local.service_account"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn update_firestore_can_promote_an_emulator_connection_to_a_service_account() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(firestore_draft("fs", None))
            .expect("add firestore emulator");
        admin
            .update(
                "fs",
                firestore_edit(
                    "Firestore (prod)",
                    FirestoreCredentialField::Set(r#"{"type":"service_account"}"#.to_string()),
                ),
            )
            .expect("promote to service account");
        assert_eq!(
            firestore_service_account_ref(&admin.entries()[0]),
            Some("dbboard.fs.service_account")
        );
        assert_eq!(
            secrets
                .get("dbboard.fs.service_account")
                .expect("service account"),
            r#"{"type":"service_account"}"#
        );
    }

    /// Switching back to the emulator drops the reference, so the stored
    /// credential must go too — otherwise a service-account key outlives every
    /// pointer to it, invisible in both the TOML and the UI.
    #[test]
    fn update_firestore_switching_to_the_emulator_purges_the_stored_credential() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(firestore_draft("fs", Some(r#"{"type":"service_account"}"#)))
            .expect("add firestore");
        admin
            .update(
                "fs",
                firestore_edit("Firestore (emulator)", FirestoreCredentialField::Emulator),
            )
            .expect("demote to emulator");
        assert_eq!(firestore_service_account_ref(&admin.entries()[0]), None);
        assert!(matches!(
            secrets.get("dbboard.fs.service_account"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn update_firestore_keep_leaves_the_stored_credential_untouched() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(firestore_draft("fs", Some(r#"{"type":"service_account"}"#)))
            .expect("add firestore");
        admin
            .update(
                "fs",
                firestore_edit("Renamed", FirestoreCredentialField::Keep),
            )
            .expect("rename only");
        assert_eq!(admin.entries()[0].name, "Renamed");
        assert_eq!(
            firestore_service_account_ref(&admin.entries()[0]),
            Some("dbboard.fs.service_account")
        );
        assert_eq!(
            secrets
                .get("dbboard.fs.service_account")
                .expect("service account"),
            r#"{"type":"service_account"}"#
        );
    }

    #[test]
    fn delete_firestore_purges_the_service_account_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(firestore_draft("fs", Some(r#"{"type":"service_account"}"#)))
            .expect("add firestore");
        admin.delete("fs").expect("delete");
        assert!(matches!(
            secrets.get("dbboard.fs.service_account"),
            Err(SecretError::NotFound(_))
        ));
    }

    /// Firestore is an HTTPS API; a tunnel on it is a configuration error, and
    /// the add path must reject it before any secret is written.
    #[test]
    fn add_firestore_with_an_ssh_tunnel_is_rejected() {
        let (_dir, secrets, mut admin) = fresh_admin();
        let mut draft = firestore_draft("fs", Some(r#"{"type":"service_account"}"#));
        draft.ssh = Some(SshTunnelDraft {
            host: "bastion.example.com".to_string(),
            port: 22,
            user: "ops".to_string(),
            auth: SshAuthDraft::Password("pw".to_string()),
            host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
        });
        assert!(matches!(
            admin.add(draft),
            Err(ConfigError::SshUnsupportedKind { .. })
        ));
        assert!(matches!(
            secrets.get("dbboard.fs.service_account"),
            Err(SecretError::NotFound(_))
        ));
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
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
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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

    // The MCP write gate (ADR-0087) is a permission, so the paths that could
    // silently grant or revoke it are worth pinning down.

    #[test]
    fn add_defaults_the_mcp_write_gate_to_closed() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(turso_draft("local", "Local", ":memory:"))
            .expect("add");
        assert!(!entry.mcp_write, "a new connection must not be writable");
    }

    #[test]
    fn add_can_open_the_mcp_write_gate() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(ConnectionDraft {
                mcp_write: true,
                ..turso_draft("local", "Local", ":memory:")
            })
            .expect("add");
        assert!(entry.mcp_write);
    }

    /// An edit that says nothing about the gate must leave it alone. The
    /// gate is normally set by hand in `connections.toml`, so a caller with
    /// no toggle — renaming a connection, rotating a URL — would otherwise
    /// revoke a permission it never showed the user.
    #[test]
    fn update_without_an_opinion_keeps_the_mcp_write_gate() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(ConnectionDraft {
                mcp_write: true,
                ..turso_draft("local", "Old", ":memory:")
            })
            .expect("add");

        admin
            .update(
                "local",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
                    name: "New".to_string(),
                    kind: ConnectionKindEditDraft::Turso {
                        path: ":memory:".to_string(),
                    },
                },
            )
            .expect("update");

        assert!(admin.entries()[0].mcp_write, "rename must not revoke");
    }

    #[test]
    fn update_can_open_and_close_the_mcp_write_gate() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Local", ":memory:"))
            .expect("add");

        let edit = |gate: Option<bool>| ConnectionEditDraft {
            mcp_alias: None,
            mcp_write: gate,
            ssh: SshEditField::Keep,
            name: "Local".to_string(),
            kind: ConnectionKindEditDraft::Turso {
                path: ":memory:".to_string(),
            },
        };

        admin.update("local", edit(Some(true))).expect("open");
        assert!(admin.entries()[0].mcp_write);

        admin.update("local", edit(Some(false))).expect("close");
        assert!(!admin.entries()[0].mcp_write);
    }

    // The MCP alias (ADR-0088) is what an agent sees instead of the id. It has
    // to be unambiguous — the agent hands it back as a handle — and it must
    // survive an edit that does not mention it, for the same reason the write
    // gate does.

    fn alias_edit(alias: Option<String>) -> ConnectionEditDraft {
        ConnectionEditDraft {
            mcp_alias: alias,
            mcp_write: None,
            ssh: SshEditField::Keep,
            name: "Local".to_string(),
            kind: ConnectionKindEditDraft::Turso {
                path: ":memory:".to_string(),
            },
        }
    }

    #[test]
    fn add_defaults_to_no_mcp_alias() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(turso_draft("local", "Local", ":memory:"))
            .expect("add");
        assert_eq!(entry.mcp_alias, None);
    }

    #[test]
    fn add_can_set_an_mcp_alias() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("app@db.internal", "Local", ":memory:")
            })
            .expect("add");
        assert_eq!(entry.mcp_alias.as_deref(), Some("shop-db"));
    }

    #[test]
    fn update_without_an_opinion_keeps_the_mcp_alias() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("local", "Old", ":memory:")
            })
            .expect("add");

        admin.update("local", alias_edit(None)).expect("update");

        assert_eq!(
            admin.entries()[0].mcp_alias.as_deref(),
            Some("shop-db"),
            "a rename must not expose the id to agents again"
        );
    }

    #[test]
    fn update_can_set_and_clear_the_mcp_alias() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Local", ":memory:"))
            .expect("add");

        admin
            .update("local", alias_edit(Some("shop-db".to_string())))
            .expect("set");
        assert_eq!(admin.entries()[0].mcp_alias.as_deref(), Some("shop-db"));

        // Blank is how a form says "no alias" — it has no other way to.
        admin
            .update("local", alias_edit(Some("  ".to_string())))
            .expect("clear");
        assert_eq!(admin.entries()[0].mcp_alias, None);
    }

    #[test]
    fn an_alias_is_trimmed() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(ConnectionDraft {
                mcp_alias: Some("  shop-db \n".to_string()),
                ..turso_draft("local", "Local", ":memory:")
            })
            .expect("add");
        assert_eq!(entry.mcp_alias.as_deref(), Some("shop-db"));
    }

    #[test]
    fn an_alias_may_not_collide_with_another_alias() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("one", "One", ":memory:")
            })
            .expect("add");

        let err = admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("two", "Two", ":memory:")
            })
            .expect_err("a duplicate alias is ambiguous as a handle");
        assert!(
            matches!(err, ConfigError::DuplicateAlias(ref a) if a == "shop-db"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn an_alias_may_not_collide_with_another_connections_id() {
        // Resolution accepts a plain id for a connection with no alias, so an
        // alias equal to some other entry's id would make the handle
        // ambiguous — and would let an agent reach a connection it was not
        // shown.
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("staging", "Staging", ":memory:"))
            .expect("add");

        let err = admin
            .add(ConnectionDraft {
                mcp_alias: Some("staging".to_string()),
                ..turso_draft("prod", "Prod", ":memory:")
            })
            .expect_err("an alias that shadows another id is ambiguous");
        assert!(
            matches!(err, ConfigError::DuplicateAlias(ref a) if a == "staging"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn an_alias_matching_the_entrys_own_id_is_allowed() {
        // Not ambiguous, and it is the honest way to say "the id is fine to
        // show" — a connection called `local` has nothing to hide.
        let (_dir, _secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(ConnectionDraft {
                mcp_alias: Some("local".to_string()),
                ..turso_draft("local", "Local", ":memory:")
            })
            .expect("add");
        assert_eq!(entry.mcp_alias.as_deref(), Some("local"));
    }

    #[test]
    fn a_new_id_may_not_collide_with_an_existing_alias() {
        // The mirror of the previous case. Resolution tries aliases first, so
        // an id that shadows one would silently route an agent's handle to the
        // wrong database.
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("one", "One", ":memory:")
            })
            .expect("add");

        let err = admin
            .add(turso_draft("shop-db", "Two", ":memory:"))
            .expect_err("an id that shadows an alias is ambiguous");
        assert!(
            matches!(err, ConfigError::DuplicateAlias(ref a) if a == "shop-db"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn update_may_not_take_an_alias_already_in_use() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("one", "One", ":memory:")
            })
            .expect("add");
        admin
            .add(turso_draft("local", "Local", ":memory:"))
            .expect("add");

        let err = admin
            .update("local", alias_edit(Some("shop-db".to_string())))
            .expect_err("duplicate");
        assert!(
            matches!(err, ConfigError::DuplicateAlias(ref a) if a == "shop-db"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn update_may_keep_its_own_alias() {
        // Re-sending the same alias is what a form does on every save; it is
        // not a collision with itself.
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(ConnectionDraft {
                mcp_alias: Some("shop-db".to_string()),
                ..turso_draft("local", "Local", ":memory:")
            })
            .expect("add");

        admin
            .update("local", alias_edit(Some("shop-db".to_string())))
            .expect("same alias is not a duplicate");
        assert_eq!(admin.entries()[0].mcp_alias.as_deref(), Some("shop-db"));
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
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
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
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
    fn import_refuses_an_entry_whose_ref_targets_an_existing_connections_slot() {
        // Target owns "victim" holding a real Supabase URL in the keychain
        // at dbboard.victim.url.
        let (_dir, secrets, mut target) = fresh_admin();
        target
            .add(supabase_draft(
                "victim",
                "postgres://real:secret@db.victim.supabase.co/postgres",
            ))
            .expect("seed victim");

        // Craft a bundle whose entry has a *brand-new* id but a
        // keyring_url_ref aimed at the victim's slot, plus a secret to write
        // there. Without the ref-collision guard this would silently
        // hijack the victim's live credentials on import.
        let mut file = ConnectionFile::empty();
        file.connections.push(ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: "attacker".to_string(),
            name: "Attacker".to_string(),
            kind: ConnectionKind::Supabase {
                keyring_url_ref: "dbboard.victim.url".to_string(),
            },
        });
        let mut malicious_secrets = BTreeMap::new();
        malicious_secrets.insert(
            "dbboard.victim.url".to_string(),
            "postgres://attacker@evil.example/db".to_string(),
        );
        let payload = BundlePayload::new(file, malicious_secrets);
        let blob = encrypt_bundle(&payload, BUNDLE_PASS).expect("encrypt");

        let report = target.import_bundle(&blob, BUNDLE_PASS).expect("import");

        // The crafted entry is refused, not imported.
        assert_eq!(report.skipped, vec!["attacker"]);
        assert!(report.imported.is_empty());
        assert_eq!(target.entries().len(), 1);
        // The victim's secret is intact — never overwritten by the bundle.
        assert_eq!(
            secrets.get("dbboard.victim.url").expect("url"),
            "postgres://real:secret@db.victim.supabase.co/postgres"
        );
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

    // ---- SSH tunnel write path (ADR-0069) ----

    fn pg_ssh_key_draft(id: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: Some(SshTunnelDraft {
                host: "bastion.example".to_string(),
                port: 2222,
                user: "deploy".to_string(),
                auth: SshAuthDraft::Key {
                    key_path: "/home/deploy/.ssh/id_ed25519".to_string(),
                    passphrase: Some("unlock-me".to_string()),
                },
                host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
            }),
            id: id.to_string(),
            name: format!("PG {id}"),
            kind: ConnectionKindDraft::Postgres {
                url: "postgres://u:p@db.internal:5432/app".to_string(),
            },
        }
    }

    fn pg_ssh_password_draft(id: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: Some(SshTunnelDraft {
                host: "bastion.example".to_string(),
                port: 22,
                user: "deploy".to_string(),
                auth: SshAuthDraft::Password("s3cr3t".to_string()),
                host_key: SshHostKeyDraft::KnownHosts("/home/deploy/.ssh/known_hosts".to_string()),
            }),
            id: id.to_string(),
            name: format!("PG {id}"),
            kind: ConnectionKindDraft::Postgres {
                url: "postgres://u:p@db.internal:5432/app".to_string(),
            },
        }
    }

    #[test]
    fn add_ssh_key_auth_persists_the_block_and_seeds_the_passphrase() {
        let (_dir, secrets, mut admin) = fresh_admin();
        let entry = admin.add(pg_ssh_key_draft("work")).expect("add").clone();
        let ssh = entry.ssh.expect("ssh block present");
        assert_eq!(ssh.host, "bastion.example");
        assert_eq!(ssh.port, 2222);
        assert_eq!(ssh.user, "deploy");
        assert_eq!(
            ssh.key_path.as_deref(),
            Some("/home/deploy/.ssh/id_ed25519")
        );
        assert_eq!(
            ssh.keyring_passphrase_ref.as_deref(),
            Some("dbboard.work.ssh_passphrase")
        );
        assert!(ssh.keyring_password_ref.is_none());
        assert_eq!(ssh.fingerprint.as_deref(), Some("SHA256:abc"));
        // The inline passphrase went to the keyring, never the TOML.
        assert_eq!(
            secrets.get("dbboard.work.ssh_passphrase").unwrap(),
            "unlock-me"
        );
        // The stored block is valid per the loader's own rules.
        ssh.validate().expect("stored block validates");
    }

    #[test]
    fn add_ssh_password_auth_seeds_the_password_secret() {
        let (_dir, secrets, mut admin) = fresh_admin();
        let entry = admin
            .add(pg_ssh_password_draft("work"))
            .expect("add")
            .clone();
        let ssh = entry.ssh.expect("ssh block present");
        assert!(ssh.key_path.is_none());
        assert_eq!(
            ssh.keyring_password_ref.as_deref(),
            Some("dbboard.work.ssh_password")
        );
        assert_eq!(
            ssh.known_hosts.as_deref(),
            Some("/home/deploy/.ssh/known_hosts")
        );
        assert_eq!(secrets.get("dbboard.work.ssh_password").unwrap(), "s3cr3t");
        ssh.validate().expect("stored block validates");
    }

    #[test]
    fn add_ssh_key_auth_without_passphrase_writes_no_secret() {
        let (_dir, secrets, mut admin) = fresh_admin();
        let mut draft = pg_ssh_key_draft("work");
        draft.ssh = Some(SshTunnelDraft {
            host: "bastion.example".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth: SshAuthDraft::Key {
                key_path: "/k/id".to_string(),
                passphrase: None,
            },
            host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
        });
        let entry = admin.add(draft).expect("add").clone();
        let ssh = entry.ssh.expect("ssh block present");
        assert!(ssh.keyring_passphrase_ref.is_none());
        assert!(matches!(
            secrets.get("dbboard.work.ssh_passphrase"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn add_ssh_on_a_non_tunnelable_kind_is_rejected_and_writes_nothing() {
        let (_dir, secrets, mut admin) = fresh_admin();
        let mut draft = turso_draft("local", "Local", "/tmp/db.sqlite");
        draft.ssh = Some(SshTunnelDraft {
            host: "bastion.example".to_string(),
            port: 22,
            user: "deploy".to_string(),
            auth: SshAuthDraft::Password("pw".to_string()),
            host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
        });
        let err = admin.add(draft).expect_err("must reject");
        assert!(
            matches!(err, ConfigError::SshUnsupportedKind { .. }),
            "got {err:?}"
        );
        // Nothing was seeded and the entry was not added.
        assert!(matches!(
            secrets.get("dbboard.local.ssh_password"),
            Err(SecretError::NotFound(_))
        ));
        assert!(admin.entries().is_empty());
    }

    #[test]
    fn update_adds_an_ssh_block_to_a_tunnelless_connection() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("work", "postgres://u:p@db.internal/app"))
            .expect("add");
        admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Set(SshTunnelEditDraft {
                        host: "bastion.example".to_string(),
                        port: 22,
                        user: "deploy".to_string(),
                        auth: SshAuthEditDraft::Password(SecretField::Set("pw".to_string())),
                        host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
                    }),
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update");
        let ssh = admin.entries()[0].ssh.as_ref().expect("ssh added");
        assert_eq!(ssh.host, "bastion.example");
        assert_eq!(secrets.get("dbboard.work.ssh_password").unwrap(), "pw");
    }

    #[test]
    fn update_keeps_the_stored_passphrase_when_asked() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_key_draft("work")).expect("add");
        // Edit a non-secret field (the bastion user) but Keep the passphrase.
        admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Set(SshTunnelEditDraft {
                        host: "bastion.example".to_string(),
                        port: 2222,
                        user: "ops".to_string(),
                        auth: SshAuthEditDraft::Key {
                            key_path: "/home/deploy/.ssh/id_ed25519".to_string(),
                            passphrase: SshPassphraseField::Keep,
                        },
                        host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
                    }),
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update");
        let ssh = admin.entries()[0].ssh.as_ref().expect("ssh kept");
        assert_eq!(ssh.user, "ops");
        assert_eq!(
            ssh.keyring_passphrase_ref.as_deref(),
            Some("dbboard.work.ssh_passphrase")
        );
        // The original passphrase is untouched.
        assert_eq!(
            secrets.get("dbboard.work.ssh_passphrase").unwrap(),
            "unlock-me"
        );
    }

    #[test]
    fn update_with_ssh_keep_preserves_the_tunnel_and_its_secret() {
        // An editor that does not render the tunnel sends `Keep`; renaming a
        // tunneled connection there must not drop the tunnel (ADR-0069).
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_password_draft("work")).expect("add");
        admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
                    name: "Renamed".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update");
        let entry = &admin.entries()[0];
        assert_eq!(entry.name, "Renamed");
        let ssh = entry.ssh.as_ref().expect("tunnel preserved");
        assert_eq!(ssh.host, "bastion.example");
        assert_eq!(
            ssh.keyring_password_ref.as_deref(),
            Some("dbboard.work.ssh_password")
        );
        // The secret slot the tunnel points at is untouched.
        assert_eq!(secrets.get("dbboard.work.ssh_password").unwrap(), "s3cr3t");
    }

    #[test]
    fn update_removing_the_tunnel_purges_its_secret() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_password_draft("work")).expect("add");
        assert!(secrets.get("dbboard.work.ssh_password").is_ok());
        admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Disable,
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update");
        assert!(admin.entries()[0].ssh.is_none());
        assert!(matches!(
            secrets.get("dbboard.work.ssh_password"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn update_switching_auth_purges_the_old_secret_and_writes_the_new() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_key_draft("work")).expect("add");
        assert!(secrets.get("dbboard.work.ssh_passphrase").is_ok());
        // Switch key auth -> password auth.
        admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Set(SshTunnelEditDraft {
                        host: "bastion.example".to_string(),
                        port: 2222,
                        user: "deploy".to_string(),
                        auth: SshAuthEditDraft::Password(SecretField::Set("pw2".to_string())),
                        host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
                    }),
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update");
        let ssh = admin.entries()[0].ssh.as_ref().expect("ssh present");
        assert!(ssh.key_path.is_none());
        assert_eq!(secrets.get("dbboard.work.ssh_password").unwrap(), "pw2");
        // The stale passphrase from the old key-auth block is gone.
        assert!(matches!(
            secrets.get("dbboard.work.ssh_passphrase"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn delete_purges_the_ssh_secret_too() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_password_draft("work")).expect("add");
        admin.delete("work").expect("delete");
        assert!(matches!(
            secrets.get("dbboard.work.ssh_password"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn update_set_password_keep_reuses_the_existing_password_ref() {
        // Editing a non-secret field of a password-auth tunnel while sending
        // `SecretField::Keep` must reuse the stored password, not fabricate a
        // fresh (unwritten) ref.
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_password_draft("work")).expect("add");
        admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Set(SshTunnelEditDraft {
                        host: "bastion.example".to_string(),
                        port: 2222,
                        user: "ops".to_string(),
                        auth: SshAuthEditDraft::Password(SecretField::Keep),
                        host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
                    }),
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect("update");
        let ssh = admin.entries()[0].ssh.as_ref().expect("ssh present");
        assert_eq!(ssh.user, "ops");
        assert_eq!(
            ssh.keyring_password_ref.as_deref(),
            Some("dbboard.work.ssh_password")
        );
        assert_eq!(secrets.get("dbboard.work.ssh_password").unwrap(), "s3cr3t");
    }

    #[test]
    fn update_switching_to_key_auth_encrypted_without_a_passphrase_is_rejected() {
        // Switching a password-auth tunnel to an *encrypted* key while leaving
        // the passphrase blank (mapped to `SshPassphraseField::Keep`) has
        // nothing to keep: the connection must not be saved pointing at a
        // passphrase ref that was never written.
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_password_draft("work")).expect("add");
        let err = admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Set(SshTunnelEditDraft {
                        host: "bastion.example".to_string(),
                        port: 22,
                        user: "deploy".to_string(),
                        auth: SshAuthEditDraft::Key {
                            key_path: "/home/deploy/.ssh/id_ed25519".to_string(),
                            passphrase: SshPassphraseField::Keep,
                        },
                        host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
                    }),
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect_err("must reject a keep with nothing to keep");
        assert!(matches!(err, ConfigError::SshInvalid { .. }), "got {err:?}");
        // The connection is unchanged: still password auth, secret intact.
        let ssh = admin.entries()[0].ssh.as_ref().expect("ssh unchanged");
        assert!(ssh.key_path.is_none());
        assert_eq!(secrets.get("dbboard.work.ssh_password").unwrap(), "s3cr3t");
    }

    #[test]
    fn update_switching_to_password_auth_without_a_password_is_rejected() {
        // Switching a key-auth tunnel to password auth with `SecretField::Keep`
        // has no stored password to keep: reject rather than persist a dangling
        // password ref.
        let (_dir, secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_key_draft("work")).expect("add");
        let err = admin
            .update(
                "work",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Set(SshTunnelEditDraft {
                        host: "bastion.example".to_string(),
                        port: 22,
                        user: "deploy".to_string(),
                        auth: SshAuthEditDraft::Password(SecretField::Keep),
                        host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
                    }),
                    name: "PG work".to_string(),
                    kind: ConnectionKindEditDraft::Postgres {
                        url: SecretField::Keep,
                    },
                },
            )
            .expect_err("must reject a password keep with nothing to keep");
        assert!(matches!(err, ConfigError::SshInvalid { .. }), "got {err:?}");
        // The connection is unchanged: still key auth, passphrase intact.
        let ssh = admin.entries()[0].ssh.as_ref().expect("ssh unchanged");
        assert_eq!(
            ssh.key_path.as_deref(),
            Some("/home/deploy/.ssh/id_ed25519")
        );
        assert_eq!(
            secrets.get("dbboard.work.ssh_passphrase").unwrap(),
            "unlock-me"
        );
    }

    #[test]
    fn export_and_import_round_trip_a_tunneled_connection() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin.add(pg_ssh_password_draft("work")).expect("add");
        let blob = admin.export_bundle(BUNDLE_PASS).expect("export");

        let (_dir2, secrets2, mut target) = fresh_admin();
        let report = target.import_bundle(&blob, BUNDLE_PASS).expect("import");
        assert_eq!(report.imported, vec!["work".to_string()]);
        let ssh = target.entries()[0].ssh.as_ref().expect("ssh imported");
        assert_eq!(ssh.host, "bastion.example");
        // The ssh secret travelled with the bundle into the new keychain.
        assert_eq!(secrets2.get("dbboard.work.ssh_password").unwrap(), "s3cr3t");
    }

    // --- DSN prefill for the edit form (ADR-0080) ---------------------------

    #[test]
    fn dsn_prefill_returns_the_stored_parts() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(mysql_draft(
                "shop",
                "mysql://app:hunter2@db.internal:3307/shop",
            ))
            .expect("add");

        let parts = admin.dsn_prefill("shop").expect("prefill").expect("some");
        assert_eq!(parts.host, "db.internal");
        assert_eq!(parts.port, Some(3307));
        assert_eq!(parts.user, "app");
        assert_eq!(parts.database, "shop");
    }

    // The reason the whole prefill path is safe to hand to a webview.
    #[test]
    fn dsn_prefill_never_exposes_the_password() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("prod", "postgres://app:hunter2@db:5432/analytics"))
            .expect("add");

        let parts = admin.dsn_prefill("prod").expect("prefill").expect("some");
        assert!(!format!("{parts:?}").contains("hunter2"));
    }

    #[test]
    fn dsn_prefill_keeps_the_tls_parameter() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(mysql_draft(
                "shop",
                "mysql://app:p@db:3306/shop?ssl-mode=disabled",
            ))
            .expect("add");

        let parts = admin.dsn_prefill("shop").expect("prefill").expect("some");
        assert_eq!(parts.query, "ssl-mode=disabled");
    }

    #[test]
    fn dsn_prefill_is_none_for_a_kind_with_no_dsn() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Local", "./a.db"))
            .expect("add");
        admin.add(d1_draft("edge")).expect("add");

        assert!(admin.dsn_prefill("local").expect("turso").is_none());
        assert!(admin.dsn_prefill("edge").expect("d1").is_none());
    }

    // Best-effort by design: a broken keychain entry opens an empty form
    // rather than a dialog that refuses to open at all.
    #[test]
    fn dsn_prefill_is_none_when_the_stored_value_is_not_a_url() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(mysql_draft("shop", "mysql://app:p@db:3306/shop"))
            .expect("add");
        secrets
            .set("dbboard.shop.url", "not a url")
            .expect("overwrite");

        assert!(admin.dsn_prefill("shop").expect("prefill").is_none());
    }

    #[test]
    fn dsn_prefill_rejects_an_unknown_id() {
        let (_dir, _secrets, admin) = fresh_admin();
        assert!(matches!(
            admin.dsn_prefill("ghost"),
            Err(ConfigError::NotFound(id)) if id == "ghost"
        ));
    }

    #[test]
    fn dsn_with_stored_password_grafts_the_kept_credential() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(mysql_draft("shop", "mysql://app:hunter2@db:3306/shop"))
            .expect("add");

        let merged = admin
            .dsn_with_stored_password("shop", "mysql://app@db.internal:3307/other")
            .expect("graft");
        assert_eq!(merged, "mysql://app:hunter2@db.internal:3307/other");
    }

    #[test]
    fn dsn_with_stored_password_keeps_a_newly_chosen_tls_mode() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(mysql_draft("shop", "mysql://app:hunter2@db:3306/shop"))
            .expect("add");

        let merged = admin
            .dsn_with_stored_password("shop", "mysql://app@db:3306/shop?ssl-mode=disabled")
            .expect("graft");
        assert_eq!(merged, "mysql://app:hunter2@db:3306/shop?ssl-mode=disabled");
    }

    // The strict half of the pair: silently saving a connection back without
    // its password would break a working connection with no visible cause.
    #[test]
    fn dsn_with_stored_password_fails_on_an_unparseable_stored_value() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(mysql_draft("shop", "mysql://app:p@db:3306/shop"))
            .expect("add");
        secrets
            .set("dbboard.shop.url", "not a url")
            .expect("overwrite");

        assert!(matches!(
            admin.dsn_with_stored_password("shop", "mysql://app@db:3306/shop"),
            Err(ConfigError::DsnUnparseable { id }) if id == "shop"
        ));
    }

    #[test]
    fn dsn_with_stored_password_rejects_a_kind_with_no_dsn() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Local", "./a.db"))
            .expect("add");

        assert!(matches!(
            admin.dsn_with_stored_password("local", "mysql://app@db:3306/shop"),
            Err(ConfigError::NotFound(id)) if id == "local"
        ));
    }
}
