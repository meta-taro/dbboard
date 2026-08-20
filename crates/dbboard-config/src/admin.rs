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

use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// Turso Cloud, or any other libSQL endpoint reached over the network
    /// (ADR-0111). The URL is not a secret and is stored inline; only the auth
    /// token reaches the keychain, the same split D1 makes.
    TursoRemote {
        url: String,
        token: String,
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
    /// Aurora DSQL with IAM auth (ADR-0036, ADR-0103). Only the AWS secret
    /// access key is a secret; the other five fields are stored inline because
    /// a `SigV4` token is minted from them at connect time, so there is no URL
    /// to hide them in.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_access_key: String,
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
    /// `MongoDB` (ADR-0096). The whole URI is the secret — the password rides
    /// in its authority — so it is the only field that reaches the keychain.
    /// `database` is optional: the URI may name it in its path.
    MongoDb {
        uri: String,
        database: Option<String>,
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
    /// Remote Turso (ADR-0111). Two states are enough for the token, as for
    /// D1's: a remote connection always has one, so there is no third "no
    /// credential" state to express.
    TursoRemote {
        url: String,
        token: SecretField,
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
    /// Aurora DSQL with IAM auth (ADR-0036, ADR-0103). Two states are enough
    /// for the secret access key, as for D1's token: an IAM connection always
    /// has one, so there is no third "no credential" state to express.
    AuroraDsqlIam {
        endpoint: String,
        region: String,
        database: String,
        username: String,
        access_key_id: String,
        secret_access_key: SecretField,
    },
    Firestore {
        project_id: String,
        database_id: Option<String>,
        base_url: Option<String>,
        service_account: FirestoreCredentialField,
    },
    /// `MongoDB` (ADR-0096). Two states are enough for the URI, unlike
    /// Firestore's three: a `MongoDB` connection always has one.
    MongoDb {
        uri: SecretField,
        database: Option<String>,
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

/// How [`ConnectionAdmin::import_bundle`] resolves an incoming id that
/// the live store already holds (ADR-0105).
///
/// This is only about the *id* collision. The ref-collision guard from
/// ADR-0038 is not a mode and does not relax: in either mode, an entry
/// whose `keyring_*_ref` aims at a slot some **other** connection owns is
/// refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImportMode {
    /// Keep what the store already has and report the collision. The
    /// original ADR-0038 behaviour, and the default: it cannot lose a
    /// credential the user has and the bundle does not.
    #[default]
    Skip,
    /// Replace the existing entry and its secrets in place. The user
    /// asked for this explicitly; the bundle is the source of truth.
    Overwrite,
}

/// One bundle entry the import refused because a `keyring_*_ref` it carries
/// belongs to a **different** connection already in the store (ADR-0038).
///
/// Carries both sides of the collision because neither alone is actionable:
/// the refused id is absent from the store afterwards, so an operator who is
/// told only the id finds nothing wherever they look and reads the import as
/// broken. Naming the slot and its owner is what turns the message into a
/// description of a deliberate refusal (ADR-0112).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedEntry {
    /// The bundle entry's id. It was not added to the store.
    pub id: String,
    /// The `keyring_*_ref` the entry carried.
    pub key_ref: String,
    /// The id of the connection in this store that owns `key_ref`.
    pub owner: String,
}

/// One entry in **this** store that carries a `keyring_*_ref` derived from a
/// different connection's id (issue #194).
///
/// A ref is minted in exactly one place — `keyring_ref(id, field)` — and
/// [`ConnectionAdmin::update`] writes the id straight back, so a connection's
/// id never changes and there is no rename path. An entry whose ref does not
/// derive from its own id therefore did not come from this program's own CRUD:
/// it was hand-edited into `connections.toml`, or imported before ADR-0038.
///
/// Because the ref carries its owner by construction, this is decidable from
/// the entry alone, with no lookup against the store. That is strictly
/// stronger than the import-side ADR-0038 check, which can only fire when the
/// receiving machine happens to hold the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignRef {
    /// The offending entry's id.
    pub id: String,
    /// The `keyring_*_ref` it carries.
    pub key_ref: String,
    /// The id the ref was minted for, read out of `key_ref` itself.
    pub owner: String,
}

/// Outcome of [`ConnectionAdmin::import_bundle`] (ADR-0038, ADR-0105,
/// ADR-0112).
///
/// The five lists partition the bundle's entries, and each preserves the
/// order in which the bundle presented them, so the UI can name exactly
/// which connections were added, which were replaced, and — separately for
/// each reason — which were not.
///
/// The three not-imported reasons are kept apart rather than merged into one
/// `skipped` list because only one of them ("already present") is true of the
/// others, and only one of them ("already present") is fixed by re-importing
/// with [`ImportMode::Overwrite`]. A single list forces one message onto all
/// three, which makes it false for two of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Ids added to the store by this import.
    pub imported: Vec<String>,
    /// Ids that replaced an existing entry of the same id
    /// ([`ImportMode::Overwrite`] only).
    pub overwritten: Vec<String>,
    /// Ids left alone because the store already held a connection of that id
    /// and the mode was [`ImportMode::Skip`]. Re-importing with
    /// [`ImportMode::Overwrite`] replaces these.
    pub skipped_existing: Vec<String>,
    /// Ids the bundle listed more than once. The first occurrence was taken;
    /// each later one is reported here. The mode makes no difference.
    pub duplicate_in_bundle: Vec<String>,
    /// Entries refused by the ADR-0038 ref-ownership check. The mode makes no
    /// difference: overwrite may replace the entry that owns an id, never a
    /// third connection's secret.
    pub refused: Vec<RefusedEntry>,
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
    /// Every `keyring_*_ref` on every entry is resolved through the
    /// [`SecretStore`] and packed alongside the metadata, because the TOML
    /// alone is useless on another machine (it stores only references).
    ///
    /// See [`ConnectionAdmin::export_bundle_of`] to export a subset.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Bundle`] if `passphrase` is weaker than
    ///   [`crate::MIN_PASSPHRASE_LEN`], or the age encryptor fails.
    /// - [`ConfigError::Secret`] if a referenced secret cannot be read
    ///   from the keychain. Export fails loudly here rather than shipping
    ///   a bundle that is silently missing a secret.
    pub fn export_bundle(&self, passphrase: &str) -> Result<Vec<u8>, ConfigError> {
        let all: Vec<&ConnectionEntry> = self.file.connections.iter().collect();
        self.encrypt_selection(&all, passphrase)
    }

    /// Encrypt only the connections named in `ids` (ADR-0105), otherwise
    /// identical to [`ConnectionAdmin::export_bundle`].
    ///
    /// The bundle lists the selected entries in **store order**, not
    /// argument order, so two exports of the same set are byte-comparable
    /// in their plaintext and the import report reads the way the list
    /// looks on screen.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::EmptySelection`] if `ids` is empty. An empty
    ///   bundle is indistinguishable from a wrong passphrase at import
    ///   time, so it is refused where the mistake was actually made.
    /// - [`ConfigError::NotFound`] if an id names no entry — the caller is
    ///   working from a stale view of the store (ADR-0016).
    /// - Everything [`ConnectionAdmin::export_bundle`] can return.
    pub fn export_bundle_of(
        &self,
        ids: &[String],
        passphrase: &str,
    ) -> Result<Vec<u8>, ConfigError> {
        if ids.is_empty() {
            return Err(ConfigError::EmptySelection);
        }
        let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
        // Name every id back at the caller before doing any work, so a
        // typo is not reported as "exported 2 of 3".
        for id in ids {
            if self.find_index(id).is_none() {
                return Err(ConfigError::NotFound(id.clone()));
            }
        }
        let selected: Vec<&ConnectionEntry> = self
            .file
            .connections
            .iter()
            .filter(|e| wanted.contains(e.id.as_str()))
            .collect();
        self.encrypt_selection(&selected, passphrase)
    }

    /// List every entry in the store carrying a keyring slot that belongs to a
    /// different connection (issue #194). Empty for a healthy store.
    ///
    /// Intended as a warning alongside an export, not a gate on it: an
    /// operator whose store is already in this state still needs a backup.
    #[must_use]
    pub fn foreign_refs(&self) -> Vec<ForeignRef> {
        let all: Vec<&ConnectionEntry> = self.file.connections.iter().collect();
        Self::foreign_refs_in(&all)
    }

    /// [`ConnectionAdmin::foreign_refs`] restricted to the connections named
    /// in `ids`, so an export warns about exactly what it is about to write.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NotFound`] if an id names no entry. Same contract as
    /// [`ConnectionAdmin::export_bundle_of`]: a caller working from a stale
    /// view must not be told "nothing wrong here" about a connection that is
    /// not there.
    pub fn foreign_refs_of(&self, ids: &[String]) -> Result<Vec<ForeignRef>, ConfigError> {
        let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
        for id in ids {
            if self.find_index(id).is_none() {
                return Err(ConfigError::NotFound(id.clone()));
            }
        }
        let selected: Vec<&ConnectionEntry> = self
            .file
            .connections
            .iter()
            .filter(|e| wanted.contains(e.id.as_str()))
            .collect();
        Ok(Self::foreign_refs_in(&selected))
    }

    /// Shared by both inspection entry points so the selective path cannot
    /// drift from the whole-store one, exactly as `encrypt_selection` is
    /// shared by the two export paths.
    fn foreign_refs_in(entries: &[&ConnectionEntry]) -> Vec<ForeignRef> {
        let mut found = Vec::new();
        for entry in entries {
            for key_ref in entry_keyring_refs(entry) {
                let Some(owner) = ref_owner(&key_ref) else {
                    // A ref of some other shape carries no owner to name. It
                    // is a different malformation, deliberately out of scope:
                    // saying it "belongs to" someone would be an invention.
                    continue;
                };
                if owner != entry.id {
                    found.push(ForeignRef {
                        id: entry.id.clone(),
                        owner: owner.to_string(),
                        key_ref,
                    });
                }
            }
        }
        found
    }

    /// Resolve every ref on `entries` and seal them into a bundle blob.
    /// Shared by both export entry points so the selective path cannot
    /// drift from the whole-store one.
    fn encrypt_selection(
        &self,
        entries: &[&ConnectionEntry],
        passphrase: &str,
    ) -> Result<Vec<u8>, ConfigError> {
        // Reject a weak passphrase before touching the keychain, so a
        // typo costs nothing.
        validate_passphrase(passphrase)?;

        let mut secrets = BTreeMap::new();
        for entry in entries {
            for key_ref in entry_keyring_refs(entry) {
                let value = self.secrets.get(&key_ref)?;
                secrets.insert(key_ref, value);
            }
        }

        let mut file = ConnectionFile::empty();
        file.version = self.file.version;
        file.connections = entries.iter().map(|e| (*e).clone()).collect();

        let payload = BundlePayload::new(file, secrets);
        let blob = encrypt_bundle(&payload, passphrase)?;
        Ok(blob)
    }

    /// Decrypt `blob` under `passphrase` and merge its connections into
    /// the live store (ADR-0038, slice b), returning an [`ImportReport`]
    /// of which ids were added and which were skipped.
    ///
    /// `mode` decides what happens when an incoming id already exists:
    /// [`ImportMode::Skip`] keeps what the store has and reports the
    /// collision, [`ImportMode::Overwrite`] replaces the entry and its
    /// secrets in place, keeping the slot it held in the list.
    ///
    /// **The id collision is the only thing `mode` governs.** An incoming
    /// entry whose `keyring_*_ref` points at a keychain slot **another**
    /// connection owns is skipped in either mode: `keyring_*_ref` is
    /// free-form JSON in the bundle, so a crafted bundle could otherwise
    /// carry a brand-new id but a ref aimed at an existing connection's
    /// slot and silently overwrite that connection's live secret (ADR-0038
    /// threat model). Overwriting an id means replacing the entry that owns
    /// that id, and nothing else.
    ///
    /// Secrets are seeded into the keychain first, then the TOML is
    /// persisted; on a TOML-write failure every touched slot is put back
    /// the way it was — deleted if this call created it, **restored to its
    /// previous value** if this call overwrote it. A rollback that only
    /// deleted would turn a failed import into credential loss.
    ///
    /// After a successful overwrite, keychain slots the replacement no
    /// longer references are purged, exactly as [`ConnectionAdmin::update`]
    /// does.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Bundle`] if the passphrase is wrong or the blob is
    ///   corrupt / not a dbboard bundle / a newer bundle version.
    /// - [`ConfigError::Secret`] if seeding an imported secret fails; any
    ///   secrets already written by this call are rolled back first.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML
    ///   write; the written secrets are rolled back before returning.
    pub fn import_bundle(
        &mut self,
        blob: &[u8],
        passphrase: &str,
        mode: ImportMode,
    ) -> Result<ImportReport, ConfigError> {
        let mut payload = decrypt_bundle(blob, passphrase)?;
        // Take the incoming entries out of the payload so we can iterate
        // them by value while still borrowing `payload.secrets` below.
        // `payload` implements `Drop` (it zeroizes its secret values), so
        // it cannot be partially moved out of — `mem::take` leaves an empty
        // vec behind and the payload still scrubs its secrets on drop.
        let incoming = std::mem::take(&mut payload.connections.connections);

        // Ids accepted earlier in this same bundle, so a bundle that lists
        // an id twice skips the second occurrence rather than creating a
        // duplicate entry (or overwriting its own first copy).
        let mut accepted: HashSet<String> = HashSet::new();
        // Which connection owns each keyring ref. A ref the incoming entry
        // already owns is not a collision — that is what overwriting its
        // own secret means — so this has to map to an owner rather than
        // just record that the ref is taken.
        let mut ref_owners: HashMap<String, String> = HashMap::new();
        for existing in &self.file.connections {
            for key_ref in entry_keyring_refs(existing) {
                ref_owners.insert(key_ref, existing.id.clone());
            }
        }

        let mut report = ImportReport::default();
        // `None` slot = append, `Some(idx)` = replace the entry sitting there.
        let mut to_apply: Vec<(Option<usize>, ConnectionEntry)> = Vec::new();
        let mut secret_writes: Vec<(String, String)> = Vec::new();

        for entry in incoming {
            if accepted.contains(&entry.id) {
                report.duplicate_in_bundle.push(entry.id);
                continue;
            }
            let slot = self.find_index(&entry.id);
            if slot.is_some() && mode == ImportMode::Skip {
                report.skipped_existing.push(entry.id);
                continue;
            }
            let refs = entry_keyring_refs(&entry);
            // Ref collides with a slot another connection owns; refuse rather
            // than overwrite that connection's secret. Report the owner along
            // with the ref: without it the entry is simply missing afterwards,
            // with no way to tell a refusal from a failed import (ADR-0112).
            let collision = refs.iter().find_map(|r| {
                ref_owners
                    .get(r)
                    .filter(|owner| *owner != &entry.id)
                    .map(|owner| (r.clone(), owner.clone()))
            });
            if let Some((key_ref, owner)) = collision {
                report.refused.push(RefusedEntry {
                    id: entry.id,
                    key_ref,
                    owner,
                });
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
                ref_owners.insert(key_ref, entry.id.clone());
            }
            accepted.insert(entry.id.clone());
            if slot.is_some() {
                report.overwritten.push(entry.id.clone());
            } else {
                report.imported.push(entry.id.clone());
            }
            to_apply.push((slot, entry));
        }

        if to_apply.is_empty() {
            return Ok(report);
        }

        // Write secrets first (same order as `add`); record what each slot
        // held beforehand so a later failure can put it back. Each cloned
        // secret value is scrubbed as soon as it has been handed to the
        // keychain (ADR-0038).
        let mut undo: Vec<(String, Option<String>)> = Vec::new();
        for i in 0..secret_writes.len() {
            let (key_ref, value) = &secret_writes[i];
            let prior = self.secrets.get(key_ref).ok();
            if let Err(err) = self.secrets.set(key_ref, value) {
                self.rollback_secret_writes(&mut undo);
                zeroize_secret_writes(&mut secret_writes);
                return Err(ConfigError::Secret(err));
            }
            undo.push((key_ref.clone(), prior));
        }
        zeroize_secret_writes(&mut secret_writes);

        let mut new_file = self.file.clone();
        // Replacements keep the slot they held, so importing over an
        // existing connection does not reshuffle the list under the user.
        let mut replaced: Vec<(usize, ConnectionEntry)> = Vec::new();
        for (slot, entry) in to_apply {
            match slot {
                Some(idx) => {
                    let old = std::mem::replace(&mut new_file.connections[idx], entry);
                    replaced.push((idx, old));
                }
                None => new_file.connections.push(entry),
            }
        }

        if let Err(err) = save_atomic(&self.path, &new_file) {
            self.rollback_secret_writes(&mut undo);
            return Err(err);
        }

        self.file = new_file;
        // The rollback plan held plaintext copies of whatever the
        // overwritten slots used to contain; the import succeeded, so
        // scrub them rather than leave them in this frame (ADR-0038).
        for (_key_ref, prior) in &mut undo {
            if let Some(value) = prior {
                value.zeroize();
            }
        }
        // Only now that the TOML names the replacements: a slot the new
        // entry does not reference is an orphan and must not linger.
        for (idx, old) in &replaced {
            self.purge_orphaned_secrets(old, &self.file.connections[*idx]);
        }
        Ok(report)
    }

    /// Best-effort undo of the secret writes a failed [`import_bundle`]
    /// made: a slot this call created is deleted, a slot it overwrote is
    /// restored to the value it held. Restoring matters — an overwrite
    /// rollback that deleted would destroy a credential the user had before
    /// the import and cannot get back from the bundle.
    ///
    /// A failure here is ignored: the alternative is aborting the rollback
    /// partway, which leaves a worse mess than one stale slot.
    /// The captured previous values are scrubbed on the way out.
    fn rollback_secret_writes(&self, undo: &mut [(String, Option<String>)]) {
        for (key_ref, prior) in undo.iter_mut() {
            match prior {
                Some(value) => {
                    let _ = self.secrets.set(key_ref, value);
                    value.zeroize();
                }
                None => {
                    let _ = self.secrets.delete(key_ref);
                }
            }
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

    /// The single-secret kinds again (see `secret_backed_add`): commit the
    /// secret only when it was retyped, then rebuild the variant around the ref
    /// it already had.
    ///
    /// The ref is never re-minted on edit — a new one would orphan the keychain
    /// entry the connection is still pointing at. Which field the ref names
    /// (`url`, `token`, …) makes no difference here; blank-means-keep is the
    /// same rule either way (ADR-0016).
    fn apply_secret_edit(
        &self,
        keyring_ref: &str,
        secret: SecretField,
        applied: &mut Vec<AppliedSecretWrite>,
        build: impl FnOnce(String) -> ConnectionKind,
    ) -> Result<ConnectionKind, ConfigError> {
        if let SecretField::Set(new_value) = secret {
            self.apply_secret_write(keyring_ref, &new_value, applied)?;
        }
        Ok(build(keyring_ref.to_string()))
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
                ConnectionKind::TursoRemote {
                    keyring_token_ref, ..
                },
                ConnectionKindEditDraft::TursoRemote { url, token },
            ) => self.apply_secret_edit(
                keyring_token_ref,
                token,
                &mut applied,
                |keyring_token_ref| ConnectionKind::TursoRemote {
                    url,
                    keyring_token_ref,
                },
            )?,
            (
                ConnectionKind::D1 {
                    keyring_token_ref, ..
                },
                draft @ ConnectionKindEditDraft::D1 { .. },
            ) => self.apply_d1_edit(id, keyring_token_ref, draft, &mut applied)?,
            (
                ConnectionKind::Postgres { keyring_url_ref },
                ConnectionKindEditDraft::Postgres { url },
            ) => self.apply_secret_edit(keyring_url_ref, url, &mut applied, |keyring_url_ref| {
                ConnectionKind::Postgres { keyring_url_ref }
            })?,
            (ConnectionKind::MySql { keyring_url_ref }, ConnectionKindEditDraft::MySql { url }) => {
                self.apply_secret_edit(keyring_url_ref, url, &mut applied, |keyring_url_ref| {
                    ConnectionKind::MySql { keyring_url_ref }
                })?
            }
            (ConnectionKind::Neon { keyring_url_ref }, ConnectionKindEditDraft::Neon { url }) => {
                self.apply_secret_edit(keyring_url_ref, url, &mut applied, |keyring_url_ref| {
                    ConnectionKind::Neon { keyring_url_ref }
                })?
            }
            (
                ConnectionKind::Supabase { keyring_url_ref },
                ConnectionKindEditDraft::Supabase { url },
            ) => self.apply_secret_edit(keyring_url_ref, url, &mut applied, |keyring_url_ref| {
                ConnectionKind::Supabase { keyring_url_ref }
            })?,
            (
                ConnectionKind::AuroraDsql { keyring_url_ref },
                ConnectionKindEditDraft::AuroraDsql { url },
            ) => self.apply_secret_edit(keyring_url_ref, url, &mut applied, |keyring_url_ref| {
                ConnectionKind::AuroraDsql { keyring_url_ref }
            })?,
            (
                ConnectionKind::AuroraDsqlIam {
                    keyring_secret_key_ref,
                    ..
                },
                draft @ ConnectionKindEditDraft::AuroraDsqlIam { .. },
            ) => {
                self.apply_aurora_dsql_iam_edit(id, keyring_secret_key_ref, draft, &mut applied)?
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
            (
                ConnectionKind::MongoDb {
                    keyring_url_ref, ..
                },
                ConnectionKindEditDraft::MongoDb { uri, database },
            ) => self.apply_secret_edit(keyring_url_ref, uri, &mut applied, |keyring_url_ref| {
                ConnectionKind::MongoDb {
                    keyring_url_ref,
                    database,
                }
            })?,
            (_, _) => {
                return Err(ConfigError::KindMismatch { id: id.to_string() });
            }
        };

        Ok((new_kind, applied))
    }

    /// The D1 arm of [`Self::apply_update_kind`], lifted out for length alone:
    /// three plain fields alongside the token make it too long to sit inline
    /// without pushing the match past clippy's function-length limit.
    fn apply_d1_edit(
        &self,
        id: &str,
        existing_ref: &str,
        draft: ConnectionKindEditDraft,
        applied: &mut Vec<AppliedSecretWrite>,
    ) -> Result<ConnectionKind, ConfigError> {
        let ConnectionKindEditDraft::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } = draft
        else {
            return Err(ConfigError::KindMismatch { id: id.to_string() });
        };

        self.apply_secret_edit(existing_ref, token, applied, |keyring_token_ref| {
            ConnectionKind::D1 {
                account_id,
                database_id,
                base_url,
                keyring_token_ref,
            }
        })
    }

    /// The Aurora DSQL (IAM) arm of [`Self::apply_update_kind`], lifted out for
    /// length alone: six fields make it the longest arm, and inline it pushed
    /// the match past clippy's function-length limit.
    fn apply_aurora_dsql_iam_edit(
        &self,
        id: &str,
        existing_ref: &str,
        draft: ConnectionKindEditDraft,
        applied: &mut Vec<AppliedSecretWrite>,
    ) -> Result<ConnectionKind, ConfigError> {
        let ConnectionKindEditDraft::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        } = draft
        else {
            return Err(ConfigError::KindMismatch { id: id.to_string() });
        };

        if let SecretField::Set(new_value) = secret_access_key {
            self.apply_secret_write(existing_ref, &new_value, applied)?;
        }

        // The ref is reused rather than re-minted: a new one would orphan the
        // keychain entry this connection still points at.
        Ok(ConnectionKind::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            keyring_secret_key_ref: existing_ref.to_string(),
        })
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

/// Read back the id a ref was minted for — the inverse of [`keyring_ref`].
/// `None` if `key_ref` is not of that shape at all.
///
/// Splits the field off from the **right**, because an id may contain dots
/// while a field name never does (they are the `*_FIELD` consts below, plus
/// the fixed `url` / `token`). Splitting from the left would read
/// `dbboard.my.db.url` as owner `my`.
fn ref_owner(key_ref: &str) -> Option<&str> {
    let rest = key_ref.strip_prefix("dbboard.")?;
    let (owner, _field) = rest.rsplit_once('.')?;
    (!owner.is_empty()).then_some(owner)
}

/// Keyring field names for the two SSH secrets. Kept as consts so the add and
/// update paths derive the exact same ref for a given id.
const SSH_PASSPHRASE_FIELD: &str = "ssh_passphrase";
const SSH_PASSWORD_FIELD: &str = "ssh_password";

/// Keyring field name for a Firestore service account, for the same reason as
/// the SSH consts above: `add` and `update` must derive the identical ref.
const FIRESTORE_SERVICE_ACCOUNT_FIELD: &str = "service_account";

/// Keyring field name for the AWS secret access key of an Aurora DSQL IAM
/// connection. Same reason again — and this one also has to keep matching the
/// refs written by hand into `connections.toml` before the kind was editable
/// in-app (ADR-0103), so its value is not free to change.
const AURORA_DSQL_IAM_SECRET_KEY_FIELD: &str = "secret_key";

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
/// at. `Turso` has none; `TursoRemote`, `D1`, `Postgres`, `MySql`, `Neon`, `Supabase`,
/// and `AuroraDsql` each carry exactly one; `AuroraDsqlIam` carries its
/// AWS secret-key ref (its other fields are non-secret and live inline);
/// `Firestore` carries one only when it is not pointed at the emulator;
/// `MongoDb` carries its URI ref (the password rides in the URI's authority,
/// so the whole URI is the secret).
fn keyring_refs_in(kind: &ConnectionKind) -> Vec<String> {
    match kind {
        ConnectionKind::Turso { .. } => Vec::new(),
        ConnectionKind::TursoRemote {
            keyring_token_ref, ..
        }
        | ConnectionKind::D1 {
            keyring_token_ref, ..
        } => vec![keyring_token_ref.clone()],
        ConnectionKind::Postgres { keyring_url_ref }
        | ConnectionKind::MySql { keyring_url_ref }
        | ConnectionKind::Neon { keyring_url_ref }
        | ConnectionKind::Supabase { keyring_url_ref }
        | ConnectionKind::AuroraDsql { keyring_url_ref }
        | ConnectionKind::MongoDb {
            keyring_url_ref, ..
        } => {
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

/// Most kinds carry exactly one secret: mint a ref for it, build the variant
/// around that ref, and queue the single write. Only the field name and the
/// variant differ, so sharing the body keeps them from drifting apart one paste
/// at a time.
fn secret_backed_add(
    id: &str,
    field: &str,
    value: String,
    build: impl FnOnce(String) -> ConnectionKind,
) -> (ConnectionKind, Vec<PendingSecretWrite>) {
    let key_ref = keyring_ref(id, field);
    let kind = build(key_ref.clone());
    let writes = vec![PendingSecretWrite { key_ref, value }];
    (kind, writes)
}

/// [`secret_backed_add`] for the kinds whose whole configuration *is* the URL:
/// the stored variant holds nothing but the ref.
fn url_backed_add(
    id: &str,
    url: String,
    build: impl FnOnce(String) -> ConnectionKind,
) -> (ConnectionKind, Vec<PendingSecretWrite>) {
    secret_backed_add(id, "url", url, build)
}

fn build_kind_for_add(
    id: &str,
    draft: ConnectionKindDraft,
) -> (ConnectionKind, Vec<PendingSecretWrite>) {
    match draft {
        ConnectionKindDraft::Turso { path } => (ConnectionKind::Turso { path }, Vec::new()),
        ConnectionKindDraft::TursoRemote { url, token } => {
            secret_backed_add(id, "token", token, |keyring_token_ref| {
                ConnectionKind::TursoRemote {
                    url,
                    keyring_token_ref,
                }
            })
        }
        ConnectionKindDraft::D1 {
            account_id,
            database_id,
            base_url,
            token,
        } => secret_backed_add(id, "token", token, |keyring_token_ref| ConnectionKind::D1 {
            account_id,
            database_id,
            base_url,
            keyring_token_ref,
        }),
        ConnectionKindDraft::Postgres { url } => url_backed_add(id, url, |keyring_url_ref| {
            ConnectionKind::Postgres { keyring_url_ref }
        }),
        ConnectionKindDraft::MySql { url } => url_backed_add(id, url, |keyring_url_ref| {
            ConnectionKind::MySql { keyring_url_ref }
        }),
        ConnectionKindDraft::Neon { url } => url_backed_add(id, url, |keyring_url_ref| {
            ConnectionKind::Neon { keyring_url_ref }
        }),
        ConnectionKindDraft::Supabase { url } => url_backed_add(id, url, |keyring_url_ref| {
            ConnectionKind::Supabase { keyring_url_ref }
        }),
        ConnectionKindDraft::AuroraDsql { url } => url_backed_add(id, url, |keyring_url_ref| {
            ConnectionKind::AuroraDsql { keyring_url_ref }
        }),
        ConnectionKindDraft::AuroraDsqlIam {
            endpoint,
            region,
            database,
            username,
            access_key_id,
            secret_access_key,
        } => secret_backed_add(
            id,
            AURORA_DSQL_IAM_SECRET_KEY_FIELD,
            secret_access_key,
            |keyring_secret_key_ref| ConnectionKind::AuroraDsqlIam {
                endpoint,
                region,
                database,
                username,
                access_key_id,
                keyring_secret_key_ref,
            },
        ),
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
        ConnectionKindDraft::MongoDb { uri, database } => {
            url_backed_add(id, uri, |keyring_url_ref| ConnectionKind::MongoDb {
                keyring_url_ref,
                database,
            })
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

    fn remote_turso_draft(id: &str, url: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("Turso Cloud {id}"),
            kind: ConnectionKindDraft::TursoRemote {
                url: url.to_string(),
                token: "t0k3n".to_string(),
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

    fn aurora_dsql_iam_draft(id: &str, secret_access_key: &str) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("Aurora DSQL IAM {id}"),
            kind: ConnectionKindDraft::AuroraDsqlIam {
                endpoint: "abc.dsql.ap-northeast-1.on.aws".to_string(),
                region: "ap-northeast-1".to_string(),
                database: "postgres".to_string(),
                username: "admin".to_string(),
                access_key_id: "AKIAEXAMPLE".to_string(),
                secret_access_key: secret_access_key.to_string(),
            },
        }
    }

    /// Every plain field differs from [`aurora_dsql_iam_draft`]'s, so a field
    /// the update forgets to carry shows up as the old value rather than
    /// passing by coincidence.
    fn aurora_dsql_iam_edit(name: &str, secret_access_key: SecretField) -> ConnectionEditDraft {
        ConnectionEditDraft {
            mcp_alias: None,
            mcp_write: None,
            ssh: SshEditField::Keep,
            name: name.to_string(),
            kind: ConnectionKindEditDraft::AuroraDsqlIam {
                endpoint: "moved.dsql.us-east-1.on.aws".to_string(),
                region: "us-east-1".to_string(),
                database: "analytics".to_string(),
                username: "reader".to_string(),
                access_key_id: "AKIAROTATED".to_string(),
                secret_access_key,
            },
        }
    }

    /// An admin holding one already-stored IAM entry with its secret seeded.
    fn iam_admin(
        id: &str,
        secret: &str,
    ) -> (tempfile::TempDir, Arc<InMemorySecretStore>, ConnectionAdmin) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let secret_ref = format!("dbboard.{id}.secret_key");
        secrets.set(&secret_ref, secret).expect("seed secret");
        let file = ConnectionFile {
            version: crate::store::CONFIG_VERSION,
            connections: vec![ConnectionEntry {
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
                id: id.to_string(),
                name: "Aurora DSQL (IAM)".to_string(),
                kind: ConnectionKind::AuroraDsqlIam {
                    endpoint: "abc.dsql.ap-northeast-1.on.aws".to_string(),
                    region: "ap-northeast-1".to_string(),
                    database: "postgres".to_string(),
                    username: "admin".to_string(),
                    access_key_id: "AKIAEXAMPLE".to_string(),
                    keyring_secret_key_ref: secret_ref,
                },
            }],
        };
        let admin =
            ConnectionAdmin::new_with_file(path, secrets.clone() as Arc<dyn SecretStore>, file);
        (dir, secrets, admin)
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

    fn mongodb_draft(id: &str, uri: &str, database: Option<&str>) -> ConnectionDraft {
        ConnectionDraft {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: id.to_string(),
            name: format!("MongoDB {id}"),
            kind: ConnectionKindDraft::MongoDb {
                uri: uri.to_string(),
                database: database.map(str::to_string),
            },
        }
    }

    #[test]
    fn add_mongodb_routes_the_uri_through_the_secret_store() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(mongodb_draft(
                "mongo-prod",
                "mongodb://user:pw@127.0.0.1:27117",
                Some("orders"),
            ))
            .expect("add mongodb");
        match &admin.entries()[0].kind {
            ConnectionKind::MongoDb {
                keyring_url_ref,
                database,
            } => {
                assert_eq!(keyring_url_ref, "dbboard.mongo-prod.url");
                assert_eq!(database.as_deref(), Some("orders"));
            }
            other => panic!("expected MongoDb, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.mongo-prod.url").expect("uri"),
            "mongodb://user:pw@127.0.0.1:27117"
        );
    }

    /// The whole URI is the secret (the password rides in its authority), so
    /// the TOML must never carry it — the same guarantee the Postgres kinds make.
    #[test]
    fn mongodb_toml_never_carries_the_uri() {
        let (dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(mongodb_draft(
                "mongo-prod",
                "mongodb://user:hunter2@127.0.0.1:27117",
                None,
            ))
            .expect("add mongodb");
        let written = std::fs::read_to_string(dir.path().join("connections.toml")).expect("read");
        assert!(!written.contains("hunter2"), "leaked: {written}");
        assert!(written.contains(r#"kind = "mongodb""#), "got: {written}");
    }

    #[test]
    fn update_mongodb_keeps_the_stored_uri_when_the_field_is_untouched() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(mongodb_draft("mongo", "mongodb://127.0.0.1:27117", None))
            .expect("add mongodb");
        admin
            .update(
                "mongo",
                ConnectionEditDraft {
                    name: "Renamed".to_string(),
                    kind: ConnectionKindEditDraft::MongoDb {
                        uri: SecretField::Keep,
                        database: Some("orders".to_string()),
                    },
                    ssh: SshEditField::Keep,
                    mcp_write: None,
                    mcp_alias: None,
                },
            )
            .expect("update mongodb");
        assert_eq!(admin.entries()[0].name, "Renamed");
        match &admin.entries()[0].kind {
            ConnectionKind::MongoDb { database, .. } => {
                assert_eq!(database.as_deref(), Some("orders"));
            }
            other => panic!("expected MongoDb, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.mongo.url").expect("uri"),
            "mongodb://127.0.0.1:27117",
            "a Keep must not disturb the stored URI"
        );
    }

    #[test]
    fn update_mongodb_overwrites_the_uri_when_set() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(mongodb_draft("mongo", "mongodb://127.0.0.1:27117", None))
            .expect("add mongodb");
        admin
            .update(
                "mongo",
                ConnectionEditDraft {
                    name: "MongoDB mongo".to_string(),
                    kind: ConnectionKindEditDraft::MongoDb {
                        uri: SecretField::Set("mongodb://127.0.0.1:27118".to_string()),
                        database: None,
                    },
                    ssh: SshEditField::Keep,
                    mcp_write: None,
                    mcp_alias: None,
                },
            )
            .expect("update mongodb");
        assert_eq!(
            secrets.get("dbboard.mongo.url").expect("uri"),
            "mongodb://127.0.0.1:27118"
        );
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
    fn add_remote_turso_keeps_the_url_inline_and_the_token_in_the_keychain() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(remote_turso_draft("cloud", "libsql://demo-acme.turso.io"))
            .expect("add remote turso");
        match &admin.entries()[0].kind {
            ConnectionKind::TursoRemote {
                url,
                keyring_token_ref,
            } => {
                assert_eq!(url, "libsql://demo-acme.turso.io");
                assert_eq!(keyring_token_ref, "dbboard.cloud.token");
            }
            other => panic!("expected TursoRemote, got {other:?}"),
        }
        assert_eq!(secrets.get("dbboard.cloud.token").expect("token"), "t0k3n");
    }

    #[test]
    fn update_remote_turso_can_change_the_url_without_retyping_the_token() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(remote_turso_draft("cloud", "libsql://old.turso.io"))
            .expect("add remote turso");
        admin
            .update(
                "cloud",
                ConnectionEditDraft {
                    mcp_alias: None,
                    mcp_write: None,
                    ssh: SshEditField::Keep,
                    name: "Turso Cloud".to_string(),
                    kind: ConnectionKindEditDraft::TursoRemote {
                        url: "libsql://new.turso.io".to_string(),
                        token: SecretField::Keep,
                    },
                },
            )
            .expect("update remote turso");
        match &admin.entries()[0].kind {
            ConnectionKind::TursoRemote { url, .. } => {
                assert_eq!(url, "libsql://new.turso.io");
            }
            other => panic!("expected TursoRemote, got {other:?}"),
        }
        // `Keep` means the keychain is not touched at all — the point of the
        // two-state field is that a URL rotation does not cost a credential.
        assert_eq!(secrets.get("dbboard.cloud.token").expect("token"), "t0k3n");
    }

    #[test]
    fn delete_remote_turso_purges_its_token() {
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(remote_turso_draft("cloud", "libsql://demo-acme.turso.io"))
            .expect("add remote turso");
        admin.delete("cloud").expect("delete");
        assert!(matches!(
            secrets.get("dbboard.cloud.token"),
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
    fn update_aurora_dsql_iam_with_a_different_kind_is_rejected_as_mismatch() {
        // The IAM kind is editable now (ADR-0103), but it is still its own
        // kind: pointing a plain Aurora DSQL draft at an IAM entry must fall
        // through `apply_update_kind`'s catch-all rather than silently
        // rewriting a token-minting entry into a URL-backed one.
        let (_dir, _secrets, mut admin) = iam_admin("dsql-iam", "AWS_SECRET");

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
    fn add_aurora_dsql_iam_stores_plain_fields_and_mints_the_secret_key_ref() {
        let (_dir, secrets, mut admin) = fresh_admin();

        admin
            .add(aurora_dsql_iam_draft("dsql-iam", "AWS_SECRET"))
            .expect("add");

        // The ref must be derived from the id exactly as delete/export expect
        // it (`dbboard.<id>.secret_key`), or the entry would leak its keychain
        // row on delete.
        assert_eq!(
            secrets.get("dbboard.dsql-iam.secret_key").expect("seeded"),
            "AWS_SECRET"
        );
        match &admin.entries()[0].kind {
            ConnectionKind::AuroraDsqlIam {
                endpoint,
                region,
                database,
                username,
                access_key_id,
                keyring_secret_key_ref,
            } => {
                assert_eq!(endpoint, "abc.dsql.ap-northeast-1.on.aws");
                assert_eq!(region, "ap-northeast-1");
                assert_eq!(database, "postgres");
                assert_eq!(username, "admin");
                assert_eq!(access_key_id, "AKIAEXAMPLE");
                assert_eq!(keyring_secret_key_ref, "dbboard.dsql-iam.secret_key");
            }
            other => panic!("expected AuroraDsqlIam, got {other:?}"),
        }
    }

    #[test]
    fn update_aurora_dsql_iam_rewrites_plain_fields_and_keeps_the_secret() {
        // The trigger for in-app editing is key rotation, but rotating the
        // *access key id* alone (a non-secret field) must not force the
        // operator to retype the secret they cannot read back (ADR-0016).
        let (_dir, secrets, mut admin) = iam_admin("dsql-iam", "AWS_SECRET");

        admin
            .update(
                "dsql-iam",
                aurora_dsql_iam_edit("renamed", SecretField::Keep),
            )
            .expect("update");

        assert_eq!(admin.entries()[0].name, "renamed");
        match &admin.entries()[0].kind {
            ConnectionKind::AuroraDsqlIam {
                endpoint,
                region,
                database,
                username,
                access_key_id,
                keyring_secret_key_ref,
            } => {
                assert_eq!(endpoint, "moved.dsql.us-east-1.on.aws");
                assert_eq!(region, "us-east-1");
                assert_eq!(database, "analytics");
                assert_eq!(username, "reader");
                assert_eq!(access_key_id, "AKIAROTATED");
                // Re-minting the ref would orphan the keychain row the entry
                // still points at.
                assert_eq!(keyring_secret_key_ref, "dbboard.dsql-iam.secret_key");
            }
            other => panic!("expected AuroraDsqlIam, got {other:?}"),
        }
        assert_eq!(
            secrets.get("dbboard.dsql-iam.secret_key").expect("kept"),
            "AWS_SECRET"
        );
    }

    #[test]
    fn update_aurora_dsql_iam_with_secret_set_overwrites_the_keyring_entry() {
        let (_dir, secrets, mut admin) = iam_admin("dsql-iam", "AWS_SECRET");

        admin
            .update(
                "dsql-iam",
                aurora_dsql_iam_edit("rotated", SecretField::Set("AWS_ROTATED".to_string())),
            )
            .expect("update");

        assert_eq!(
            secrets.get("dbboard.dsql-iam.secret_key").expect("written"),
            "AWS_ROTATED"
        );
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
        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

        assert_eq!(report.imported, vec!["store-a", "store-c", "local"]);
        assert!(report.skipped_existing.is_empty());
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

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

        // The conflict is reported, the two fresh ids are imported.
        assert_eq!(report.skipped_existing, vec!["store-a"]);
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
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("first import");
        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("second import");

        assert!(report.imported.is_empty());
        assert_eq!(report.skipped_existing, vec!["store-a", "store-c", "local"]);
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
        target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

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

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

        // The crafted entry is refused, not imported, and the report says
        // which slot it aimed at and who owns it.
        assert_eq!(
            report.refused,
            vec![RefusedEntry {
                id: "attacker".to_string(),
                key_ref: "dbboard.victim.url".to_string(),
                owner: "victim".to_string(),
            }]
        );
        assert!(report.skipped_existing.is_empty());
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
            .import_bundle(&blob, "the wrong passphrase", ImportMode::Skip)
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::Bundle(_)), "got {err:?}");
        // A failed import leaves the store empty.
        assert!(target.entries().is_empty());
    }

    #[test]
    fn import_of_garbage_bytes_is_a_bundle_error_not_a_panic() {
        let (_dir, _secrets, mut target) = fresh_admin();
        let err = target
            .import_bundle(b"not an age file", BUNDLE_PASS, ImportMode::Skip)
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

    // --- Selective export / overwrite import (ADR-0105) ---------------

    /// A store holding the same three connections `source_bundle` uses, so a
    /// subset export can be compared against the whole.
    fn three_connection_admin() -> (tempfile::TempDir, Arc<InMemorySecretStore>, ConnectionAdmin) {
        let (dir, secrets, mut admin) = fresh_admin();
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
        (dir, secrets, admin)
    }

    #[test]
    fn export_of_a_subset_carries_only_the_named_connections() {
        let (_dir, _secrets, admin) = three_connection_admin();

        let blob = admin
            .export_bundle_of(&["store-c".to_string()], BUNDLE_PASS)
            .expect("export subset");

        let (_target_dir, target_secrets, mut target) = fresh_admin();
        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

        assert_eq!(report.imported, vec!["store-c"]);
        assert_eq!(target.entries().len(), 1);
        // Only the named connection's secret travelled — an unnamed one must
        // not ride along in the payload's secret map.
        assert_eq!(
            target_secrets.get("dbboard.store-c.url").expect("url"),
            "postgres://postgres:pw@db.example.supabase.co/postgres"
        );
        assert!(matches!(
            target_secrets.get("dbboard.store-a.token"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn export_of_a_subset_keeps_the_stores_order_not_the_arguments() {
        let (_dir, _secrets, admin) = three_connection_admin();

        // Named back to front; the bundle must still list them store order.
        let blob = admin
            .export_bundle_of(&["local".to_string(), "store-a".to_string()], BUNDLE_PASS)
            .expect("export subset");

        let (_target_dir, _target_secrets, mut target) = fresh_admin();
        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");
        assert_eq!(report.imported, vec!["store-a", "local"]);
    }

    #[test]
    fn export_of_an_unknown_id_is_not_found() {
        let (_dir, _secrets, admin) = three_connection_admin();
        let err = admin
            .export_bundle_of(&["nope".to_string()], BUNDLE_PASS)
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::NotFound(id) if id == "nope"));
    }

    #[test]
    fn export_of_an_empty_selection_is_refused() {
        // An empty bundle is a footgun, not a feature: it decrypts fine and
        // imports nothing, which reads as "the passphrase was wrong".
        let (_dir, _secrets, admin) = three_connection_admin();
        let err = admin
            .export_bundle_of(&[], BUNDLE_PASS)
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::EmptySelection), "got {err:?}");
    }

    /// A store in the state `add` cannot produce: `beta` carries a slot
    /// derived from `alpha`'s id. Only a hand-edited `connections.toml` or an
    /// import predating ADR-0038 gets here, which is the whole reason export
    /// has to look for it (issue #194).
    fn store_with_a_foreign_ref() -> (tempfile::TempDir, Arc<InMemorySecretStore>, ConnectionAdmin)
    {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        secrets
            .set("dbboard.alpha.url", "postgres://a@example.test/db")
            .expect("seed alpha url");

        let mut file = ConnectionFile::empty();
        for id in ["alpha", "beta"] {
            file.connections.push(ConnectionEntry {
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
                id: id.to_string(),
                name: id.to_string(),
                // Both name alpha's slot. For alpha that is correct; for beta
                // it is the malformation.
                kind: ConnectionKind::Supabase {
                    keyring_url_ref: "dbboard.alpha.url".to_string(),
                },
            });
        }
        let admin =
            ConnectionAdmin::new_with_file(path, secrets.clone() as Arc<dyn SecretStore>, file);
        (dir, secrets, admin)
    }

    #[test]
    fn a_foreign_ref_is_reported_with_the_entry_the_slot_and_its_owner() {
        let (_dir, _secrets, admin) = store_with_a_foreign_ref();

        assert_eq!(
            admin.foreign_refs(),
            vec![ForeignRef {
                id: "beta".to_string(),
                key_ref: "dbboard.alpha.url".to_string(),
                owner: "alpha".to_string(),
            }]
        );
    }

    #[test]
    fn a_store_whose_refs_are_all_its_own_reports_nothing() {
        let (_dir, _secrets, admin) = three_connection_admin();
        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn a_foreign_ref_is_reported_even_when_the_owner_is_not_in_the_store() {
        // This is what makes the export-side check stronger than the
        // import-side one: the owner is read out of the ref, which carries it
        // by construction, so no store lookup is involved. The import check
        // can only fire when the target machine happens to hold the owner.
        let dir = tempdir().expect("tempdir");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut file = ConnectionFile::empty();
        file.connections.push(ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: "beta".to_string(),
            name: "Beta".to_string(),
            kind: ConnectionKind::Supabase {
                keyring_url_ref: "dbboard.alpha.url".to_string(),
            },
        });
        let admin = ConnectionAdmin::new_with_file(
            dir.path().join("connections.toml"),
            secrets as Arc<dyn SecretStore>,
            file,
        );

        assert_eq!(
            admin.foreign_refs(),
            vec![ForeignRef {
                id: "beta".to_string(),
                key_ref: "dbboard.alpha.url".to_string(),
                owner: "alpha".to_string(),
            }]
        );
    }

    #[test]
    fn a_ref_that_names_no_owner_is_left_alone() {
        // `dbboard.{owner}.{field}` is the only shape that carries an owner.
        // Anything else is a different malformation, and claiming it "belongs
        // to" someone would be an invention.
        let dir = tempdir().expect("tempdir");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut file = ConnectionFile::empty();
        file.connections.push(ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: "beta".to_string(),
            name: "Beta".to_string(),
            kind: ConnectionKind::Supabase {
                keyring_url_ref: "legacy-url".to_string(),
            },
        });
        let admin = ConnectionAdmin::new_with_file(
            dir.path().join("connections.toml"),
            secrets as Arc<dyn SecretStore>,
            file,
        );

        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn foreign_refs_of_looks_only_at_the_named_connections() {
        let (_dir, _secrets, admin) = store_with_a_foreign_ref();

        // Exporting alpha alone is a clean export; nothing to warn about.
        assert_eq!(
            admin
                .foreign_refs_of(&["alpha".to_string()])
                .expect("alpha exists"),
            Vec::new()
        );
        assert_eq!(
            admin
                .foreign_refs_of(&["beta".to_string()])
                .expect("beta exists")
                .len(),
            1
        );
    }

    #[test]
    fn foreign_refs_of_an_unknown_id_is_not_found() {
        // Same contract as `export_bundle_of`, so the caller cannot be told
        // "nothing wrong here" about a connection that is not there.
        let (_dir, _secrets, admin) = store_with_a_foreign_ref();
        let err = admin
            .foreign_refs_of(&["nope".to_string()])
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::NotFound(id) if id == "nope"));
    }

    #[test]
    fn export_still_produces_a_bundle_when_a_ref_is_foreign() {
        // Warn, do not refuse. An operator whose store is already in this
        // state still needs a backup, and blocking the export is the one
        // outcome that leaves them worse off than before.
        let (_dir, _secrets, admin) = store_with_a_foreign_ref();

        let blob = admin.export_bundle(BUNDLE_PASS).expect("export");
        assert!(!blob.is_empty());
    }

    #[test]
    fn overwrite_import_replaces_the_entry_and_its_secret_in_place() {
        let blob = source_bundle();

        // Target already holds `store-a` with a different name and token.
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

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Overwrite)
            .expect("import");

        assert_eq!(report.overwritten, vec!["store-a"]);
        assert_eq!(report.imported, vec!["store-c", "local"]);
        assert!(report.skipped_existing.is_empty());
        assert_eq!(target.entries().len(), 3);
        // The replacement keeps the slot the existing entry held, so the
        // list does not reshuffle under the user on every import.
        assert_eq!(target.entries()[0].id, "store-a");
        assert_eq!(target.entries()[0].name, "D1 store-a");
        // The bundle's secret won this time.
        assert_eq!(
            secrets.get("dbboard.store-a.token").expect("token"),
            "t0k3n"
        );
    }

    #[test]
    fn overwrite_import_purges_a_secret_the_replacement_no_longer_references() {
        // Bundle carries a *Turso* `x` (no secret at all); the target holds a
        // Supabase `x` whose URL sits in the keychain. After the overwrite
        // nothing points at that slot, so it must not survive.
        let (_src_dir, _src_secrets, src) = {
            let (dir, secrets, mut admin) = fresh_admin();
            admin
                .add(turso_draft("x", "Replacement", ":memory:"))
                .expect("add turso");
            (dir, secrets, admin)
        };
        let blob = src.export_bundle(BUNDLE_PASS).expect("export");

        let (_dir, secrets, mut target) = fresh_admin();
        target
            .add(supabase_draft(
                "x",
                "postgres://real:secret@db.example.supabase.co/postgres",
            ))
            .expect("seed");
        assert!(secrets.get("dbboard.x.url").is_ok());

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Overwrite)
            .expect("import");

        assert_eq!(report.overwritten, vec!["x"]);
        assert!(matches!(
            secrets.get("dbboard.x.url"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn overwrite_import_still_refuses_a_ref_aimed_at_another_connection() {
        // The ADR-0038 threat model does not relax in overwrite mode: an
        // incoming entry may replace the entry that owns its id, and nothing
        // else. A brand-new id whose ref points at someone else's slot is
        // still a hijack.
        let (_dir, secrets, mut target) = fresh_admin();
        target
            .add(supabase_draft(
                "victim",
                "postgres://real:secret@db.victim.supabase.co/postgres",
            ))
            .expect("seed victim");

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

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Overwrite)
            .expect("import");

        assert_eq!(
            report.refused.iter().map(|r| &r.id).collect::<Vec<_>>(),
            vec!["attacker"]
        );
        assert!(report.imported.is_empty());
        assert!(report.overwritten.is_empty());
        assert_eq!(
            secrets.get("dbboard.victim.url").expect("url"),
            "postgres://real:secret@db.victim.supabase.co/postgres"
        );
    }

    #[test]
    fn overwrite_import_restores_the_old_secret_when_the_toml_save_fails() {
        // The rollback for an overwritten secret cannot be a delete: the ref
        // existed before this import and still has to hold the value it held.
        let blob = source_bundle();

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("connections.toml");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut target =
            ConnectionAdmin::open(path, secrets.clone() as Arc<dyn SecretStore>).expect("open");
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
            .expect("seed");

        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"file-not-dir").expect("seed blocker");
        target.path = blocker.join("connections.toml");

        let err = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Overwrite)
            .expect_err("save must fail");
        assert!(matches!(err, ConfigError::Io(_)), "got {err:?}");

        // Restored, not deleted.
        assert_eq!(
            secrets.get("dbboard.store-a.token").expect("token"),
            "local-token"
        );
        // And the entry the failed import would have replaced is untouched.
        assert_eq!(target.entries().len(), 1);
        assert_eq!(target.entries()[0].name, "pre-existing");
    }

    #[test]
    fn skip_mode_is_unchanged_by_the_addition_of_overwrite() {
        // The pre-ADR-0105 behaviour is the default and must stay exactly as
        // it was: nothing overwritten, conflicts reported.
        let blob = source_bundle();
        let (_dir, _secrets, mut target) = fresh_admin();
        target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("first import");
        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("second import");

        assert!(report.imported.is_empty());
        assert!(report.overwritten.is_empty());
        assert_eq!(report.skipped_existing, vec!["store-a", "store-c", "local"]);
    }

    // --- The three not-imported reasons are reported apart (ADR-0112) ---

    /// A bundle that trips every not-imported condition at once, so the
    /// report has to keep them apart rather than pour them into one list.
    fn mixed_outcome_bundle() -> Vec<u8> {
        let mut file = ConnectionFile::empty();
        // (a) An id the target already holds. Deliberately a different kind
        //     from the target's, to show the check is on the id alone.
        file.connections.push(ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: None,
            id: "store-a".to_string(),
            name: "Bundle store-a".to_string(),
            kind: ConnectionKind::Turso {
                path: ":memory:".to_string(),
            },
        });
        // (b) A brand-new id whose ref aims at another connection's slot.
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
        // (c) An id the bundle itself lists twice.
        for name in ["Fresh", "Fresh again"] {
            file.connections.push(ConnectionEntry {
                mcp_alias: None,
                mcp_write: false,
                ssh: None,
                id: "fresh".to_string(),
                name: name.to_string(),
                kind: ConnectionKind::Turso {
                    path: ":memory:".to_string(),
                },
            });
        }
        let mut secrets = BTreeMap::new();
        secrets.insert(
            "dbboard.victim.url".to_string(),
            "postgres://attacker@evil.example/db".to_string(),
        );
        let payload = BundlePayload::new(file, secrets);
        encrypt_bundle(&payload, BUNDLE_PASS).expect("encrypt")
    }

    /// Seeds `store-a` (id collision) and `victim` (ref owner) so
    /// `mixed_outcome_bundle` trips both against it.
    fn mixed_outcome_target() -> (tempfile::TempDir, Arc<InMemorySecretStore>, ConnectionAdmin) {
        let (dir, secrets, mut target) = fresh_admin();
        target.add(d1_draft("store-a")).expect("seed store-a");
        target
            .add(supabase_draft(
                "victim",
                "postgres://real:secret@db.victim.supabase.co/postgres",
            ))
            .expect("seed victim");
        (dir, secrets, target)
    }

    #[test]
    fn the_three_not_imported_reasons_land_in_three_separate_lists() {
        let blob = mixed_outcome_bundle();
        let (_dir, _secrets, mut target) = mixed_outcome_target();

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

        assert_eq!(report.imported, vec!["fresh"]);
        // Only this one is genuinely "already present" — the string the UI
        // shows for it is the only one that may say so.
        assert_eq!(report.skipped_existing, vec!["store-a"]);
        assert_eq!(report.duplicate_in_bundle, vec!["fresh"]);
        assert_eq!(
            report.refused.iter().map(|r| &r.id).collect::<Vec<_>>(),
            vec!["attacker"]
        );
    }

    #[test]
    fn a_refusal_names_the_offending_ref_and_the_connection_that_owns_it() {
        // The operator cannot act on "skipped": the id is nowhere to be
        // found afterwards, so the message has to carry both sides of the
        // collision or it reads as a corrupted import.
        let blob = mixed_outcome_bundle();
        let (_dir, _secrets, mut target) = mixed_outcome_target();

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");

        assert_eq!(
            report.refused,
            vec![RefusedEntry {
                id: "attacker".to_string(),
                key_ref: "dbboard.victim.url".to_string(),
                owner: "victim".to_string(),
            }]
        );
    }

    #[test]
    fn a_refusal_is_still_a_refusal_in_overwrite_mode_not_a_skip() {
        // The ref-ownership check ignores the mode, so re-importing with
        // overwrite on produces the same outcome. The report must not put
        // the entry anywhere that invites that retry.
        let blob = mixed_outcome_bundle();
        let (_dir, _secrets, mut target) = mixed_outcome_target();

        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Overwrite)
            .expect("import");

        assert_eq!(
            report.refused.iter().map(|r| &r.id).collect::<Vec<_>>(),
            vec!["attacker"]
        );
        // Overwrite took the id collision, so nothing is left to "skip".
        assert!(report.skipped_existing.is_empty());
        assert_eq!(report.overwritten, vec!["store-a"]);
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
        let report = target
            .import_bundle(&blob, BUNDLE_PASS, ImportMode::Skip)
            .expect("import");
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
