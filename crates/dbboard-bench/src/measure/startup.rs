//! What the desktop shell does before a window exists.
//!
//! `run()` in `apps/desktop/src-tauri/src/lib.rs` resolves the config
//! directory, opens `connections.toml` and `annotations.toml`, stands up the
//! AI layer and builds the MCP service — all of it before Tauri is told to
//! show anything. The first three are this crate's to measure; the last two
//! live behind `pub(crate)` in the app and would need the Tauri runtime.
//!
//! Fixtures go in a temporary directory. Timing the operator's real
//! `connections.toml` would measure one machine's file, and would point a
//! tool that prints tables at a file full of connection names.

use std::sync::Arc;
use std::time::Instant;

use dbboard_config::annotations::{self, ANNOTATIONS_VERSION};
use dbboard_config::{
    AnnotationsFile, ColumnAnnotation, ConnectionAdmin, ConnectionAnnotations, ConnectionDraft,
    ConnectionKindDraft, InMemorySecretStore, SecretStore, TableAnnotations,
};
use tempfile::TempDir;

use super::{BenchResult, Sampler};
use crate::harness::Reading;

/// Twenty, because that is the size at which the connection list stopped
/// being hand-manageable — the case ADR-0140 and the 0.13.0 tidying verbs
/// were written for.
const CONNECTIONS: usize = 20;
const TABLES_PER_CONNECTION: usize = 5;
const COLUMNS_PER_TABLE: usize = 4;

/// A secret store that is not the platform keychain.
///
/// Deliberate: a benchmark must not pop an authorisation dialog, and on a
/// machine where it did the number would be measuring how fast someone
/// clicks. What the real keychain costs at startup is worth knowing and is
/// not knowable from here.
fn secrets() -> Arc<dyn SecretStore> {
    Arc::new(InMemorySecretStore::new())
}

/// Write a `connections.toml` holding [`CONNECTIONS`] entries, using the same
/// writer the app uses, and return the directory holding it.
fn connections_fixture() -> BenchResult<(TempDir, std::path::PathBuf)> {
    let dir = TempDir::new()?;
    let path = dir.path().join("connections.toml");
    let mut admin = ConnectionAdmin::open(path.clone(), secrets())?;
    for i in 0..CONNECTIONS {
        admin.add(ConnectionDraft {
            id: format!("conn-{i:02}"),
            name: format!("Connection {i:02}"),
            // A local libSQL path: the one kind with no secret, so the
            // fixture never touches a keyring even in principle.
            kind: ConnectionKindDraft::Turso {
                path: format!("/tmp/dbboard-bench-{i:02}.db"),
            },
            ssh: None,
            mcp_write: false,
            mcp_alias: None,
            color: None,
            tag: None,
        })?;
    }
    Ok((dir, path))
}

/// Write an `annotations.toml` covering the same connections.
fn annotations_fixture() -> BenchResult<(TempDir, std::path::PathBuf)> {
    let dir = TempDir::new()?;
    let path = dir.path().join("annotations.toml");
    let file = AnnotationsFile {
        version: ANNOTATIONS_VERSION,
        connections: (0..CONNECTIONS)
            .map(|c| ConnectionAnnotations {
                id: format!("conn-{c:02}"),
                tables: (0..TABLES_PER_CONNECTION)
                    .map(|t| TableAnnotations {
                        key: format!("table_{t:02}"),
                        note: Some(format!("What table {t:02} is for")),
                        columns: (0..COLUMNS_PER_TABLE)
                            .map(|col| ColumnAnnotation {
                                name: format!("col_{col:02}"),
                                note: format!("What column {col:02} holds"),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    };
    std::fs::write(&path, toml::to_string(&file)?)?;
    Ok((dir, path))
}

/// Time the startup group into `out`.
///
/// # Errors
///
/// Returns any fixture-building failure.
pub fn measure(out: &mut Vec<Reading>) -> BenchResult<()> {
    // ---- startup/config_paths -------------------------------------------
    let mut s = Sampler::new("startup/config_paths");
    while s.wants_more() {
        let t = Instant::now();
        let resolved = dbboard_config::default_path();
        s.record(t.elapsed());
        drop(std::hint::black_box(resolved));
    }
    out.extend(s.finish());

    // ---- startup/connections_open_20 ------------------------------------
    let (_guard, path) = connections_fixture()?;
    let mut s = Sampler::new("startup/connections_open_20");
    while s.wants_more() {
        let store = secrets();
        let t = Instant::now();
        let admin = ConnectionAdmin::open(path.clone(), store)?;
        s.record(t.elapsed());
        drop(std::hint::black_box(admin));
    }
    out.extend(s.finish());

    // ---- startup/annotations_open_20 ------------------------------------
    let (_guard, path) = annotations_fixture()?;
    let mut s = Sampler::new("startup/annotations_open_20");
    while s.wants_more() {
        let t = Instant::now();
        let file = annotations::load_or_empty(&path)?;
        s.record(t.elapsed());
        drop(std::hint::black_box(file));
    }
    out.extend(s.finish());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{annotations_fixture, connections_fixture};

    /// The two startup parse points do not read comparable files, and the
    /// first reading of them said `annotations.toml` parses "six times
    /// slower than `connections.toml` for the same twenty connections".
    /// It does not: the annotations fixture carries a note per table and
    /// per column on top of those connections, so it is several times the
    /// document. Anyone reading the two medians side by side will reach for
    /// the same wrong conclusion, so the disparity is asserted here rather
    /// than left to be re-derived.
    #[test]
    fn the_two_startup_fixtures_are_not_the_same_size() {
        let (_a, connections) = connections_fixture().expect("connections fixture");
        let (_b, annotations) = annotations_fixture().expect("annotations fixture");

        let conn_bytes = std::fs::metadata(&connections)
            .expect("stat connections")
            .len();
        let anno_bytes = std::fs::metadata(&annotations)
            .expect("stat annotations")
            .len();

        assert!(
            anno_bytes > conn_bytes * 4,
            "annotations.toml ({anno_bytes} bytes) should dwarf connections.toml \
             ({conn_bytes} bytes); if it no longer does, the fixtures changed and \
             the two medians may finally be worth comparing"
        );
    }
}
