//! Integration tests for the filesystem layer of `dbboard-config`.
//!
//! Schema-only parse tests live next to the types in `src/store.rs`;
//! these exercise the real `fs::read_to_string` / `fs::rename` paths
//! against a `tempfile::TempDir` so each test gets a clean filesystem
//! root that is dropped automatically.

use dbboard_config::store::{default_path, load_or_empty, save_atomic};
use dbboard_config::{ConfigError, ConnectionEntry, ConnectionFile, ConnectionKind};

#[test]
fn load_or_empty_on_missing_file_yields_empty_store() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("connections.toml");
    let file = load_or_empty(&path).expect("missing file is not an error");
    assert!(file.connections.is_empty());
    assert_eq!(file.version, dbboard_config::CONFIG_VERSION);
    assert!(
        !path.exists(),
        "load_or_empty must not create the file as a side effect"
    );
}

#[test]
fn save_then_load_round_trips_through_disk() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("connections.toml");
    let original = ConnectionFile {
        version: dbboard_config::CONFIG_VERSION,
        connections: vec![ConnectionEntry {
            id: "local-turso".to_string(),
            name: "Local libSQL".to_string(),
            kind: ConnectionKind::Turso {
                path: ":memory:".to_string(),
            },
        }],
    };
    save_atomic(&path, &original).expect("save");
    let reloaded = load_or_empty(&path).expect("load");
    assert_eq!(original, reloaded);
}

#[test]
fn save_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let nested = dir.path().join("a").join("b").join("connections.toml");
    save_atomic(&nested, &ConnectionFile::empty()).expect("save into nested path");
    assert!(nested.exists(), "save_atomic must create parent dirs");
}

#[test]
fn save_overwrites_an_existing_file_atomically() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("connections.toml");

    let first = ConnectionFile {
        version: dbboard_config::CONFIG_VERSION,
        connections: vec![ConnectionEntry {
            id: "old".to_string(),
            name: "Old".to_string(),
            kind: ConnectionKind::Turso {
                path: ":memory:".to_string(),
            },
        }],
    };
    save_atomic(&path, &first).expect("first save");

    let second = ConnectionFile {
        version: dbboard_config::CONFIG_VERSION,
        connections: vec![ConnectionEntry {
            id: "new".to_string(),
            name: "New".to_string(),
            kind: ConnectionKind::Turso {
                path: "/tmp/new.db".to_string(),
            },
        }],
    };
    save_atomic(&path, &second).expect("second save replaces the first");

    let reloaded = load_or_empty(&path).expect("load after overwrite");
    assert_eq!(reloaded.connections.len(), 1);
    assert_eq!(reloaded.connections[0].id, "new");

    let leftover = dir.path().join("connections.toml.tmp");
    assert!(
        !leftover.exists(),
        "the tmp sibling must be renamed away after a successful save"
    );
}

#[test]
fn load_propagates_parse_error_for_garbage_payload() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("connections.toml");
    std::fs::write(&path, "this is not toml at all = = =").expect("seed garbage");
    let err = load_or_empty(&path).expect_err("garbage must not parse");
    assert!(matches!(err, ConfigError::Parse(_)));
}

#[test]
fn load_propagates_duplicate_id_error_for_hand_edited_files() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("connections.toml");
    std::fs::write(
        &path,
        r#"
version = 1

[[connections]]
id   = "dup"
name = "A"
kind = "turso"
path = ":memory:"

[[connections]]
id   = "dup"
name = "B"
kind = "turso"
path = "/tmp/x.db"
"#,
    )
    .expect("seed duplicate");
    let err = load_or_empty(&path).expect_err("duplicate id must propagate");
    match err {
        ConfigError::DuplicateId(id) => assert_eq!(id, "dup"),
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

#[test]
fn default_path_resolves_to_a_connections_toml_filename() {
    // We can't predict the exact path (it's per-user), but we can
    // assert the file name is the expected one and the parent exists
    // conceptually under a "dbboard" segment.
    let path = default_path().expect("default_path");
    assert_eq!(
        path.file_name().and_then(|s| s.to_str()),
        Some("connections.toml")
    );
    let as_str = path.to_string_lossy().to_lowercase();
    assert!(
        as_str.contains("dbboard"),
        "default path should live under a dbboard-named directory: {path:?}"
    );
}

#[cfg(unix)]
#[test]
fn save_writes_the_file_with_mode_0o600_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("create tempdir");
    let path = dir.path().join("connections.toml");
    save_atomic(&path, &ConnectionFile::empty()).expect("save");
    let mode = std::fs::metadata(&path)
        .expect("stat the saved file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o600,
        "user-only mode required for secret-adjacent file"
    );
}
