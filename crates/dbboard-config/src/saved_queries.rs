//! On-disk shape + admin API for saved queries (ADR-0147).
//!
//! Sibling to [`crate::annotations`]: same `ProjectDirs` config dir, same
//! `secure_fs` at-rest posture (Unix `0o600` / Windows inherited DACL), same
//! parse-and-validate / `load_or_empty` / `save_atomic` shape.
//!
//! **Why not the SQL history.** The editor already remembers what was run,
//! in the webview's `localStorage`, capped and de-duplicated (ADR-0017). That
//! is the right home for something the tool records on your behalf and the
//! wrong one for something you deliberately kept: `localStorage` is cleared
//! by "clear site data", is not visible to a backup, and does not survive
//! moving to another machine. A saved query is an artefact the operator made
//! on purpose, so it goes in the config dir with the rest of the profile.
//!
//! Queries are anchored by connection **id** — the stable primary key from
//! `connections.toml`, never the display name — so renaming a connection
//! keeps its queries, exactly as annotations do.
//!
//! Nothing here touches [`crate::secrets`]. A saved query is SQL text the
//! operator typed; if they paste a credential into it, it lands in this file
//! like it would land in any other note, which is why the file is written
//! user-only.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secure_fs;

/// The single TOML schema version this build understands. Bumping it will
/// come with an explicit migration; until then an unknown version is a hard
/// error rather than a silent round-trip.
pub const SAVED_QUERIES_VERSION: u32 = 1;

/// Top-level shape of `saved-queries.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedQueriesFile {
    pub version: u32,
    /// One entry per connection that has at least one saved query.
    /// Connections with none are pruned, so an untouched install has an
    /// empty vec (and, lazily, no file at all).
    #[serde(default)]
    pub connections: Vec<ConnectionQueries>,
}

/// Every saved query for one connection, anchored by connection id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionQueries {
    pub id: String,
    #[serde(default)]
    pub queries: Vec<SavedQuery>,
}

/// One saved query: the name the operator gave it, the SQL, and when it was
/// last written.
///
/// `saved_at` is epoch milliseconds. It exists so a caller can offer
/// "most recent first" without the file having to be stored in that order —
/// the file keeps insertion order, which makes its diffs readable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedQuery {
    pub name: String,
    pub sql: String,
    pub saved_at: i64,
}

impl SavedQueriesFile {
    /// Parse and validate a `saved-queries.toml` payload.
    ///
    /// # Errors
    ///
    /// - [`SavedQueryError::Parse`] if the TOML is malformed.
    /// - [`SavedQueryError::UnsupportedVersion`] if `version` is not
    ///   [`SAVED_QUERIES_VERSION`].
    /// - [`SavedQueryError::DuplicateConnectionId`] if two connection entries
    ///   share an id, or [`SavedQueryError::DuplicateName`] if two queries
    ///   within one connection share a name — surfaced loudly rather than
    ///   letting one shadow the other, because the shadowed one is a query
    ///   somebody meant to keep.
    pub fn parse(input: &str) -> Result<Self, SavedQueryError> {
        let file: SavedQueriesFile = toml::from_str(input)?;
        if file.version != SAVED_QUERIES_VERSION {
            return Err(SavedQueryError::UnsupportedVersion(file.version));
        }
        let mut seen_conn = std::collections::HashSet::with_capacity(file.connections.len());
        for conn in &file.connections {
            if !seen_conn.insert(conn.id.as_str()) {
                return Err(SavedQueryError::DuplicateConnectionId(conn.id.clone()));
            }
            let mut seen_name = std::collections::HashSet::with_capacity(conn.queries.len());
            for query in &conn.queries {
                if !seen_name.insert(query.name.as_str()) {
                    return Err(SavedQueryError::DuplicateName {
                        connection: conn.id.clone(),
                        name: query.name.clone(),
                    });
                }
            }
        }
        Ok(file)
    }

    /// An empty store at the current schema version.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            version: SAVED_QUERIES_VERSION,
            connections: Vec::new(),
        }
    }
}

/// The default per-user path for `saved-queries.toml`, resolved via the same
/// `directories` lookup as the other per-user stores so it lives next to
/// `connections.toml` / `annotations.toml` / `history.jsonl`.
///
/// # Errors
///
/// [`SavedQueryError::NoConfigDir`] when the OS reports no usable per-user
/// config directory.
pub fn default_saved_queries_path() -> Result<PathBuf, SavedQueryError> {
    // Through `store::config_dir` rather than its own `ProjectDirs` lookup, so
    // that `DBBOARD_CONFIG_DIR` moves this file with the rest of the profile.
    let dir = crate::store::config_dir().map_err(|_| SavedQueryError::NoConfigDir)?;
    Ok(dir.join("saved-queries.toml"))
}

/// Read and parse `saved-queries.toml` at `path`. A missing file is **not**
/// an error: it yields an empty store, created lazily on the first save.
///
/// # Errors
///
/// - [`SavedQueryError::Io`] for non-`NotFound` I/O failures.
/// - Any validation error from [`SavedQueriesFile::parse`].
pub fn load_or_empty(path: &Path) -> Result<SavedQueriesFile, SavedQueryError> {
    match fs::read_to_string(path) {
        Ok(contents) => SavedQueriesFile::parse(&contents),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(SavedQueriesFile::empty()),
        Err(err) => Err(SavedQueryError::Io(err)),
    }
}

/// Write `file` to `path` atomically (sibling `*.tmp` via
/// [`secure_fs::create_new_user_only`] → `rename`), matching the other
/// stores' at-rest posture (ADR-0024).
///
/// # Errors
///
/// - [`SavedQueryError::Serialize`] if re-serializing to TOML fails.
/// - [`SavedQueryError::Io`] for any filesystem failure.
pub fn save_atomic(path: &Path, file: &SavedQueriesFile) -> Result<(), SavedQueryError> {
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
        return Err(SavedQueryError::Io(err));
    }
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".saved-queries.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), SavedQueryError> {
    // A leftover `.tmp` from a killed process must not make the next save
    // fail forever, so it is removed before the exclusive create.
    let _ = fs::remove_file(path);
    let mut file = secure_fs::create_new_user_only(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Read/write access to the saved-query store, owning the file it edits.
///
/// Mirrors [`crate::annotations::AnnotationsAdmin`]: every mutation writes
/// the whole file atomically, and the in-memory copy is only updated once
/// that write succeeded — a failed save leaves the admin exactly as it was.
#[derive(Debug)]
pub struct SavedQueryAdmin {
    path: PathBuf,
    file: SavedQueriesFile,
}

impl SavedQueryAdmin {
    /// Open the store at the default per-user path, loading existing queries
    /// (or starting empty if the file does not exist yet).
    ///
    /// # Errors
    ///
    /// [`SavedQueryError::NoConfigDir`] if no per-user config dir resolves,
    /// or any error from [`load_or_empty`].
    pub fn open_default() -> Result<Self, SavedQueryError> {
        let path = default_saved_queries_path()?;
        Self::new_with_file(path)
    }

    /// Open the store at an explicit path. Used by tests and by any caller
    /// that resolves the path itself.
    ///
    /// # Errors
    ///
    /// Any error from [`load_or_empty`].
    pub fn new_with_file(path: PathBuf) -> Result<Self, SavedQueryError> {
        let file = load_or_empty(&path)?;
        Ok(Self { path, file })
    }

    /// Every saved query for `connection`, in file order (oldest first).
    #[must_use]
    pub fn queries(&self, connection: &str) -> &[SavedQuery] {
        self.file
            .connections
            .iter()
            .find(|c| c.id == connection)
            .map_or(&[], |c| c.queries.as_slice())
    }

    /// Save `sql` under `name`, refusing to replace an existing name.
    ///
    /// The refusal is the point: overwriting is how a saved query is lost,
    /// and only the caller knows whether the operator meant to replace one.
    /// Confirm with them, then call [`Self::replace`].
    ///
    /// # Errors
    ///
    /// - [`SavedQueryError::EmptyName`] / [`SavedQueryError::EmptySql`] when
    ///   either side is blank once trimmed.
    /// - [`SavedQueryError::DuplicateName`] if `name` is already taken for
    ///   this connection.
    /// - [`SavedQueryError::Serialize`] / [`SavedQueryError::Io`] if the
    ///   atomic save fails; `self` is left unchanged in that case.
    pub fn add(
        &mut self,
        connection: &str,
        name: &str,
        sql: &str,
        at: i64,
    ) -> Result<(), SavedQueryError> {
        let (name, sql) = validated(name, sql)?;
        if self.queries(connection).iter().any(|q| q.name == name) {
            return Err(SavedQueryError::DuplicateName {
                connection: connection.to_owned(),
                name,
            });
        }
        self.write_query(connection, name, sql, at)
    }

    /// Save `sql` under `name`, replacing an existing query of that name and
    /// keeping its position in the list.
    ///
    /// # Errors
    ///
    /// As [`Self::add`], minus [`SavedQueryError::DuplicateName`] — replacing
    /// is what this one is for.
    pub fn replace(
        &mut self,
        connection: &str,
        name: &str,
        sql: &str,
        at: i64,
    ) -> Result<(), SavedQueryError> {
        let (name, sql) = validated(name, sql)?;
        self.write_query(connection, name, sql, at)
    }

    /// Delete the query named `name`. Returns whether one was there.
    ///
    /// # Errors
    ///
    /// [`SavedQueryError::Serialize`] / [`SavedQueryError::Io`] if the atomic
    /// save fails; `self` is left unchanged in that case.
    pub fn remove(&mut self, connection: &str, name: &str) -> Result<bool, SavedQueryError> {
        let mut next = self.file.clone();
        let Some(entry) = next.connections.iter_mut().find(|c| c.id == connection) else {
            return Ok(false);
        };
        let before = entry.queries.len();
        entry.queries.retain(|q| q.name != name);
        if entry.queries.len() == before {
            return Ok(false);
        }
        // Prune the connection stanza when its last query goes, so the file
        // never accumulates empty containers.
        next.connections.retain(|c| !c.queries.is_empty());
        save_atomic(&self.path, &next)?;
        self.file = next;
        Ok(true)
    }

    fn write_query(
        &mut self,
        connection: &str,
        name: String,
        sql: String,
        at: i64,
    ) -> Result<(), SavedQueryError> {
        let mut next = self.file.clone();
        if !next.connections.iter().any(|c| c.id == connection) {
            next.connections.push(ConnectionQueries {
                id: connection.to_owned(),
                queries: Vec::new(),
            });
        }
        let entry = next
            .connections
            .iter_mut()
            .find(|c| c.id == connection)
            .expect("the connection entry exists or was just pushed");
        let query = SavedQuery {
            name,
            sql,
            saved_at: at,
        };
        match entry.queries.iter_mut().find(|q| q.name == query.name) {
            Some(existing) => *existing = query,
            None => entry.queries.push(query),
        }
        save_atomic(&self.path, &next)?;
        self.file = next;
        Ok(())
    }
}

/// Trim both sides and refuse a blank one.
///
/// A blank name gives the operator a query they cannot pick out of a list; a
/// blank body gives them a saved query that does nothing. Both are the kind
/// of empty a UI can produce by accident, so the store refuses rather than
/// storing something useless.
fn validated(name: &str, sql: &str) -> Result<(String, String), SavedQueryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SavedQueryError::EmptyName);
    }
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(SavedQueryError::EmptySql);
    }
    Ok((name.to_owned(), sql.to_owned()))
}

/// Everything that can go wrong reading or writing `saved-queries.toml`.
#[derive(Debug, Error)]
pub enum SavedQueryError {
    /// The TOML payload could not be parsed at all.
    #[error("saved queries parse failed: {0}")]
    Parse(#[from] toml::de::Error),

    /// `version` does not equal [`SAVED_QUERIES_VERSION`].
    #[error("unsupported saved queries version: {0} (only version {expected} is supported)", expected = SAVED_QUERIES_VERSION)]
    UnsupportedVersion(u32),

    /// Two connection entries share an id.
    #[error("duplicate saved queries connection id: {0}")]
    DuplicateConnectionId(String),

    /// Two queries within one connection share a name.
    #[error("a saved query named {name} already exists for connection {connection}")]
    DuplicateName { connection: String, name: String },

    /// The name was blank once trimmed.
    #[error("a saved query needs a name")]
    EmptyName,

    /// The SQL was blank once trimmed.
    #[error("a saved query needs a statement")]
    EmptySql,

    /// Filesystem read or write failed. The path is not embedded so the
    /// message is safe to log; callers attach the path when they have it.
    #[error("saved queries io failed: {0}")]
    Io(#[from] std::io::Error),

    /// Re-serializing the in-memory store back to TOML failed.
    #[error("saved queries serialize failed: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// The OS reported no usable per-user config directory.
    #[error("could not resolve a per-user config directory")]
    NoConfigDir,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn admin_in(dir: &TempDir) -> SavedQueryAdmin {
        let path = dir.path().join("saved-queries.toml");
        SavedQueryAdmin::new_with_file(path).expect("open empty")
    }

    #[test]
    fn a_missing_file_is_an_empty_store_not_an_error() {
        // The file is created on the first save. An install that has never
        // saved a query must not greet the operator with a read error.
        let dir = TempDir::new().expect("tempdir");
        let admin = admin_in(&dir);
        assert!(admin.queries("conn-1").is_empty());
    }

    #[test]
    fn a_saved_query_survives_a_reopen() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("saved-queries.toml");
        let mut admin = SavedQueryAdmin::new_with_file(path.clone()).expect("open");
        admin
            .add(
                "conn-1",
                "daily orders",
                "SELECT * FROM orders",
                1_700_000_000_000,
            )
            .expect("add");

        let reopened = SavedQueryAdmin::new_with_file(path).expect("reopen");
        let queries = reopened.queries("conn-1");
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].name, "daily orders");
        assert_eq!(queries[0].sql, "SELECT * FROM orders");
        assert_eq!(queries[0].saved_at, 1_700_000_000_000);
    }

    #[test]
    fn queries_are_scoped_to_their_connection() {
        // Anchored by connection id: a query written against one database's
        // tables must not appear while another connection is open.
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin.add("conn-1", "a", "SELECT 1", 1).expect("add");
        assert_eq!(admin.queries("conn-1").len(), 1);
        assert!(admin.queries("conn-2").is_empty());
    }

    #[test]
    fn add_refuses_to_replace_an_existing_name() {
        // Overwriting is how a saved query is lost. Only the caller knows
        // whether the operator meant to, so the store refuses and lets them
        // ask.
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin.add("conn-1", "report", "SELECT 1", 1).expect("add");

        let err = admin
            .add("conn-1", "report", "SELECT 2", 2)
            .expect_err("second add must be refused");
        assert!(matches!(err, SavedQueryError::DuplicateName { .. }));
        assert_eq!(admin.queries("conn-1")[0].sql, "SELECT 1");
    }

    #[test]
    fn replace_updates_in_place_and_keeps_the_position() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin.add("conn-1", "first", "SELECT 1", 1).expect("add");
        admin.add("conn-1", "second", "SELECT 2", 2).expect("add");

        admin
            .replace("conn-1", "first", "SELECT 3", 9)
            .expect("replace");

        let queries = admin.queries("conn-1");
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].name, "first");
        assert_eq!(queries[0].sql, "SELECT 3");
        assert_eq!(queries[0].saved_at, 9);
        assert_eq!(queries[1].name, "second");
    }

    #[test]
    fn a_blank_name_or_statement_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        assert!(matches!(
            admin.add("conn-1", "   ", "SELECT 1", 1),
            Err(SavedQueryError::EmptyName)
        ));
        assert!(matches!(
            admin.add("conn-1", "name", "  \n ", 1),
            Err(SavedQueryError::EmptySql)
        ));
        assert!(admin.queries("conn-1").is_empty());
    }

    #[test]
    fn name_and_statement_are_stored_trimmed() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin
            .add("conn-1", "  report  ", "  SELECT 1  ", 1)
            .expect("add");
        assert_eq!(admin.queries("conn-1")[0].name, "report");
        assert_eq!(admin.queries("conn-1")[0].sql, "SELECT 1");
    }

    #[test]
    fn removing_the_last_query_prunes_the_connection_stanza() {
        // Otherwise the file accumulates empty containers for every
        // connection that ever held a query.
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("saved-queries.toml");
        let mut admin = SavedQueryAdmin::new_with_file(path.clone()).expect("open");
        admin.add("conn-1", "only", "SELECT 1", 1).expect("add");

        assert!(admin.remove("conn-1", "only").expect("remove"));

        let contents = fs::read_to_string(&path).expect("read back");
        let file = SavedQueriesFile::parse(&contents).expect("parse");
        assert!(file.connections.is_empty());
    }

    #[test]
    fn removing_something_that_is_not_there_is_not_an_error() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        assert!(!admin.remove("conn-1", "absent").expect("remove"));
        admin.add("conn-1", "present", "SELECT 1", 1).expect("add");
        assert!(!admin.remove("conn-1", "absent").expect("remove"));
        assert_eq!(admin.queries("conn-1").len(), 1);
    }

    #[test]
    fn a_duplicate_name_in_the_file_is_refused_loudly() {
        // The shadowed entry is a query somebody meant to keep. Failing to
        // load says so; silently dropping one does not.
        let input = r#"
version = 1

[[connections]]
id = "conn-1"

[[connections.queries]]
name = "report"
sql = "SELECT 1"
saved_at = 1

[[connections.queries]]
name = "report"
sql = "SELECT 2"
saved_at = 2
"#;
        let err = SavedQueriesFile::parse(input).expect_err("must refuse");
        assert!(matches!(err, SavedQueryError::DuplicateName { .. }));
    }

    #[test]
    fn an_unknown_version_is_refused_rather_than_round_tripped() {
        // Round-tripping a future file through this build would drop whatever
        // that version added — a silent downgrade of the operator's data.
        let input = "version = 2\n";
        let err = SavedQueriesFile::parse(input).expect_err("must refuse");
        assert!(matches!(err, SavedQueryError::UnsupportedVersion(2)));
    }

    #[test]
    fn a_duplicate_connection_id_is_refused() {
        let input = r#"
version = 1

[[connections]]
id = "conn-1"

[[connections]]
id = "conn-1"
"#;
        let err = SavedQueriesFile::parse(input).expect_err("must refuse");
        assert!(matches!(err, SavedQueryError::DuplicateConnectionId(_)));
    }

    #[test]
    fn a_failed_save_leaves_the_admin_unchanged() {
        // The in-memory copy is only advanced once the write succeeded, so a
        // full disk cannot leave the app showing a query that is not on disk.
        let dir = TempDir::new().expect("tempdir");
        // The store writes a sibling `.tmp` and renames it. A directory
        // sitting on that name fails the create on every platform, while the
        // store file itself is still absent — so the load succeeds and only
        // the write fails, which is the case under test.
        fs::create_dir(dir.path().join("saved-queries.toml.tmp")).expect("occupy the tmp name");
        let path = dir.path().join("saved-queries.toml");
        let mut admin = SavedQueryAdmin::new_with_file(path).expect("open");

        assert!(admin.add("conn-1", "report", "SELECT 1", 1).is_err());
        assert!(admin.queries("conn-1").is_empty());
    }
}
