//! Duplicating a connection, and repairing one that points at another
//! connection's keychain slot (issue #213).
//!
//! Both operations exist because a `keyring_*_ref` is minted in exactly one
//! place — `keyring_ref` — and only on [`ConnectionAdmin::add`]. `update`
//! reuses whatever ref the entry already carries, so there has never been a
//! way to *change* one. That left two things a user could not do without
//! hand-editing `connections.toml`:
//!
//! - register a second connection sharing one credential (two D1 databases on
//!   one API token, two schemas behind one Postgres URL), and
//! - fix an entry whose ref names someone else's slot, which is what
//!   hand-editing produces and what [`ConnectionAdmin::foreign_refs`] and the
//!   import guard (ADR-0038) refuse.
//!
//! The two differ in where the secret comes from, and that difference is the
//! whole point. A duplicate copies a slot the source entry **owns**, so the
//! value can be read back and re-written without asking. A repair points at a
//! slot some *other* connection owns, so the value is not ours to copy: the
//! caller supplies it.

use zeroize::Zeroize;

use super::{
    keyring_ref, split_ref, zeroize_secret_writes, ConnectionAdmin, ConnectionEntry, ConnectionKind,
};
use crate::error::ConfigError;
use crate::store::save_atomic;

impl ConnectionAdmin {
    /// Copy the connection `id` into a new entry `new_id` named `new_name`,
    /// minting the copy's own keyring refs and seeding them with the source's
    /// secret values (issue #213).
    ///
    /// The copy is deliberately not a byte-for-byte clone:
    ///
    /// - **`mcp_alias` is dropped.** An alias is a unique handle (ADR-0088);
    ///   copying it would collide with the source by construction.
    /// - **`mcp_write` is off.** The copy addresses a *different* database, and
    ///   nobody has approved writes to that one yet (ADR-0087). Inheriting the
    ///   flag would hand an agent write access to a database the human never
    ///   saw.
    ///
    /// Everything else — kind, endpoints, the SSH tunnel — is carried over, and
    /// every secret the source owns is re-written under a ref derived from
    /// `new_id`. The source's own slots are left exactly as they were.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::NotFound`] if `id` names no entry.
    /// - [`ConfigError::DuplicateId`] if `new_id` is taken, or
    ///   [`ConfigError::DuplicateAlias`] if it shadows another entry's alias
    ///   (ADR-0088).
    /// - [`ConfigError::UnusableSourceRef`] if the source carries a ref not
    ///   minted from its own id. Reading that slot would be reading another
    ///   connection's credential; repair the source first.
    /// - [`ConfigError::Secret`] if a source secret cannot be read, or a copy
    ///   cannot be written. A partial write is rolled back.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML write,
    ///   after rolling the copy's secret writes back.
    ///
    /// # Panics
    ///
    /// Never in practice: the just-pushed entry is borrowed back via `last()`,
    /// exactly as [`ConnectionAdmin::add`] does.
    pub fn duplicate(
        &mut self,
        id: &str,
        new_id: String,
        new_name: String,
    ) -> Result<&ConnectionEntry, ConfigError> {
        let idx = self
            .find_index(id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        if self.find_index(&new_id).is_some() {
            return Err(ConfigError::DuplicateId(new_id));
        }
        // The new id is itself a handle an agent may hand back, so it may not
        // shadow an existing alias either — the same check `add` runs.
        self.ensure_handle_is_free(&new_id, &new_id)?;

        let source = self.file.connections[idx].clone();

        let mut copy = ConnectionEntry {
            id: new_id.clone(),
            name: new_name,
            mcp_alias: None,
            mcp_write: false,
            kind: source.kind.clone(),
            ssh: source.ssh.clone(),
        };

        // Re-point every slot at the copy's own id before touching the
        // keychain, so a source we refuse to read costs nothing to back out of.
        let mut remints: Vec<(String, String)> = Vec::new();
        for slot in entry_keyring_refs_mut(&mut copy) {
            let old_ref = slot.clone();
            let field = match split_ref(&old_ref) {
                Some((owner, field)) if owner == source.id => field,
                _ => {
                    return Err(ConfigError::UnusableSourceRef {
                        id: source.id,
                        key_ref: old_ref,
                    })
                }
            };
            let new_ref = keyring_ref(&new_id, field);
            new_ref.clone_into(slot);
            remints.push((old_ref, new_ref));
        }

        let mut writes: Vec<(String, String)> = Vec::with_capacity(remints.len());
        for (old_ref, new_ref) in remints {
            match self.secrets.get(&old_ref) {
                Ok(value) => writes.push((new_ref, value)),
                Err(err) => {
                    zeroize_secret_writes(&mut writes);
                    return Err(ConfigError::Secret(err));
                }
            }
        }

        let outcome = self.commit_copy(copy, &writes);
        zeroize_secret_writes(&mut writes);
        outcome?;

        Ok(self.file.connections.last().expect("just-duplicated entry"))
    }

    /// Write the copy's secrets and then the file, rolling the keychain back to
    /// where it started if either half fails.
    ///
    /// Split out of [`ConnectionAdmin::duplicate`] so the plaintext values stay
    /// in one buffer the caller can zeroize on every path out, rather than
    /// being scattered across early returns.
    fn commit_copy(
        &mut self,
        copy: ConnectionEntry,
        writes: &[(String, String)],
    ) -> Result<(), ConfigError> {
        let mut written: Vec<&str> = Vec::new();
        for (key_ref, value) in writes {
            if let Err(err) = self.secrets.set(key_ref, value) {
                for done in &written {
                    let _ = self.secrets.delete(done);
                }
                return Err(ConfigError::Secret(err));
            }
            written.push(key_ref);
        }

        let mut new_file = self.file.clone();
        new_file.connections.push(copy);
        if let Err(err) = save_atomic(&self.path, &new_file) {
            // Nothing references these yet, so removing them leaves the
            // keychain exactly as this call found it.
            for done in &written {
                let _ = self.secrets.delete(done);
            }
            return Err(err);
        }

        self.file = new_file;
        Ok(())
    }

    /// Re-point one of `id`'s keyring slots — currently `key_ref`, which was
    /// minted for a *different* connection — at a ref derived from `id`, and
    /// store `secret` there (issue #213).
    ///
    /// `secret` comes from the caller because the value under `key_ref` is not
    /// ours to copy: it is whatever the owning connection put there. Reading it
    /// to seed the repair would silently duplicate another connection's
    /// credential, which is the thing the import guard (ADR-0038) exists to
    /// refuse.
    ///
    /// For the same reason `key_ref` is **not** deleted. It is a live slot with
    /// a live owner; this call stops referencing it, nothing more.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::NotFound`] if `id` names no entry.
    /// - [`ConfigError::RefNotOnEntry`] if that entry does not point at
    ///   `key_ref` — a caller working from a stale view of the store.
    /// - [`ConfigError::NothingToRepair`] if `key_ref` is already derived from
    ///   `id`, or is shaped so no owner can be read out of it at all.
    /// - [`ConfigError::Secret`] if the new slot cannot be written.
    /// - [`ConfigError::Io`] / [`ConfigError::Serialize`] from the TOML write,
    ///   after deleting the slot this call had just minted.
    ///
    /// # Panics
    ///
    /// Never in practice: the entry is repaired in place, so the id that
    /// resolved on the way in still resolves on the way out.
    pub fn repair_foreign_ref(
        &mut self,
        id: &str,
        key_ref: &str,
        mut secret: String,
    ) -> Result<&ConnectionEntry, ConfigError> {
        let outcome = self.repair_inner(id, key_ref, &secret);
        secret.zeroize();
        outcome?;
        let idx = self.find_index(id).expect("entry repaired in place");
        Ok(&self.file.connections[idx])
    }

    /// The body of [`ConnectionAdmin::repair_foreign_ref`], kept separate so
    /// the caller can zeroize the plaintext on every path out.
    fn repair_inner(&mut self, id: &str, key_ref: &str, secret: &str) -> Result<(), ConfigError> {
        let idx = self
            .find_index(id)
            .ok_or_else(|| ConfigError::NotFound(id.to_string()))?;
        let mut entry = self.file.connections[idx].clone();

        // Say "this entry does not have that slot" before "that slot needs no
        // repair": a stale view is the more specific diagnosis, and the more
        // likely one.
        if !entry_keyring_refs_mut(&mut entry)
            .iter()
            .any(|slot| *slot == key_ref)
        {
            return Err(ConfigError::RefNotOnEntry {
                id: id.to_string(),
                key_ref: key_ref.to_string(),
            });
        }

        let field = match split_ref(key_ref) {
            Some((owner, field)) if owner != id => field,
            _ => {
                return Err(ConfigError::NothingToRepair {
                    id: id.to_string(),
                    key_ref: key_ref.to_string(),
                })
            }
        };

        let own_ref = keyring_ref(id, field);
        for slot in entry_keyring_refs_mut(&mut entry) {
            if slot == key_ref {
                own_ref.clone_into(slot);
            }
        }

        self.secrets.set(&own_ref, secret)?;

        let mut new_file = self.file.clone();
        new_file.connections[idx] = entry;
        if let Err(err) = save_atomic(&self.path, &new_file) {
            // Only the slot this call minted is removed. `key_ref` belongs to
            // another connection and is never touched here.
            let _ = self.secrets.delete(&own_ref);
            return Err(err);
        }

        self.file = new_file;
        Ok(())
    }
}

/// Every `keyring_*_ref` on `entry`, borrowed mutably so it can be re-pointed.
///
/// The read-only twin of this is `entry_keyring_refs`, and the two must agree
/// on which slots exist: a variant missed here is a slot a duplicate would
/// carry over unchanged, which is exactly the breakage this module exists to
/// stop. `duplicate_re_mints_every_kind` below walks all kinds so an omission
/// fails loudly rather than shipping.
fn entry_keyring_refs_mut(entry: &mut ConnectionEntry) -> Vec<&mut String> {
    let mut refs = kind_keyring_refs_mut(&mut entry.kind);
    if let Some(ssh) = entry.ssh.as_mut() {
        refs.extend(ssh.keyring_passphrase_ref.as_mut());
        refs.extend(ssh.keyring_password_ref.as_mut());
    }
    refs
}

/// The mutable twin of `keyring_refs_in`; see [`entry_keyring_refs_mut`].
fn kind_keyring_refs_mut(kind: &mut ConnectionKind) -> Vec<&mut String> {
    match kind {
        ConnectionKind::Turso { .. } => Vec::new(),
        ConnectionKind::TursoRemote {
            keyring_token_ref, ..
        }
        | ConnectionKind::D1 {
            keyring_token_ref, ..
        } => vec![keyring_token_ref],
        ConnectionKind::Postgres { keyring_url_ref }
        | ConnectionKind::MySql { keyring_url_ref }
        | ConnectionKind::Neon { keyring_url_ref }
        | ConnectionKind::Supabase { keyring_url_ref }
        | ConnectionKind::AuroraDsql { keyring_url_ref }
        | ConnectionKind::MongoDb {
            keyring_url_ref, ..
        } => vec![keyring_url_ref],
        ConnectionKind::AuroraDsqlIam {
            keyring_secret_key_ref,
            ..
        } => vec![keyring_secret_key_ref],
        ConnectionKind::Firestore {
            keyring_service_account_ref,
            ..
        } => keyring_service_account_ref.as_mut().into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::tests::{
        aurora_dsql_draft, aurora_dsql_iam_draft, d1_draft, firestore_draft, fresh_admin,
        mongodb_draft, mysql_draft, neon_draft, pg_draft, remote_turso_draft, supabase_draft,
        turso_draft,
    };
    use super::*;
    use crate::admin::{ref_owner, ConnectionDraft, SshAuthDraft, SshHostKeyDraft, SshTunnelDraft};
    use crate::secrets::{InMemorySecretStore, SecretStore};
    use crate::store::{ConnectionFile, SshTunnelToml};
    use tempfile::tempdir;

    /// A store in the state `add` cannot produce: `beta` carries a slot derived
    /// from `alpha`'s id, which is what hand-editing `connections.toml`
    /// produces (issue #194). Mirrors the fixture in the parent module's tests;
    /// kept separate because repair mutates it and those tests only read it.
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
                kind: ConnectionKind::Supabase {
                    keyring_url_ref: "dbboard.alpha.url".to_string(),
                },
            });
        }
        let admin =
            ConnectionAdmin::new_with_file(path, secrets.clone() as Arc<dyn SecretStore>, file);
        (dir, secrets, admin)
    }

    fn ssh_tunnel_draft(password: &str) -> SshTunnelDraft {
        SshTunnelDraft {
            host: "bastion.example.test".to_string(),
            port: 22,
            user: "tunnel".to_string(),
            auth: SshAuthDraft::Password(password.to_string()),
            host_key: SshHostKeyDraft::Fingerprint("SHA256:abc".to_string()),
        }
    }

    /// Every keyring slot the entry points at, in the order the kind declares
    /// them. Reads through the mutable accessor on purpose: it is the thing
    /// under test.
    fn refs_of(entry: &ConnectionEntry) -> Vec<String> {
        let mut clone = entry.clone();
        entry_keyring_refs_mut(&mut clone)
            .into_iter()
            .map(|slot| slot.clone())
            .collect()
    }

    // ---- duplicate ------------------------------------------------------

    #[test]
    fn duplicate_mints_the_copy_its_own_slot_and_seeds_it_with_the_same_secret() {
        // The whole point of #213: two connections may legitimately share one
        // credential, and the supported way to get there must not leave the
        // second one pointing at the first one's slot.
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("prod", "postgres://u:p@example.test/app"))
            .expect("add source");

        let copy = admin
            .duplicate(
                "prod",
                "prod-reporting".to_string(),
                "Reporting".to_string(),
            )
            .expect("duplicate");

        assert_eq!(copy.id, "prod-reporting");
        assert_eq!(copy.name, "Reporting");
        assert_eq!(refs_of(copy), vec!["dbboard.prod-reporting.url"]);
        assert_eq!(
            secrets.get("dbboard.prod-reporting.url").expect("copy url"),
            "postgres://u:p@example.test/app"
        );
        // ... and the source is untouched, slot and value alike.
        assert_eq!(
            secrets.get("dbboard.prod.url").expect("source url"),
            "postgres://u:p@example.test/app"
        );
        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn duplicate_persists_so_a_reload_sees_the_copy() {
        let (dir, secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("prod", "postgres://u:p@example.test/app"))
            .expect("add source");
        admin
            .duplicate("prod", "copy".to_string(), "Copy".to_string())
            .expect("duplicate");

        let reopened = ConnectionAdmin::open(
            dir.path().join("connections.toml"),
            secrets as Arc<dyn SecretStore>,
        )
        .expect("reopen");
        let copy = reopened
            .entries()
            .iter()
            .find(|e| e.id == "copy")
            .expect("copy survived the round trip");
        assert_eq!(refs_of(copy), vec!["dbboard.copy.url"]);
    }

    #[test]
    fn duplicate_drops_the_alias_and_leaves_mcp_write_off() {
        // An alias is unique by construction (ADR-0088), so it cannot be
        // copied; and the copy addresses a different database, which nobody has
        // approved writes to yet (ADR-0087).
        let (_dir, _secrets, mut admin) = fresh_admin();
        let mut draft = pg_draft("prod", "postgres://u:p@example.test/app");
        draft.mcp_alias = Some("main".to_string());
        draft.mcp_write = true;
        admin.add(draft).expect("add source");

        let copy = admin
            .duplicate("prod", "copy".to_string(), "Copy".to_string())
            .expect("duplicate");

        assert_eq!(copy.mcp_alias, None);
        assert!(!copy.mcp_write);
    }

    #[test]
    fn duplicate_copies_the_ssh_tunnel_onto_the_copys_own_slot() {
        let (_dir, secrets, mut admin) = fresh_admin();
        let mut draft = pg_draft("prod", "postgres://u:p@example.test/app");
        draft.ssh = Some(ssh_tunnel_draft("s3cret"));
        admin.add(draft).expect("add source");

        let copy = admin
            .duplicate("prod", "copy".to_string(), "Copy".to_string())
            .expect("duplicate");

        let ssh = copy.ssh.as_ref().expect("tunnel carried over");
        assert_eq!(ssh.host, "bastion.example.test");
        assert_eq!(
            ssh.keyring_password_ref.as_deref(),
            Some("dbboard.copy.ssh_password")
        );
        assert_eq!(
            secrets
                .get("dbboard.copy.ssh_password")
                .expect("copy ssh password"),
            "s3cret"
        );
        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn duplicate_re_mints_every_kind() {
        // `entry_keyring_refs_mut` mirrors `keyring_refs_in` by hand. A variant
        // missed there is a slot the copy would carry over unchanged — exactly
        // the state this module exists to stop — so walk all of them.
        let (_dir, _secrets, mut admin) = fresh_admin();
        let drafts: Vec<ConnectionDraft> = vec![
            turso_draft("local", "Local", "/tmp/local.db"),
            remote_turso_draft("turso", "libsql://example.turso.io"),
            d1_draft("d1"),
            pg_draft("pg", "postgres://u:p@example.test/app"),
            mysql_draft("my", "mysql://u:p@example.test/app"),
            neon_draft("neon", "postgres://u:p@neon.example.test/app"),
            supabase_draft("supa", "postgres://u:p@supa.example.test/app"),
            aurora_dsql_draft("dsql", "postgres://u:p@dsql.example.test/app"),
            aurora_dsql_iam_draft("dsqliam", "aws-secret"),
            firestore_draft("fire", Some("{\"type\":\"service_account\"}")),
            mongodb_draft("mongo", "mongodb://u:p@example.test/app", Some("app")),
        ];
        let ids: Vec<String> = drafts.iter().map(|d| d.id.clone()).collect();
        for draft in drafts {
            admin.add(draft).expect("add source");
        }

        for id in &ids {
            let copy_id = format!("{id}-copy");
            let copy = admin
                .duplicate(id, copy_id.clone(), format!("{id} copy"))
                .unwrap_or_else(|err| panic!("duplicate {id}: {err}"));
            for key_ref in refs_of(copy) {
                assert_eq!(
                    ref_owner(&key_ref),
                    Some(copy_id.as_str()),
                    "{id}: {key_ref} was carried over instead of re-minted"
                );
            }
        }
        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn duplicate_of_a_local_turso_touches_no_secret_at_all() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(turso_draft("local", "Local", "/tmp/local.db"))
            .expect("add source");

        let copy = admin
            .duplicate("local", "copy".to_string(), "Copy".to_string())
            .expect("duplicate");

        assert_eq!(refs_of(copy), Vec::<String>::new());
    }

    #[test]
    fn duplicate_onto_a_taken_id_is_refused() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("prod", "postgres://u:p@example.test/app"))
            .expect("add source");
        admin
            .add(pg_draft("staging", "postgres://u:p@example.test/stg"))
            .expect("add other");

        let err = admin
            .duplicate("prod", "staging".to_string(), "Copy".to_string())
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::DuplicateId(id) if id == "staging"));
        // The refusal happened before any keyring write, so staging's own slot
        // still holds staging's own url.
        assert_eq!(admin.entries().len(), 2);
    }

    #[test]
    fn duplicate_onto_an_id_that_shadows_an_alias_is_refused() {
        // The id is a handle an agent hands back, and the resolver tries
        // aliases first (ADR-0088).
        let (_dir, _secrets, mut admin) = fresh_admin();
        let mut other = pg_draft("staging", "postgres://u:p@example.test/stg");
        other.mcp_alias = Some("reporting".to_string());
        admin.add(other).expect("add other");
        admin
            .add(pg_draft("prod", "postgres://u:p@example.test/app"))
            .expect("add source");

        let err = admin
            .duplicate("prod", "reporting".to_string(), "Copy".to_string())
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::DuplicateAlias(a) if a == "reporting"));
    }

    #[test]
    fn duplicate_of_an_unknown_id_is_not_found() {
        let (_dir, _secrets, mut admin) = fresh_admin();
        let err = admin
            .duplicate("nope", "copy".to_string(), "Copy".to_string())
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::NotFound(id) if id == "nope"));
    }

    #[test]
    fn duplicate_refuses_a_source_that_points_at_another_connections_slot() {
        // Copying it would read alpha's credential to seed a third entry, which
        // is precisely what the import guard (ADR-0038) refuses. Repair first.
        let (_dir, _secrets, mut admin) = store_with_a_foreign_ref();

        let err = admin
            .duplicate("beta", "gamma".to_string(), "Gamma".to_string())
            .expect_err("must fail");
        assert!(
            matches!(&err, ConfigError::UnusableSourceRef { id, key_ref }
                if id == "beta" && key_ref == "dbboard.alpha.url"),
            "got {err:?}"
        );
        assert_eq!(admin.entries().len(), 2);
    }

    // ---- repair ---------------------------------------------------------

    #[test]
    fn repair_points_the_entry_at_its_own_slot_and_stores_the_supplied_secret() {
        let (_dir, secrets, mut admin) = store_with_a_foreign_ref();

        let repaired = admin
            .repair_foreign_ref(
                "beta",
                "dbboard.alpha.url",
                "postgres://b@example.test/db".to_string(),
            )
            .expect("repair");

        assert_eq!(refs_of(repaired), vec!["dbboard.beta.url"]);
        assert_eq!(
            secrets.get("dbboard.beta.url").expect("beta url"),
            "postgres://b@example.test/db"
        );
        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn repair_never_touches_the_slot_it_stopped_referencing() {
        // It is a live credential with a live owner. `delete` and `update`
        // purge orphans; this one has an owner, so it is not an orphan.
        let (_dir, secrets, mut admin) = store_with_a_foreign_ref();

        admin
            .repair_foreign_ref("beta", "dbboard.alpha.url", "beta-url".to_string())
            .expect("repair");

        assert_eq!(
            secrets.get("dbboard.alpha.url").expect("alpha url intact"),
            "postgres://a@example.test/db"
        );
        let alpha = admin
            .entries()
            .iter()
            .find(|e| e.id == "alpha")
            .expect("alpha still there");
        assert_eq!(refs_of(alpha), vec!["dbboard.alpha.url"]);
    }

    #[test]
    fn repair_persists_so_a_reload_sees_the_new_slot() {
        let (dir, secrets, mut admin) = store_with_a_foreign_ref();
        admin
            .repair_foreign_ref("beta", "dbboard.alpha.url", "beta-url".to_string())
            .expect("repair");

        let reopened = ConnectionAdmin::open(
            dir.path().join("connections.toml"),
            secrets as Arc<dyn SecretStore>,
        )
        .expect("reopen");
        assert_eq!(reopened.foreign_refs(), Vec::new());
    }

    #[test]
    fn repair_of_a_slot_the_entry_does_not_carry_is_refused() {
        let (_dir, _secrets, mut admin) = store_with_a_foreign_ref();

        let err = admin
            .repair_foreign_ref("beta", "dbboard.alpha.token", "x".to_string())
            .expect_err("must fail");
        assert!(
            matches!(&err, ConfigError::RefNotOnEntry { id, key_ref }
                if id == "beta" && key_ref == "dbboard.alpha.token"),
            "got {err:?}"
        );
    }

    #[test]
    fn repair_of_a_slot_the_entry_already_owns_is_refused() {
        // Nothing to repair, and re-minting would be a no-op that overwrote a
        // working credential with whatever the caller happened to pass.
        let (_dir, secrets, mut admin) = fresh_admin();
        admin
            .add(pg_draft("prod", "postgres://u:p@example.test/app"))
            .expect("add");

        let err = admin
            .repair_foreign_ref("prod", "dbboard.prod.url", "overwrite".to_string())
            .expect_err("must fail");
        assert!(
            matches!(&err, ConfigError::NothingToRepair { id, key_ref }
                if id == "prod" && key_ref == "dbboard.prod.url"),
            "got {err:?}"
        );
        assert_eq!(
            secrets.get("dbboard.prod.url").expect("url untouched"),
            "postgres://u:p@example.test/app"
        );
    }

    #[test]
    fn repair_of_a_ref_that_names_no_owner_is_refused() {
        // `foreign_refs` deliberately skips this shape because it carries no
        // owner to name; it also carries no field to re-mint from.
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
        let mut admin = ConnectionAdmin::new_with_file(
            dir.path().join("connections.toml"),
            secrets as Arc<dyn SecretStore>,
            file,
        );

        let err = admin
            .repair_foreign_ref("beta", "legacy-url", "x".to_string())
            .expect_err("must fail");
        assert!(
            matches!(&err, ConfigError::NothingToRepair { id, key_ref }
                if id == "beta" && key_ref == "legacy-url"),
            "got {err:?}"
        );
    }

    #[test]
    fn repair_of_an_unknown_id_is_not_found() {
        let (_dir, _secrets, mut admin) = store_with_a_foreign_ref();
        let err = admin
            .repair_foreign_ref("nope", "dbboard.alpha.url", "x".to_string())
            .expect_err("must fail");
        assert!(matches!(err, ConfigError::NotFound(id) if id == "nope"));
    }

    #[test]
    fn repair_fixes_an_ssh_slot_without_disturbing_the_url_slot() {
        // An entry can carry two slots. Repairing one must leave the other
        // alone, including when only the tunnel was hand-edited.
        let dir = tempdir().expect("tempdir");
        let secrets = Arc::new(InMemorySecretStore::new());
        let mut file = ConnectionFile::empty();
        file.connections.push(ConnectionEntry {
            mcp_alias: None,
            mcp_write: false,
            ssh: Some(SshTunnelToml {
                host: "bastion.example.test".to_string(),
                port: 22,
                user: "tunnel".to_string(),
                key_path: None,
                keyring_passphrase_ref: None,
                keyring_password_ref: Some("dbboard.alpha.ssh_password".to_string()),
                fingerprint: Some("SHA256:abc".to_string()),
                known_hosts: None,
            }),
            id: "beta".to_string(),
            name: "Beta".to_string(),
            kind: ConnectionKind::Postgres {
                keyring_url_ref: "dbboard.beta.url".to_string(),
            },
        });
        let mut admin = ConnectionAdmin::new_with_file(
            dir.path().join("connections.toml"),
            secrets.clone() as Arc<dyn SecretStore>,
            file,
        );

        let repaired = admin
            .repair_foreign_ref("beta", "dbboard.alpha.ssh_password", "pw".to_string())
            .expect("repair");

        assert_eq!(
            repaired
                .ssh
                .as_ref()
                .and_then(|ssh| ssh.keyring_password_ref.as_deref()),
            Some("dbboard.beta.ssh_password")
        );
        assert_eq!(refs_of(repaired)[0], "dbboard.beta.url");
        assert_eq!(secrets.get("dbboard.beta.ssh_password").expect("pw"), "pw");
        assert_eq!(admin.foreign_refs(), Vec::new());
    }

    #[test]
    fn a_repaired_entry_can_then_be_duplicated() {
        // The two halves of #213 in sequence: the reporter's store could
        // neither be fixed nor extended, and fixing it must leave it extendable.
        let (_dir, secrets, mut admin) = store_with_a_foreign_ref();
        admin
            .repair_foreign_ref("beta", "dbboard.alpha.url", "beta-url".to_string())
            .expect("repair");

        let copy = admin
            .duplicate("beta", "beta-2".to_string(), "Beta 2".to_string())
            .expect("duplicate");

        assert_eq!(refs_of(copy), vec!["dbboard.beta-2.url"]);
        assert_eq!(secrets.get("dbboard.beta-2.url").expect("copy"), "beta-url");
        assert_eq!(admin.foreign_refs(), Vec::new());
    }
}
