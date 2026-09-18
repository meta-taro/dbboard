//! A snapshot of `connections.toml` taken the first time a new build runs,
//! so that rolling back to the previous build finds a file that build can
//! certainly read.
//!
//! The hazard this covers is narrow and total: an older build that meets a
//! `kind` it does not know fails to parse **the whole file**, not the one
//! entry. So a rollback — the thing a person reaches for when a release does
//! not work — lands them on a build with no connections at all, which is the
//! opposite of what a rollback is for.
//!
//! The snapshot is named after the build that is *about to* take over rather
//! than the one that wrote it, because the writer is not recorded anywhere
//! and guessing it would be a lie. `connections.pre-0.17.0.toml` says exactly
//! what it is: the file as it stood before 0.17.0 first ran.

use std::fs;

use dbboard_config::store::{snapshot_before_version, ConnectionFile};

/// A file already on disk is copied, once, under a name carrying the version
/// that is taking over.
#[test]
fn the_first_run_of_a_version_takes_a_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("connections.toml");
    fs::write(&path, "version = 1\n").expect("seed");

    let taken = snapshot_before_version(&path, "0.17.0").expect("snapshot");

    let expected = dir.path().join("connections.pre-0.17.0.toml");
    assert_eq!(taken.as_deref(), Some(expected.as_path()));
    assert_eq!(
        fs::read_to_string(&expected).expect("read"),
        "version = 1\n"
    );
}

/// The second run of the same version must not overwrite the copy: by then
/// the live file has been through this build, and the point of the snapshot
/// is that it has *not*.
#[test]
fn a_later_run_of_the_same_version_leaves_the_copy_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("connections.toml");
    fs::write(&path, "version = 1\n").expect("seed");
    snapshot_before_version(&path, "0.17.0").expect("first run");

    fs::write(&path, "version = 1\n# touched by the new build\n").expect("rewrite");
    let taken = snapshot_before_version(&path, "0.17.0").expect("second run");

    assert_eq!(
        taken, None,
        "the second run must report that it took nothing"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("connections.pre-0.17.0.toml")).expect("read"),
        "version = 1\n",
        "the copy must still be the pre-upgrade file",
    );
}

/// A fresh install has nothing to protect, and must not manufacture a file.
#[test]
fn nothing_to_copy_is_not_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("connections.toml");

    let taken = snapshot_before_version(&path, "0.17.0").expect("missing file is not an error");

    assert_eq!(taken, None);
    assert!(
        !dir.path().join("connections.pre-0.17.0.toml").exists(),
        "no live file means no snapshot, not an empty one",
    );
}

/// The copy is taken verbatim, including entries this build would refuse.
/// A snapshot that only kept what the *current* build understands would be
/// worthless for the rollback it exists to serve.
#[test]
fn a_kind_this_build_cannot_parse_is_still_copied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("connections.toml");
    let unreadable = "version = 1\n\n[[connections]]\nid = \"x\"\nname = \"X\"\nkind = \"engine-from-the-future\"\n";
    fs::write(&path, unreadable).expect("seed");

    assert!(
        ConnectionFile::parse(unreadable).is_err(),
        "precondition: this build must not understand the fixture",
    );

    let taken = snapshot_before_version(&path, "0.18.0").expect("snapshot");

    assert!(taken.is_some());
    assert_eq!(
        fs::read_to_string(dir.path().join("connections.pre-0.18.0.toml")).expect("read"),
        unreadable,
        "the copy must be byte-for-byte, not a re-serialisation",
    );
}
