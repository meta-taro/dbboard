//! A breadcrumb for an auto-update that started but may never have landed
//! (ADR-0067 installs in place and relaunches).
//!
//! The updater downloads, installs, and restarts. When the restart does not
//! happen — the process exits and no new one appears — the next manual launch
//! is the *old* build again, and nothing anywhere says why. From the outside
//! that is indistinguishable from "the update never ran": the notice offers
//! the same version again, the app looks fine, and the person is left
//! re-running an update that keeps not taking.
//!
//! So the attempt is written down *before* the installer is handed control,
//! and read back on the next launch. If the running build is still the one we
//! were updating away from, the update did not complete and the app says so,
//! with a link to download the installer by hand.
//!
//! Sibling to [`crate::ui_settings`]: same `ProjectDirs` config dir, same
//! atomic sibling-`*.tmp`-then-rename write. Like that module, nothing here
//! is allowed to break startup — every read failure resolves to "no notice".

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::secure_fs;

/// The single TOML schema version this build understands. An unknown version
/// is treated as unreadable (dropped, no notice) rather than an error: a
/// breadcrumb is a hint, never a thing worth failing a launch over.
pub const UPDATE_ATTEMPT_VERSION: u32 = 1;

/// The on-disk record of an update that was started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AttemptFile {
    version: u32,
    /// The version that was running when the install began.
    from: String,
    /// The version the install was expected to produce.
    to: String,
}

/// An update that was started and did not land, as reported to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StalledUpdate {
    /// The version still running — the one the update was meant to replace.
    pub from: String,
    /// The version the download was for.
    pub to: String,
}

/// Default per-user path for `update-attempt.toml`, alongside the other
/// config files.
///
/// # Errors
///
/// Returns [`ConfigError::NoConfigDir`] when the OS reports no usable
/// per-user config directory.
pub fn default_update_attempt_path() -> Result<PathBuf, ConfigError> {
    Ok(crate::store::config_dir()?.join("update-attempt.toml"))
}

/// Write down that an update from `from` to `to` is about to be installed.
///
/// Call this *before* the installer runs. Overwrites any earlier attempt:
/// only the most recent one can still be in flight.
///
/// # Errors
///
/// - [`ConfigError::Serialize`] if TOML serialization fails.
/// - [`ConfigError::Io`] for any filesystem failure.
pub fn record(path: &Path, from: &str, to: &str) -> Result<(), ConfigError> {
    let attempt = AttemptFile {
        version: UPDATE_ATTEMPT_VERSION,
        from: from.to_owned(),
        to: to.to_owned(),
    };
    let serialized = toml::to_string(&attempt)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path_for(path);
    write_new_file(&tmp, serialized.as_bytes())?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::Io(err));
    }
    Ok(())
}

/// Read the breadcrumb, remove it, and report whether the update it describes
/// failed to land.
///
/// Returns `Some` only when `running_version` is still the version the update
/// was meant to replace. Anything else — no breadcrumb, an unreadable one, or
/// a running build that has moved — clears the file and returns `None`.
///
/// The file is removed either way, so a stalled update is reported once and
/// does not nag on every launch afterwards.
#[must_use]
pub fn take(path: &Path, running_version: &str) -> Option<StalledUpdate> {
    let raw = fs::read_to_string(path).ok()?;
    // Removed before it is understood, on purpose: a breadcrumb we cannot
    // read will not become readable later, and keeping it means carrying the
    // same unusable file forward on every launch from here on.
    let _ = fs::remove_file(path);

    let attempt: AttemptFile = toml::from_str(&raw).ok()?;
    if attempt.version != UPDATE_ATTEMPT_VERSION {
        return None;
    }
    // Only the build we were updating *away from* proves nothing landed.
    if !same_version(&attempt.from, running_version) {
        return None;
    }
    Some(StalledUpdate {
        from: attempt.from,
        to: attempt.to,
    })
}

/// Compare two version strings the way the notice does: ignore surrounding
/// whitespace and a single leading `v`, so `v0.10.0` and `0.10.0` are the
/// same build. Mirrors `normalizeVersion` in
/// `apps/desktop/src/lib/update/notice.ts`.
fn same_version(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

fn normalize(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed.strip_prefix(['v', 'V']).unwrap_or(trimmed)
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".update-attempt.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let mut file = secure_fs::create_new_user_only(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn no_breadcrumb_is_no_notice() {
        let d = dir();
        assert_eq!(take(&d.path().join("update-attempt.toml"), "0.10.0"), None);
    }

    #[test]
    fn an_update_that_did_not_land_is_reported() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("record");

        // Still on the old build: the installer never took over.
        assert_eq!(
            take(&p, "0.10.0"),
            Some(StalledUpdate {
                from: "0.10.0".to_owned(),
                to: "0.11.0".to_owned(),
            })
        );
    }

    #[test]
    fn an_update_that_landed_says_nothing() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("record");

        assert_eq!(take(&p, "0.11.0"), None);
    }

    #[test]
    fn a_build_that_moved_somewhere_else_says_nothing() {
        // Not the target, but not the old build either — something installed.
        // Reporting a failure here would be a lie the person cannot check.
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("record");

        assert_eq!(take(&p, "0.12.0"), None);
    }

    #[test]
    fn a_leading_v_is_the_same_build() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "v0.10.0", "v0.11.0").expect("record");

        assert_eq!(take(&p, "0.10.0").map(|s| s.to), Some("v0.11.0".to_owned()));
    }

    #[test]
    fn the_breadcrumb_is_reported_once_and_then_gone() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("record");

        assert!(take(&p, "0.10.0").is_some());
        assert!(!p.exists(), "taking the breadcrumb must remove it");
        assert_eq!(take(&p, "0.10.0"), None, "and it must not report twice");
    }

    #[test]
    fn a_landed_update_clears_the_breadcrumb_too() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("record");

        assert_eq!(take(&p, "0.11.0"), None);
        assert!(
            !p.exists(),
            "a landed update must not leave the file behind"
        );
    }

    #[test]
    fn an_unreadable_breadcrumb_is_dropped_not_reported() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        fs::write(&p, "this is not toml").expect("write");

        assert_eq!(take(&p, "0.10.0"), None);
        assert!(!p.exists(), "an unreadable breadcrumb must not linger");
    }

    #[test]
    fn an_unknown_schema_version_is_dropped_not_reported() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        let body = "version = 99\nfrom = \"0.10.0\"\nto = \"0.11.0\"\n";
        fs::write(&p, body).expect("write");

        assert_eq!(take(&p, "0.10.0"), None);
    }

    #[test]
    fn recording_twice_keeps_only_the_latest_attempt() {
        let d = dir();
        let p = d.path().join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("first");
        record(&p, "0.10.0", "0.12.0").expect("second");

        assert_eq!(take(&p, "0.10.0").map(|s| s.to), Some("0.12.0".to_owned()));
    }

    #[test]
    fn recording_creates_the_config_dir_if_it_is_not_there_yet() {
        let d = dir();
        let p = d.path().join("nested").join("update-attempt.toml");
        record(&p, "0.10.0", "0.11.0").expect("record");

        assert!(p.exists());
    }
}
