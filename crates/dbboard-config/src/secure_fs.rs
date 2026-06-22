//! At-rest file permission helpers shared by `dbboard-config` and
//! `dbboard-ui` (ADR-0024).
//!
//! Two operations and a path classifier:
//!
//! - [`create_new_user_only`] creates a fresh file that the current
//!   user can read and write (`0o600` on Unix; inherited DACL on
//!   Windows, which `%APPDATA%\Roaming\<user>\` keeps user-only by
//!   default). Errors if the path already exists.
//! - [`open_append_user_only`] opens a file for append, creating it
//!   on first use with the same permissions, and defensively
//!   tightening any pre-existing file to `0o600` on Unix.
//! - [`is_likely_cloud_synced_path`] is a pure string classifier
//!   that returns the vendor name (`"OneDrive"`, `"iCloud Drive"`,
//!   `"Dropbox"`, `"Google Drive"`) when a path segment matches a
//!   known cloud-sync folder. No I/O.
//!
//! The Windows side intentionally does not call `SetNamedSecurityInfoW`:
//! the workspace declares `unsafe_code = "forbid"`, and the only
//! no-unsafe alternatives (`windows-acl` is abandoned; shelling out to
//! `icacls.exe` is heavy and locale-dependent) buy little over the
//! inherited ACL of `%APPDATA%\Roaming\<user>\`. ADR-0024 records the
//! trade-off and the conditions under which it would be reopened.

use std::fs;
use std::io;
use std::path::Path;

/// Create a new file at `path` that only the current user can read or
/// write. Returns `AlreadyExists` if the path already exists.
///
/// - Unix: opens with `create_new(true).mode(0o600)`. Race-free.
/// - Windows / other: opens with `create_new(true)` and lets the file
///   inherit its parent directory's ACL. Under `%APPDATA%\Roaming\`,
///   that ACL is user-only by default. See ADR-0024 for the rationale.
///
/// The returned [`fs::File`] is open for write and is `sync_all`-ready
/// (the caller is responsible for syncing).
///
/// # Errors
///
/// Returns the underlying [`io::Error`] for any filesystem failure
/// (e.g. missing parent dir, permission denied, or `AlreadyExists`
/// when `path` was raced into existence by another process).
pub fn create_new_user_only(path: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    }
}

/// Open `path` for append, creating it if absent with user-only
/// permissions, and defensively tightening an existing file's
/// permissions on Unix.
///
/// On Unix the first-creation path is a *single* open with the flags
/// `O_CREAT | O_EXCL | O_APPEND | mode(0o600)`, so the returned handle
/// is the same kernel descriptor the file was created with — no
/// close-and-reopen window in which an attacker could substitute a
/// symlink. If the file already exists the function falls through to
/// a plain append open after calling [`fs::set_permissions`] with
/// `0o600`, so a file that pre-dates ADR-0024 (e.g. an upgraded user's
/// existing `history.jsonl`) gets tightened on the next write. The
/// tightening path retains a small TOCTOU between `chmod` and `open`
/// — acceptable under ADR-0024's lost-laptop threat model, which does
/// not assume a hostile active local attacker.
///
/// On Windows / other platforms, this defers to the file's inherited
/// ACL — see [`create_new_user_only`] and ADR-0024.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] for any filesystem failure
/// other than `AlreadyExists` during the creation attempt (which is
/// recovered by falling through to the append open).
pub fn open_append_user_only(path: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;

        match fs::OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(file) => Ok(file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                // Pre-existing file: defensively tighten its
                // permissions in case it was created before ADR-0024,
                // then open append. The chmod-then-open pair is the
                // only non-atomic step in this module; see the doc
                // comment for the threat-model rationale.
                let perms = fs::Permissions::from_mode(0o600);
                fs::set_permissions(path, perms)?;
                fs::OpenOptions::new().append(true).open(path)
            }
            Err(err) => Err(err),
        }
    }
    #[cfg(not(unix))]
    {
        // Windows / other: inherited DACL of `%APPDATA%\Roaming\<user>\`
        // is already user-only on every supported version. A single
        // `create(true).append(true)` open suffices — no per-file ACL
        // manipulation, no extra round-trip. See ADR-0024.
        fs::OpenOptions::new().append(true).create(true).open(path)
    }
}

/// Pure-string classifier: does `path` traverse a known cloud-sync
/// vendor folder? Returns the vendor name on a hit, `None` otherwise.
///
/// The match is case-insensitive and looks for a literal directory
/// segment named one of:
///
/// - `OneDrive` / `OneDrive - <Tenant>` (Microsoft 365 personal /
///   business; the Known Folder Move feature relocates
///   `%APPDATA%\Roaming\` under this tree)
/// - `iCloud Drive` / `iCloudDrive` / `Mobile Documents` (macOS)
/// - `Dropbox`
/// - `Google Drive` / `GoogleDrive` / `My Drive` (the default
///   *Google Drive for Desktop* mount; on macOS it lives under
///   `~/Library/CloudStorage/GoogleDrive-<email>/My Drive`, so we
///   also match `CloudStorage` and any `GoogleDrive-*` segment)
///
/// This is intentionally a syntactic heuristic — it can produce false
/// positives (e.g. an unrelated folder a user manually named
/// `Dropbox`) and false negatives (e.g. an NTFS junction that hides
/// the `OneDrive` name behind a different resolved path; non-UTF-8 path
/// segments are silently skipped via [`std::ffi::OsStr::to_str`]).
/// The intended use is a *warning* at startup, not a hard failure.
/// See ADR-0024.
#[must_use]
pub fn is_likely_cloud_synced_path(path: &Path) -> Option<&'static str> {
    for component in path.components() {
        // Non-UTF-8 segments (theoretically possible on Unix) are
        // silently skipped — the matcher is a best-effort heuristic
        // and missing a warning is preferable to panicking.
        if let Some(segment) = component.as_os_str().to_str() {
            let lower = segment.to_ascii_lowercase();
            // OneDrive personal is bare "OneDrive"; OneDrive for
            // Business is "OneDrive - <Tenant Name>" with arbitrary
            // tenant text after the separator.
            if lower == "onedrive" || lower.starts_with("onedrive - ") {
                return Some("OneDrive");
            }
            if lower == "icloud drive" || lower == "iclouddrive" || lower == "mobile documents" {
                return Some("iCloud Drive");
            }
            if lower == "dropbox" {
                return Some("Dropbox");
            }
            // "Google Drive" / "GoogleDrive": classic Windows mount
            // points. "My Drive": the default Google Drive for
            // Desktop subfolder, used as the visible root on every
            // platform since the 2021–2023 redesign.
            // "CloudStorage" / "GoogleDrive-<email>": macOS's
            // `~/Library/CloudStorage/GoogleDrive-<email>/` layout.
            if lower == "google drive"
                || lower == "googledrive"
                || lower == "my drive"
                || lower == "cloudstorage"
                || lower.starts_with("googledrive-")
            {
                return Some("Google Drive");
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // ---- create_new_user_only --------------------------------------

    #[test]
    fn create_new_user_only_creates_a_writable_file() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("brand-new.txt");
        let mut file =
            create_new_user_only(&path).expect("create_new_user_only on fresh path must succeed");
        file.write_all(b"hello").expect("write to fresh file");
        file.sync_all().expect("sync fresh file");

        let read_back = fs::read_to_string(&path).expect("read back fresh file");
        assert_eq!(read_back, "hello");
    }

    #[test]
    fn create_new_user_only_rejects_an_existing_path() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("already-here.txt");
        fs::write(&path, b"existing").expect("seed existing file");

        let err = create_new_user_only(&path)
            .expect_err("create_new_user_only on existing path must fail");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[cfg(unix)]
    #[test]
    fn create_new_user_only_lands_as_0o600_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("private.txt");
        let _file = create_new_user_only(&path).expect("create");

        let meta = fs::metadata(&path).expect("metadata");
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "newly created file must be user-only (0o600)");
    }

    // ---- open_append_user_only -------------------------------------

    #[test]
    fn open_append_user_only_creates_file_when_absent() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("history.jsonl");
        let mut file = open_append_user_only(&path).expect("open_append on fresh path");
        file.write_all(b"line-1\n").expect("write line 1");
        drop(file);

        assert_eq!(
            fs::read_to_string(&path).expect("read back"),
            "line-1\n",
            "first open_append must create + write"
        );
    }

    #[test]
    fn open_append_user_only_appends_to_existing_file() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("history.jsonl");
        fs::write(&path, b"line-1\n").expect("seed");

        let mut file = open_append_user_only(&path).expect("open_append on existing path");
        file.write_all(b"line-2\n").expect("append line 2");
        drop(file);

        assert_eq!(
            fs::read_to_string(&path).expect("read back"),
            "line-1\nline-2\n",
            "open_append must preserve existing content and add to the tail"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_append_user_only_first_create_is_0o600_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("history.jsonl");
        let _file = open_append_user_only(&path).expect("first open_append");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "first-time creation via open_append must land 0o600"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_append_user_only_tightens_preexisting_file_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("legacy.jsonl");
        fs::write(&path, b"legacy\n").expect("seed legacy file");
        // Simulate an upgrade scenario: existing file was created with
        // umask-default 0o644 before ADR-0024 landed.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("loosen");

        let _file = open_append_user_only(&path).expect("open_append on legacy");

        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "open_append must defensively tighten a pre-existing file"
        );
    }

    // ---- is_likely_cloud_synced_path -------------------------------

    #[test]
    fn is_likely_cloud_synced_path_returns_none_for_a_clean_appdata_path() {
        // The path doesn't have to exist; the matcher is pure-string.
        let path = PathBuf::from("/home/alice/.config/dbboard/connections.toml");
        assert_eq!(is_likely_cloud_synced_path(&path), None);
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_onedrive_personal() {
        let path = PathBuf::from(r"C:\Users\alice\OneDrive\AppData\Roaming\dbboard\config");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("OneDrive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_onedrive_for_business_tenant() {
        let path = PathBuf::from(r"C:\Users\alice\OneDrive - Contoso Ltd\AppData\Roaming\dbboard");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("OneDrive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_onedrive_case_insensitively() {
        let path = PathBuf::from(r"C:\Users\alice\onedrive\AppData\Roaming\dbboard");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("OneDrive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_icloud_drive() {
        let path = PathBuf::from("/Users/alice/Library/Mobile Documents/com~apple~CloudDocs");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("iCloud Drive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_dropbox() {
        let path = PathBuf::from("/Users/alice/Dropbox/dbboard/config");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("Dropbox"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_google_drive() {
        let path = PathBuf::from(r"C:\Users\alice\Google Drive\dbboard\config");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("Google Drive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_google_drive_my_drive() {
        // Default visible root for Google Drive for Desktop on every
        // platform since 2021–2023. Without this match, a user whose
        // dbboard config lands inside `…/My Drive/dbboard` gets no
        // cloud-sync warning at all.
        let path = PathBuf::from(
            "/Users/alice/Library/CloudStorage/GoogleDrive-alice@example.com/My Drive/dbboard",
        );
        assert_eq!(is_likely_cloud_synced_path(&path), Some("Google Drive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_detects_google_drive_macos_cloudstorage() {
        // The intermediate `CloudStorage` segment alone is enough — a
        // user might have moved or renamed `My Drive` and still be
        // synced.
        let path = PathBuf::from("/Users/alice/Library/CloudStorage/GoogleDrive-alice@example.com");
        assert_eq!(is_likely_cloud_synced_path(&path), Some("Google Drive"));
    }

    #[test]
    fn is_likely_cloud_synced_path_does_not_match_substring_inside_segment() {
        // "OneDriveBackup" is not OneDrive itself — refuse the
        // substring false positive.
        let path = PathBuf::from(r"C:\Users\alice\OneDriveBackup\dbboard\config");
        assert_eq!(is_likely_cloud_synced_path(&path), None);
    }
}
