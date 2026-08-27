//! Handing an agent a bundle it cannot open (ADR-0140).
//!
//! Every other export in dbboard is a person at a keyboard choosing a
//! passphrase. This one has no person in it: an agent names the connections,
//! and the bundle still has to be sealed. So the passphrase is minted here,
//! written straight into the OS credential store, and never returned. What
//! comes back is a path and the name of the slot the passphrase went into —
//! the agent leaves holding a file it has no way to read, and the operator
//! collects the passphrase from the credential manager when they need it.
//!
//! That is the whole trick, and it is the only reason this verb is allowed
//! to exist. Credential handling belongs to the human; an export that
//! answered with a passphrase would have moved every secret in the store
//! into a transcript instead.
//!
//! Two further limits, both deliberate:
//!
//! - **The permission is a directory, not a flag.** `[mcp_export]` in
//!   `connections.toml` names where bundles may be written, and its absence
//!   is the off state. It is not an environment variable, because an agent
//!   usually owns the file that launches this server, and a gate the gated
//!   thing can open is not a gate (ADR-0087).
//! - **The selection is always explicit.** There is no "export everything"
//!   form. Naming the connections is what makes the request a record of
//!   what was collected.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbboard_config::secrets::SecretStore;
use dbboard_config::{generate_passphrase, secure_fs, ConfigError, ConnectionAdmin};
use serde::Serialize;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::service::ServiceError;

/// Extension the desktop app's import already recognises.
const BUNDLE_EXTENSION: &str = "dbbx";

/// Leading text of every generated file name, so a directory of these sorts
/// together and reads as one kind of thing.
const STEM_PREFIX: &str = "dbboard-export-";

/// Keyring namespace the minted passphrases live in. Distinct from the
/// `dbboard.<connection>.<field>` shape connection secrets use, so a bundle
/// slot can never be mistaken for — or collide with — a connection's.
const KEYRING_PREFIX: &str = "dbboard.export.";

/// How many names to try before giving up. Only reached when several exports
/// land in the same second, which takes an agent driving the verb in a loop.
const MAX_NAME_ATTEMPTS: u32 = 100;

/// What an export answers with.
///
/// Note what is *not* here: the passphrase, and the ids of the connections
/// sealed. The passphrase is the point of the design. The ids are left out
/// because an operator may have replaced them with neutral aliases
/// (ADR-0088), and a result that echoed the real ones would undo that.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExportOutcome {
    /// Absolute path of the sealed bundle.
    pub path: String,
    /// The OS credential store slot holding the passphrase. A name, never a
    /// value.
    pub passphrase_ref: String,
    /// How many connections went into the bundle. Confirms the count rather
    /// than the names — a caller that sent the same id twice learns so here.
    pub connection_count: usize,
    /// Things worth saying about the bundle that are not reasons to refuse
    /// it: a destination inside a syncing folder, entries whose keyring
    /// slots belong to another connection.
    pub warnings: Vec<String>,
}

/// Seal the connections named in `ids` into a bundle under a passphrase
/// nobody has seen.
///
/// `now` is passed in rather than read here so a test can put two exports in
/// the same second and watch the second one take another name.
///
/// The order of operations is chosen so that no unopenable file survives a
/// failure: the ciphertext is built first, the name is reserved second, and
/// the passphrase is stored before the bytes are written. A crash between
/// the last two leaves an empty file and a stored passphrase, which is
/// litter. The other order leaves a sealed bundle whose key was never
/// written down, which is a bundle nobody can ever open.
///
/// # Errors
///
/// [`ServiceError::ExportNotEnabled`] when no `[mcp_export]` permission is
/// set, [`ServiceError::Export`] for a missing destination or a failed
/// write, [`ServiceError::InvalidRequest`] for an empty selection or an id
/// that has left the store, and [`ServiceError::Config`] when the store or a
/// secret cannot be read.
pub fn export_named(
    config_path: &Path,
    secrets: &Arc<dyn SecretStore>,
    ids: &[String],
    now: OffsetDateTime,
) -> Result<ExportOutcome, ServiceError> {
    if ids.is_empty() {
        return Err(ServiceError::InvalidRequest(
            "name the connections to export; there is no form of this that exports \
             the whole store"
                .to_string(),
        ));
    }

    let admin = ConnectionAdmin::open(config_path.to_path_buf(), Arc::clone(secrets))?;
    let dir = admin
        .mcp_export()
        .map(|e| e.dir.clone())
        .ok_or(ServiceError::ExportNotEnabled)?;
    if !dir.is_dir() {
        // Not created on the operator's behalf. A directory that is not
        // there is usually a typo in the setting, and creating it would put
        // a file full of credentials somewhere nobody is looking.
        return Err(ServiceError::Export(format!(
            "the configured export directory {} does not exist; a human must \
             create it or correct mcp_export.dir in connections.toml",
            dir.display()
        )));
    }

    let mut warnings = Vec::new();
    if let Some(provider) = secure_fs::is_likely_cloud_synced_path(&dir) {
        warnings.push(format!(
            "the export directory looks like it is inside {provider}; the bundle \
             will be uploaded there as soon as it is written"
        ));
    }
    let foreign = admin.foreign_refs_of(ids).map_err(map_config_error)?;
    if !foreign.is_empty() {
        // Counted, not named: the names are the ones an alias may be hiding.
        warnings.push(format!(
            "{} of the selected connections carry a keyring slot minted for a \
             different connection; repair them in the dbboard app before relying \
             on this bundle",
            foreign.len()
        ));
    }

    let passphrase = generate_passphrase();
    let blob = admin
        .export_bundle_of(ids, &passphrase)
        .map_err(map_config_error)?;
    let connection_count = admin
        .entries()
        .iter()
        .filter(|e| ids.iter().any(|id| id == &e.id))
        .count();

    let stamp = now
        .format(&format_description!(
            "[year][month][day]-[hour][minute][second]Z"
        ))
        .map_err(|e| ServiceError::Export(format!("could not name the bundle: {e}")))?;
    let (path, stem, mut file) = reserve(&dir, &format!("{STEM_PREFIX}{stamp}"))?;
    let passphrase_ref = format!("{KEYRING_PREFIX}{stem}");

    if let Err(err) = secrets.set(&passphrase_ref, &passphrase) {
        let _ = std::fs::remove_file(&path);
        return Err(ServiceError::Export(format!(
            "the bundle passphrase could not be stored in the OS credential \
             store, so nothing was written: {err}"
        )));
    }
    if let Err(err) = file.write_all(&blob).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&path);
        return Err(ServiceError::Export(format!(
            "writing the bundle to {} failed: {err}",
            path.display()
        )));
    }

    Ok(ExportOutcome {
        path: path.display().to_string(),
        passphrase_ref,
        connection_count,
        warnings,
    })
}

/// Take an unused name in `dir`, creating the file as we go so two exports
/// racing each other cannot pick the same one.
fn reserve(dir: &Path, stem_base: &str) -> Result<(PathBuf, String, std::fs::File), ServiceError> {
    for attempt in 0..MAX_NAME_ATTEMPTS {
        let stem = if attempt == 0 {
            stem_base.to_string()
        } else {
            format!("{stem_base}-{}", attempt + 1)
        };
        let path = dir.join(format!("{stem}.{BUNDLE_EXTENSION}"));
        match secure_fs::create_new_user_only(&path) {
            Ok(file) => return Ok((path, stem, file)),
            // Taken by another export in the same second. Try the next name.
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(ServiceError::Export(format!(
                    "could not create {}: {err}",
                    path.display()
                )))
            }
        }
    }
    Err(ServiceError::Export(format!(
        "could not find an unused name in {} after {MAX_NAME_ATTEMPTS} tries",
        dir.display()
    )))
}

/// Route the store layer's refusals to the error classes an agent can act on.
///
/// Neither named case echoes an id back. The caller resolved its handles
/// against the store moments ago, so an id missing now is a race rather than
/// a typo — and the id it would name may be one an alias is hiding.
fn map_config_error(err: ConfigError) -> ServiceError {
    match err {
        ConfigError::EmptySelection => ServiceError::InvalidRequest(
            "the selection came out empty; name at least one connection".to_string(),
        ),
        ConfigError::NotFound(_) => ServiceError::InvalidRequest(
            "one of the connections named is no longer in the store; list the \
             connections again and retry"
                .to_string(),
        ),
        other => ServiceError::Config(other),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use dbboard_config::secrets::SecretStore;
    use dbboard_config::{decrypt_bundle, InMemorySecretStore};
    use tempfile::TempDir;
    use time::OffsetDateTime;

    use super::{export_named, ExportOutcome};
    use crate::service::ServiceError;

    /// A store holding two credential-free connections, so an export needs no
    /// keyring reads and each test is about the export and nothing else.
    fn store(dir: &TempDir, export_dir: Option<&Path>) -> PathBuf {
        let permission = export_dir.map_or_else(String::new, |d| {
            format!("[mcp_export]\ndir = {:?}\n\n", d.display().to_string())
        });
        let path = dir.path().join("connections.toml");
        std::fs::write(
            &path,
            format!(
                "version = 1\n\n{permission}\
                 [[connections]]\n\
                 id = \"one\"\nname = \"One\"\nkind = \"turso\"\npath = \"one.db\"\n\n\
                 [[connections]]\n\
                 id = \"two\"\nname = \"Two\"\nkind = \"turso\"\npath = \"two.db\"\n"
            ),
        )
        .expect("write the store");
        path
    }

    fn at(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).expect("a valid timestamp")
    }

    fn run(
        config: &Path,
        secrets: &Arc<dyn SecretStore>,
        ids: &[&str],
        now: i64,
    ) -> Result<ExportOutcome, ServiceError> {
        let ids: Vec<String> = ids.iter().map(|s| (*s).to_string()).collect();
        export_named(config, secrets, &ids, at(now))
    }

    fn secrets() -> Arc<dyn SecretStore> {
        Arc::new(InMemorySecretStore::new())
    }

    #[test]
    fn a_store_without_the_permission_refuses() {
        let home = TempDir::new().expect("tempdir");
        let config = store(&home, None);
        let err = run(&config, &secrets(), &["one"], 1_800_000_000).expect_err("must refuse");
        assert!(
            matches!(err, ServiceError::ExportNotEnabled),
            "expected the missing permission to be the reason, got {err}"
        );
    }

    #[test]
    fn a_configured_directory_that_is_not_there_refuses_rather_than_creating_it() {
        let home = TempDir::new().expect("tempdir");
        let missing = home.path().join("nowhere");
        let config = store(&home, Some(&missing));
        let err = run(&config, &secrets(), &["one"], 1_800_000_000).expect_err("must refuse");
        assert!(matches!(err, ServiceError::Export(_)), "got {err}");
        assert!(
            !missing.exists(),
            "the directory must not have been created"
        );
    }

    #[test]
    fn an_empty_selection_refuses_before_the_store_is_even_read() {
        let home = TempDir::new().expect("tempdir");
        // No permission set: were the empty selection checked second, this
        // would come back as ExportNotEnabled instead.
        let config = store(&home, None);
        let err = run(&config, &secrets(), &[], 1_800_000_000).expect_err("must refuse");
        match err {
            ServiceError::InvalidRequest(msg) => {
                assert!(msg.contains("whole store"), "unhelpful message: {msg}");
            }
            other => panic!("expected InvalidRequest, got {other}"),
        }
    }

    #[test]
    fn the_passphrase_reaches_the_keyring_and_never_the_caller() {
        let home = TempDir::new().expect("tempdir");
        let out = home.path().join("exports");
        std::fs::create_dir(&out).expect("mkdir");
        let config = store(&home, Some(&out));
        let secrets = secrets();

        let outcome = run(&config, &secrets, &["one"], 1_800_000_000).expect("export");
        assert_eq!(outcome.connection_count, 1);

        let blob = std::fs::read(&outcome.path).expect("the bundle was written");
        let passphrase = secrets
            .get(&outcome.passphrase_ref)
            .expect("the passphrase was stored");
        assert!(!passphrase.is_empty());

        // No field of the answer is the passphrase, however it is rendered.
        let rendered = serde_json::to_string(&outcome).expect("serialize");
        assert!(
            !rendered.contains(&passphrase),
            "the passphrase leaked into the tool result: {rendered}"
        );

        // And the slot really does open the file.
        let payload = decrypt_bundle(&blob, &passphrase).expect("decrypt");
        let ids: Vec<&str> = payload
            .connections
            .connections
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, vec!["one"], "only the named connection is sealed");
    }

    #[test]
    fn a_second_export_in_the_same_second_takes_another_name() {
        let home = TempDir::new().expect("tempdir");
        let out = home.path().join("exports");
        std::fs::create_dir(&out).expect("mkdir");
        let config = store(&home, Some(&out));
        let secrets = secrets();

        let first = run(&config, &secrets, &["one"], 1_800_000_000).expect("first");
        let second = run(&config, &secrets, &["two"], 1_800_000_000).expect("second");
        assert_ne!(first.path, second.path, "the first bundle was overwritten");
        assert_ne!(
            first.passphrase_ref, second.passphrase_ref,
            "the second passphrase replaced the first in the keyring"
        );
        // Both must still open — which is the whole point of the name check.
        for outcome in [&first, &second] {
            let blob = std::fs::read(&outcome.path).expect("bundle");
            let pass = secrets.get(&outcome.passphrase_ref).expect("passphrase");
            decrypt_bundle(&blob, &pass).expect("decrypt");
        }
    }

    #[test]
    fn naming_the_same_connection_twice_seals_it_once() {
        let home = TempDir::new().expect("tempdir");
        let out = home.path().join("exports");
        std::fs::create_dir(&out).expect("mkdir");
        let config = store(&home, Some(&out));

        let outcome = run(&config, &secrets(), &["one", "one"], 1_800_000_000).expect("export");
        assert_eq!(
            outcome.connection_count, 1,
            "the count is of connections sealed, not of ids sent"
        );
    }
}
