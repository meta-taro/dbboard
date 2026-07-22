//! On-disk shape + admin API for local table/column annotations
//! (ADR-0045).
//!
//! Sibling to [`crate::ai_store`] / [`crate::store`]: same `ProjectDirs`
//! config dir, same `secure_fs` at-rest posture (Unix `0o600` / Windows
//! inherited DACL), same parse-and-validate / `load_or_empty` /
//! `save_atomic` shape.
//!
//! These are *documentation*, not schema and not secrets. SQLite / D1 /
//! libSQL have no first-class column-comment concept and offer no
//! extension that adds one, and even where a DB comment exists it may
//! demand write access the operator lacks — so dbboard keeps the notes
//! locally and never writes them to any database (ADR-0045 §Context).
//! Nothing here touches [`crate::secrets`]: annotations carry no secret,
//! so — unlike connections and AI providers — they are stored inline.
//!
//! Keys are strings, deliberately: this crate stays free of a
//! `dbboard-core` dependency, so the caller (which holds a `TableInfo`)
//! derives the schema-qualified table key via [`table_key`] and passes
//! it in. The `Vec`-of-structs layout (rather than nested maps) mirrors
//! `ai_store`'s `providers` and avoids TOML key-quoting for table names
//! that contain dots.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::secure_fs;

/// The single TOML schema version this build understands. Bumping it
/// will come with an explicit migration; until then an unknown version
/// is a hard error rather than a silent round-trip.
pub const ANNOTATIONS_VERSION: u32 = 1;

/// Top-level shape of `annotations.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnotationsFile {
    pub version: u32,
    /// One entry per connection that has at least one note. Connections
    /// with no notes are pruned, so an untouched install has an empty
    /// vec (and, lazily, no file at all).
    #[serde(default)]
    pub connections: Vec<ConnectionAnnotations>,
}

/// All notes for one connection, anchored by the connection **id** (the
/// stable primary key from `connections.toml`), never the display name —
/// renaming a connection keeps its notes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionAnnotations {
    pub id: String,
    #[serde(default)]
    pub tables: Vec<TableAnnotations>,
}

/// Notes for one table: an optional table-level note plus per-column
/// notes. `key` is schema-qualified where the engine has schemas
/// (`public.orders`) and the bare name where it does not
/// (SQLite/libSQL/D1) — build it with [`table_key`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableAnnotations {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub columns: Vec<ColumnAnnotation>,
}

/// A single column note. Only non-empty notes are stored; clearing a
/// note removes the entry (see [`AnnotationsAdmin::set_column_note`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnAnnotation {
    pub name: String,
    pub note: String,
}

/// Build the schema-qualified table key used to index annotations.
///
/// `Some("public")` + `"orders"` → `"public.orders"`; `None` (or an
/// empty schema) + `"orders"` → `"orders"`. Kept pure and
/// `dbboard-core`-free so the derivation is testable here and the caller
/// passes the result in.
#[must_use]
pub fn table_key(schema: Option<&str>, name: &str) -> String {
    match schema {
        Some(s) if !s.is_empty() => format!("{s}.{name}"),
        _ => name.to_string(),
    }
}

impl AnnotationsFile {
    /// Parse and validate an `annotations.toml` payload.
    ///
    /// # Errors
    ///
    /// - [`AnnotationsError::Parse`] if the TOML is malformed.
    /// - [`AnnotationsError::UnsupportedVersion`] if `version` is not
    ///   [`ANNOTATIONS_VERSION`].
    /// - [`AnnotationsError::DuplicateConnectionId`] if two connection
    ///   entries share an id, or [`AnnotationsError::DuplicateTableKey`]
    ///   if two table entries within one connection share a key —
    ///   surfaced loudly rather than letting one shadow the other.
    pub fn parse(input: &str) -> Result<Self, AnnotationsError> {
        let file: AnnotationsFile = toml::from_str(input)?;
        if file.version != ANNOTATIONS_VERSION {
            return Err(AnnotationsError::UnsupportedVersion(file.version));
        }
        let mut seen_conn = std::collections::HashSet::with_capacity(file.connections.len());
        for conn in &file.connections {
            if !seen_conn.insert(conn.id.as_str()) {
                return Err(AnnotationsError::DuplicateConnectionId(conn.id.clone()));
            }
            let mut seen_table = std::collections::HashSet::with_capacity(conn.tables.len());
            for table in &conn.tables {
                if !seen_table.insert(table.key.as_str()) {
                    return Err(AnnotationsError::DuplicateTableKey {
                        connection: conn.id.clone(),
                        key: table.key.clone(),
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
            version: ANNOTATIONS_VERSION,
            connections: Vec::new(),
        }
    }
}

/// The default per-user path for `annotations.toml`, resolved via the
/// same `directories` lookup as the other per-user stores so it lives
/// next to `connections.toml` / `ai-providers.toml` / `history.jsonl`.
///
/// # Errors
///
/// [`AnnotationsError::NoConfigDir`] when the OS reports no usable
/// per-user config directory.
pub fn default_annotations_path() -> Result<PathBuf, AnnotationsError> {
    let dirs =
        ProjectDirs::from("dev", "dbboard", "dbboard").ok_or(AnnotationsError::NoConfigDir)?;
    Ok(dirs.config_dir().join("annotations.toml"))
}

/// Read and parse `annotations.toml` at `path`. A missing file is **not**
/// an error: it yields an empty store, created lazily on the first note.
///
/// # Errors
///
/// - [`AnnotationsError::Io`] for non-`NotFound` I/O failures.
/// - Any validation error from [`AnnotationsFile::parse`].
pub fn load_or_empty(path: &Path) -> Result<AnnotationsFile, AnnotationsError> {
    match fs::read_to_string(path) {
        Ok(contents) => AnnotationsFile::parse(&contents),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(AnnotationsFile::empty()),
        Err(err) => Err(AnnotationsError::Io(err)),
    }
}

/// Write `file` to `path` atomically (sibling `*.tmp` via
/// [`secure_fs::create_new_user_only`] → `rename`), matching the
/// connection/AI stores' at-rest posture (ADR-0024).
///
/// # Errors
///
/// - [`AnnotationsError::Serialize`] if re-serializing to TOML fails.
/// - [`AnnotationsError::Io`] for any filesystem failure.
pub fn save_atomic(path: &Path, file: &AnnotationsFile) -> Result<(), AnnotationsError> {
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
        return Err(AnnotationsError::Io(err));
    }
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".annotations.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

// `create_new_user_only` rejects a stale temp from an interrupted save —
// fail loudly rather than clobber (ADR-0024).
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = secure_fs::create_new_user_only(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

/// In-memory owner of `annotations.toml` plus its path, exposing
/// read/set/clear for table- and column-level notes.
///
/// Every mutation follows the same crash-safe shape as
/// [`crate::ai_settings::AiSettingsAdmin`]: build the next
/// [`AnnotationsFile`], persist it with [`save_atomic`], and only then
/// swap it into `self.file`. A failed write therefore leaves both disk
/// and memory on the previous state — no partial mutation to unwind.
#[derive(Debug)]
pub struct AnnotationsAdmin {
    path: PathBuf,
    file: AnnotationsFile,
}

impl AnnotationsAdmin {
    /// Open the store at the default per-user path, loading existing
    /// notes (or starting empty if the file does not exist yet).
    ///
    /// # Errors
    ///
    /// [`AnnotationsError::NoConfigDir`] if no per-user config dir
    /// resolves, or any error from [`load_or_empty`].
    pub fn open_default() -> Result<Self, AnnotationsError> {
        let path = default_annotations_path()?;
        Self::new_with_file(path)
    }

    /// Open the store at an explicit path. Used by tests and by any
    /// caller that resolves the path itself.
    ///
    /// # Errors
    ///
    /// Any error from [`load_or_empty`].
    pub fn new_with_file(path: PathBuf) -> Result<Self, AnnotationsError> {
        let file = load_or_empty(&path)?;
        Ok(Self { path, file })
    }

    /// The note for one column, if any.
    #[must_use]
    pub fn column_note(&self, connection: &str, table: &str, column: &str) -> Option<&str> {
        self.table(connection, table)
            .and_then(|t| t.columns.iter().find(|c| c.name == column))
            .map(|c| c.note.as_str())
    }

    /// The table-level note, if any.
    #[must_use]
    pub fn table_note(&self, connection: &str, table: &str) -> Option<&str> {
        self.table(connection, table)
            .and_then(|t| t.note.as_deref())
    }

    /// Set (or, with an empty/whitespace note, clear) a column note and
    /// persist. Empty containers are pruned so the file never
    /// accumulates blank connection/table stanzas.
    ///
    /// # Errors
    ///
    /// [`AnnotationsError::Serialize`] / [`AnnotationsError::Io`] if the
    /// atomic save fails; `self` is left unchanged in that case.
    pub fn set_column_note(
        &mut self,
        connection: &str,
        table: &str,
        column: &str,
        note: &str,
    ) -> Result<(), AnnotationsError> {
        let trimmed = note.trim();
        let mut next = self.file.clone();
        {
            let tbl = table_entry_mut(&mut next, connection, table);
            match tbl.columns.iter_mut().find(|c| c.name == column) {
                Some(existing) if !trimmed.is_empty() => existing.note = trimmed.to_string(),
                Some(_) => tbl.columns.retain(|c| c.name != column),
                None if !trimmed.is_empty() => tbl.columns.push(ColumnAnnotation {
                    name: column.to_string(),
                    note: trimmed.to_string(),
                }),
                None => {}
            }
        }
        prune(&mut next);
        self.persist(next)
    }

    /// Set (or clear, on empty input) the table-level note and persist.
    ///
    /// # Errors
    ///
    /// As [`Self::set_column_note`].
    pub fn set_table_note(
        &mut self,
        connection: &str,
        table: &str,
        note: &str,
    ) -> Result<(), AnnotationsError> {
        let trimmed = note.trim();
        let mut next = self.file.clone();
        {
            let tbl = table_entry_mut(&mut next, connection, table);
            tbl.note = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }
        prune(&mut next);
        self.persist(next)
    }

    /// The in-memory file (read-only view, for callers that want to
    /// enumerate everything, e.g. a future export).
    #[must_use]
    pub fn file(&self) -> &AnnotationsFile {
        &self.file
    }

    fn table(&self, connection: &str, table: &str) -> Option<&TableAnnotations> {
        self.file
            .connections
            .iter()
            .find(|c| c.id == connection)
            .and_then(|c| c.tables.iter().find(|t| t.key == table))
    }

    fn persist(&mut self, next: AnnotationsFile) -> Result<(), AnnotationsError> {
        save_atomic(&self.path, &next)?;
        self.file = next;
        Ok(())
    }
}

/// Get or create the mutable table entry for `(connection, table)`,
/// creating the connection stanza first if needed. Empty stanzas it
/// creates are cleaned up by [`prune`] when the caller leaves no note.
fn table_entry_mut<'a>(
    file: &'a mut AnnotationsFile,
    connection: &str,
    table: &str,
) -> &'a mut TableAnnotations {
    let conn_idx = if let Some(i) = file.connections.iter().position(|c| c.id == connection) {
        i
    } else {
        file.connections.push(ConnectionAnnotations {
            id: connection.to_string(),
            tables: Vec::new(),
        });
        file.connections.len() - 1
    };
    let conn = &mut file.connections[conn_idx];
    let tbl_idx = if let Some(i) = conn.tables.iter().position(|t| t.key == table) {
        i
    } else {
        conn.tables.push(TableAnnotations {
            key: table.to_string(),
            note: None,
            columns: Vec::new(),
        });
        conn.tables.len() - 1
    };
    &mut conn.tables[tbl_idx]
}

/// Drop table stanzas with no note and no columns, then connection
/// stanzas with no tables, so an emptied-out annotation leaves no cruft.
fn prune(file: &mut AnnotationsFile) {
    for conn in &mut file.connections {
        conn.tables
            .retain(|t| t.note.is_some() || !t.columns.is_empty());
    }
    file.connections.retain(|c| !c.tables.is_empty());
}

/// Errors while loading, validating, or saving the annotation store.
/// Independent of the HTTP envelope: annotations are a purely local,
/// in-process concern (ADR-0045).
#[derive(Debug, Error)]
pub enum AnnotationsError {
    /// The TOML payload could not be parsed at all.
    #[error("annotations parse failed: {0}")]
    Parse(#[from] toml::de::Error),

    /// `version` does not equal [`ANNOTATIONS_VERSION`].
    #[error("unsupported annotations version: {0} (only version {expected} is supported)", expected = ANNOTATIONS_VERSION)]
    UnsupportedVersion(u32),

    /// Two connection entries share an id.
    #[error("duplicate annotations connection id: {0}")]
    DuplicateConnectionId(String),

    /// Two table entries within one connection share a key.
    #[error("duplicate annotations table key {key} in connection {connection}")]
    DuplicateTableKey { connection: String, key: String },

    /// Filesystem read or write failed. The path is not embedded so the
    /// message is safe to log; callers attach the path when they have it.
    #[error("annotations io failed: {0}")]
    Io(#[from] std::io::Error),

    /// Re-serializing the in-memory store back to TOML failed.
    #[error("annotations serialize failed: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// The OS reported no usable per-user config directory.
    #[error("could not resolve a per-user config directory")]
    NoConfigDir,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn admin_in(dir: &TempDir) -> AnnotationsAdmin {
        let path = dir.path().join("annotations.toml");
        AnnotationsAdmin::new_with_file(path).expect("open empty")
    }

    #[test]
    fn table_key_qualifies_only_when_a_schema_is_present() {
        assert_eq!(table_key(Some("public"), "orders"), "public.orders");
        assert_eq!(table_key(None, "orders"), "orders");
        // An empty schema string is treated as "no schema", not "".table.
        assert_eq!(table_key(Some(""), "orders"), "orders");
    }

    #[test]
    fn empty_constructor_uses_the_current_version() {
        let file = AnnotationsFile::empty();
        assert_eq!(file.version, ANNOTATIONS_VERSION);
        assert!(file.connections.is_empty());
    }

    #[test]
    fn load_or_empty_on_missing_file_returns_empty_store() {
        let dir = TempDir::new().expect("tempdir");
        let file = load_or_empty(&dir.path().join("nope.toml")).expect("missing is empty");
        assert_eq!(file, AnnotationsFile::empty());
    }

    #[test]
    fn version_only_file_parses_with_no_connections() {
        let file = AnnotationsFile::parse("version = 1\n").expect("parses");
        assert_eq!(file.version, 1);
        assert!(file.connections.is_empty());
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let err = AnnotationsFile::parse("version = 2\n").expect_err("v2 rejected");
        assert!(matches!(err, AnnotationsError::UnsupportedVersion(2)));
    }

    #[test]
    fn duplicate_connection_id_is_rejected() {
        let src = r#"
version = 1
[[connections]]
id = "store-a"
[[connections]]
id = "store-a"
"#;
        let err = AnnotationsFile::parse(src).expect_err("dupe conn rejected");
        assert!(matches!(err, AnnotationsError::DuplicateConnectionId(id) if id == "store-a"));
    }

    #[test]
    fn duplicate_table_key_within_a_connection_is_rejected() {
        let src = r#"
version = 1
[[connections]]
id = "store-a"
[[connections.tables]]
key = "orders"
[[connections.tables]]
key = "orders"
"#;
        let err = AnnotationsFile::parse(src).expect_err("dupe table rejected");
        assert!(matches!(
            err,
            AnnotationsError::DuplicateTableKey { connection, key }
            if connection == "store-a" && key == "orders"
        ));
    }

    #[test]
    fn set_column_note_persists_and_reopens() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("annotations.toml");
        {
            let mut admin = AnnotationsAdmin::new_with_file(path.clone()).expect("open");
            admin
                .set_column_note("store-a", "orders", "status", "0=pending 1=paid 2=void")
                .expect("set");
            assert_eq!(
                admin.column_note("store-a", "orders", "status"),
                Some("0=pending 1=paid 2=void")
            );
        }
        // Reopen from disk: the note survived the round-trip.
        let reopened = AnnotationsAdmin::new_with_file(path).expect("reopen");
        assert_eq!(
            reopened.column_note("store-a", "orders", "status"),
            Some("0=pending 1=paid 2=void")
        );
    }

    #[test]
    fn set_table_note_is_independent_of_column_notes() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin
            .set_table_note("store-a", "orders", "one row per placed order")
            .expect("table note");
        admin
            .set_column_note("store-a", "orders", "amt", "minor units, JPY")
            .expect("column note");
        assert_eq!(
            admin.table_note("store-a", "orders"),
            Some("one row per placed order")
        );
        assert_eq!(
            admin.column_note("store-a", "orders", "amt"),
            Some("minor units, JPY")
        );
    }

    #[test]
    fn empty_note_clears_and_prunes_the_entry() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin
            .set_column_note("store-a", "orders", "status", "temp")
            .expect("set");
        // Clearing with whitespace removes the column and, since nothing
        // else remains, the whole connection stanza.
        admin
            .set_column_note("store-a", "orders", "status", "   ")
            .expect("clear");
        assert_eq!(admin.column_note("store-a", "orders", "status"), None);
        assert!(
            admin.file().connections.is_empty(),
            "emptied stores prune to nothing, got {:?}",
            admin.file().connections
        );
    }

    #[test]
    fn note_is_trimmed_before_storing() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin
            .set_column_note("store-a", "orders", "status", "  paid flag  ")
            .expect("set");
        assert_eq!(
            admin.column_note("store-a", "orders", "status"),
            Some("paid flag")
        );
    }

    #[test]
    fn notes_are_keyed_per_connection_and_do_not_bleed() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin
            .set_column_note("store-a", "orders", "id", "A's id")
            .expect("set a");
        admin
            .set_column_note("store-b", "orders", "id", "B's id")
            .expect("set b");
        assert_eq!(admin.column_note("store-a", "orders", "id"), Some("A's id"));
        assert_eq!(admin.column_note("store-b", "orders", "id"), Some("B's id"));
    }

    #[test]
    fn schema_qualified_and_bare_keys_are_distinct() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        let qualified = table_key(Some("public"), "orders");
        let bare = table_key(None, "orders");
        admin
            .set_column_note("pg", &qualified, "id", "public.orders id")
            .expect("qualified");
        admin
            .set_column_note("pg", &bare, "id", "bare orders id")
            .expect("bare");
        assert_eq!(
            admin.column_note("pg", &qualified, "id"),
            Some("public.orders id")
        );
        assert_eq!(admin.column_note("pg", &bare, "id"), Some("bare orders id"));
    }

    #[test]
    fn unknown_lookup_is_none_not_a_panic() {
        let dir = TempDir::new().expect("tempdir");
        let admin = admin_in(&dir);
        assert_eq!(admin.column_note("nope", "nope", "nope"), None);
        assert_eq!(admin.table_note("nope", "nope"), None);
    }

    #[test]
    fn round_trip_through_toml_is_identity() {
        let dir = TempDir::new().expect("tempdir");
        let mut admin = admin_in(&dir);
        admin
            .set_table_note("store-a", "public.orders", "table note")
            .expect("t");
        admin
            .set_column_note("store-a", "public.orders", "status", "col note")
            .expect("c");
        let serialized = toml::to_string(admin.file()).expect("serialize");
        let parsed = AnnotationsFile::parse(&serialized).expect("reparse");
        assert_eq!(&parsed, admin.file());
    }
}
