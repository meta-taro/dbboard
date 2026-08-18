//! A one-shot instruction to the running window, and its answer (ADR-0109).
//!
//! [`crate::ui_settings`] carries *state*: the last write wins, re-reading it
//! is harmless, and nobody waits for an answer. An instruction to the window
//! — put this SQL in the editor, run it, open the AI panel — is the opposite
//! on every count. It must happen exactly once, and the caller wants to know
//! whether it happened, so this module adds the two things a settings file
//! does not need: a sequence number, and a place to write the answer.
//!
//! Two files, not one, because the two ends are separate OS processes. A
//! single file would make the MCP server and the client read-modify-write the
//! same path, and one of the two would silently lose its write. Here each
//! file has exactly one writer:
//!
//! | file | written by | read by |
//! |---|---|---|
//! | `ui-command.toml` | the MCP server | the client |
//! | `ui-command-result.toml` | the client | the MCP server |
//!
//! Both use the same atomic sibling-`*.tmp`-then-rename write as the rest of
//! the crate, so a reader never sees a half-written file, and both degrade to
//! a default rather than erroring — an unreadable command must not stop the
//! window, and an unreadable answer must not stop the agent.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::secure_fs;

/// The single TOML schema version this build understands, for both files.
pub const UI_COMMAND_VERSION: u32 = 1;

/// What the window is being asked to do.
///
/// Tagged by `kind` so the file reads as prose and an unknown verb fails as a
/// parse error rather than as a silently-empty struct. Deliberately small:
/// each variant exists because a verification step needed it and asking a
/// person to click was the alternative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiCommand {
    /// Replace the SQL editor's contents. Does not run anything.
    SetEditorSql {
        /// The text to put in the editor, verbatim.
        sql: String,
    },
    /// Run whatever the editor currently holds, as if the run button were
    /// pressed.
    RunQuery,
    /// Open the AI panel.
    OpenAiPanel,
    /// Open the AI provider settings, which live *inside* the AI panel.
    ///
    /// Refused when the panel is closed, because the part of the window that
    /// owns this verb is not mounted then. That refusal is the honest answer:
    /// there is no top-level route to these settings to fall back on.
    OpenAiSettings,
}

impl UiCommand {
    /// The `kind` tag as it appears on the wire.
    ///
    /// Used for the human-readable half of an MCP reply and in log lines, so
    /// that neither has to re-derive the name from the variant.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::SetEditorSql { .. } => "set_editor_sql",
            Self::RunQuery => "run_query",
            Self::OpenAiPanel => "open_ai_panel",
            Self::OpenAiSettings => "open_ai_settings",
        }
    }
}

/// Top-level shape of `ui-command.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiCommandFile {
    pub version: u32,
    /// Increases by one per instruction. The client acts when this rises
    /// above what it last acted on, which is what makes "run the same query
    /// again" a second event rather than a no-op — comparing the *command*
    /// would collapse the two.
    pub seq: u64,
    /// Absent in the default file, i.e. before anything has ever been asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<UiCommand>,
}

impl Default for UiCommandFile {
    fn default() -> Self {
        Self {
            version: UI_COMMAND_VERSION,
            seq: 0,
            command: None,
        }
    }
}

impl UiCommandFile {
    /// Parse from TOML, enforcing the schema version.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Parse`] on malformed TOML or an unknown `kind`.
    /// - [`ConfigError::UnsupportedVersion`] when `version` is not
    ///   [`UI_COMMAND_VERSION`].
    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let file: UiCommandFile = toml::from_str(contents)?;
        if file.version != UI_COMMAND_VERSION {
            return Err(ConfigError::UnsupportedVersion(file.version));
        }
        Ok(file)
    }
}

/// Top-level shape of `ui-command-result.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiResultFile {
    pub version: u32,
    /// The `seq` of the command this answers. A caller matches on it rather
    /// than on "a newer file appeared", so a leftover answer from an earlier
    /// session can never be mistaken for its own.
    pub seq: u64,
    pub ok: bool,
    /// Why it failed. Present when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What happened, for the caller to report. Present when there is
    /// something to say beyond "done".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Default for UiResultFile {
    fn default() -> Self {
        Self {
            version: UI_COMMAND_VERSION,
            // No command has been answered. Real sequence numbers start at 1,
            // so this can never collide with one.
            seq: 0,
            ok: false,
            error: None,
            detail: None,
        }
    }
}

impl UiResultFile {
    /// A successful answer to `seq`.
    #[must_use]
    pub fn ok(seq: u64, detail: Option<String>) -> Self {
        Self {
            version: UI_COMMAND_VERSION,
            seq,
            ok: true,
            error: None,
            detail,
        }
    }

    /// A failed answer to `seq`.
    #[must_use]
    pub fn failed(seq: u64, error: impl Into<String>) -> Self {
        Self {
            version: UI_COMMAND_VERSION,
            seq,
            ok: false,
            error: Some(error.into()),
            detail: None,
        }
    }

    /// Parse from TOML, enforcing the schema version.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::Parse`] on malformed TOML.
    /// - [`ConfigError::UnsupportedVersion`] when `version` is not
    ///   [`UI_COMMAND_VERSION`].
    pub fn parse(contents: &str) -> Result<Self, ConfigError> {
        let file: UiResultFile = toml::from_str(contents)?;
        if file.version != UI_COMMAND_VERSION {
            return Err(ConfigError::UnsupportedVersion(file.version));
        }
        Ok(file)
    }

    /// Is this the answer to `seq`?
    #[must_use]
    pub fn answers(&self, seq: u64) -> bool {
        self.seq == seq
    }
}

/// The sequence number to write next.
///
/// Takes the highest of what either file has seen, so an answer that outlives
/// its command file — the profile was cleaned, the command file was deleted
/// by hand — cannot make the next command reuse a number that already has an
/// answer sitting next to it. Reusing one would let the caller match on a
/// stale answer and report success for something that never ran.
///
/// Saturating rather than wrapping: a wrap would hand out a number below one
/// already answered, which is the same bug arriving 2^64 commands later.
#[must_use]
pub fn next_seq(command_seq: u64, result_seq: u64) -> u64 {
    command_seq.max(result_seq).saturating_add(1)
}

/// Should the client act on `file`, given the `seq` it last acted on?
///
/// Strictly greater, so re-reading an unchanged file does nothing, and a file
/// whose `seq` went *backwards* (an older profile restored underneath a
/// running window) is ignored rather than replayed.
#[must_use]
pub fn pending_command(file: &UiCommandFile, last_acted: u64) -> Option<&UiCommand> {
    if file.seq > last_acted {
        file.command.as_ref()
    } else {
        None
    }
}

/// Default per-user path for `ui-command.toml`.
///
/// # Errors
///
/// Returns [`ConfigError::NoConfigDir`] when the OS reports no usable
/// per-user config directory.
pub fn default_ui_command_path() -> Result<PathBuf, ConfigError> {
    Ok(crate::store::config_dir()?.join("ui-command.toml"))
}

/// Default per-user path for `ui-command-result.toml`.
///
/// # Errors
///
/// Returns [`ConfigError::NoConfigDir`] when the OS reports no usable
/// per-user config directory.
pub fn default_ui_result_path() -> Result<PathBuf, ConfigError> {
    Ok(crate::store::config_dir()?.join("ui-command-result.toml"))
}

/// Load `ui-command.toml`, falling back to the default on any problem.
///
/// Never errors, for the same reason [`crate::ui_settings::load_or_default`]
/// never does: this is read on a timer inside a running window, and a file
/// someone hand-edited into invalidity must degrade to "nothing pending"
/// rather than take the window down.
#[must_use]
pub fn load_command_or_default(path: &Path) -> UiCommandFile {
    match fs::read_to_string(path) {
        Ok(contents) => UiCommandFile::parse(&contents).unwrap_or_else(|e| {
            eprintln!("dbboard: ignoring unreadable ui-command.toml ({e})");
            UiCommandFile::default()
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => UiCommandFile::default(),
        Err(err) => {
            eprintln!("dbboard: could not read ui-command.toml ({err})");
            UiCommandFile::default()
        }
    }
}

/// Load `ui-command-result.toml`, falling back to the default on any problem.
///
/// The default's `seq` is 0, which [`UiResultFile::answers`] never matches
/// against a real command, so an unreadable answer reads as "not answered
/// yet" and the caller times out honestly instead of claiming success.
#[must_use]
pub fn load_result_or_default(path: &Path) -> UiResultFile {
    match fs::read_to_string(path) {
        Ok(contents) => UiResultFile::parse(&contents).unwrap_or_else(|e| {
            eprintln!("dbboard: ignoring unreadable ui-command-result.toml ({e})");
            UiResultFile::default()
        }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => UiResultFile::default(),
        Err(err) => {
            eprintln!("dbboard: could not read ui-command-result.toml ({err})");
            UiResultFile::default()
        }
    }
}

/// Write the command file atomically.
///
/// # Errors
///
/// - [`ConfigError::Serialize`] if TOML serialization fails.
/// - [`ConfigError::Io`] for any filesystem failure.
pub fn save_command_atomic(path: &Path, file: &UiCommandFile) -> Result<(), ConfigError> {
    save_atomic(path, &toml::to_string(file)?)
}

/// Write the result file atomically.
///
/// # Errors
///
/// - [`ConfigError::Serialize`] if TOML serialization fails.
/// - [`ConfigError::Io`] for any filesystem failure.
pub fn save_result_atomic(path: &Path, file: &UiResultFile) -> Result<(), ConfigError> {
    save_atomic(path, &toml::to_string(file)?)
}

fn save_atomic(path: &Path, serialized: &str) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_path_for(path);
    // A temp left behind by a killed process would otherwise fail every later
    // write, and unlike a connection store this file is worthless once stale.
    let _ = fs::remove_file(&tmp);
    write_new_file(&tmp, serialized.as_bytes())?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::Io(err));
    }
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(
        || std::ffi::OsString::from(".ui-command.toml"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".tmp");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent.join(name)
}

// Same user-only create as the rest of the crate (ADR-0024): a command can
// carry SQL, and SQL can carry a literal that names a customer.
fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = secure_fs::create_new_user_only(path)?;
    handle.write_all(contents)?;
    handle.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_fresh_profile_has_nothing_pending() {
        let file = UiCommandFile::default();
        assert_eq!(file.version, UI_COMMAND_VERSION);
        assert_eq!(file.seq, 0);
        assert_eq!(pending_command(&file, 0), None);
    }

    #[test]
    fn every_command_round_trips_through_toml() {
        for command in [
            UiCommand::SetEditorSql {
                sql: "SELECT 1;".to_string(),
            },
            UiCommand::RunQuery,
            UiCommand::OpenAiPanel,
        ] {
            let file = UiCommandFile {
                version: UI_COMMAND_VERSION,
                seq: 3,
                command: Some(command.clone()),
            };
            let toml = toml::to_string(&file).expect("serialize");
            let back = UiCommandFile::parse(&toml).expect("parse");
            assert_eq!(back.command, Some(command));
        }
    }

    #[test]
    fn sql_survives_verbatim_including_cjk_and_quotes() {
        // The whole point of the editor command is checking what the window
        // renders, so anything lost in transit here would be misread as a
        // font bug on screen.
        let sql = "SELECT '日本語のテキスト' AS \"列\", '한국어' , 'emoji 🎌';";
        let file = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 1,
            command: Some(UiCommand::SetEditorSql {
                sql: sql.to_string(),
            }),
        };
        let toml = toml::to_string(&file).expect("serialize");
        let back = UiCommandFile::parse(&toml).expect("parse");
        assert_eq!(
            back.command,
            Some(UiCommand::SetEditorSql {
                sql: sql.to_string()
            })
        );
    }

    #[test]
    fn multiline_sql_survives_verbatim() {
        let sql = "SELECT\n  1 AS a,\n  '改行\tタブ' AS b\n;";
        let file = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 1,
            command: Some(UiCommand::SetEditorSql {
                sql: sql.to_string(),
            }),
        };
        let toml = toml::to_string(&file).expect("serialize");
        let back = UiCommandFile::parse(&toml).expect("parse");
        let Some(UiCommand::SetEditorSql { sql: got }) = back.command else {
            panic!("wrong variant")
        };
        assert_eq!(got, sql);
    }

    #[test]
    fn the_wire_names_the_verb_in_snake_case() {
        let file = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 1,
            command: Some(UiCommand::OpenAiPanel),
        };
        let toml = toml::to_string(&file).expect("serialize");
        assert!(toml.contains("kind = \"open_ai_panel\""), "got: {toml}");
    }

    #[test]
    fn kind_matches_the_wire_tag() {
        assert_eq!(
            UiCommand::SetEditorSql { sql: String::new() }.kind(),
            "set_editor_sql"
        );
        assert_eq!(UiCommand::RunQuery.kind(), "run_query");
        assert_eq!(UiCommand::OpenAiPanel.kind(), "open_ai_panel");
        assert_eq!(UiCommand::OpenAiSettings.kind(), "open_ai_settings");
    }

    #[test]
    fn an_unknown_verb_is_a_parse_error_not_an_empty_command() {
        // A newer build's verb must not read back as "nothing to do": the
        // caller would be told the window obeyed something it ignored.
        let toml = "version = 1\nseq = 4\n[command]\nkind = \"reboot_the_universe\"\n";
        assert!(UiCommandFile::parse(toml).is_err());
    }

    #[test]
    fn parse_rejects_an_unknown_version() {
        let toml = "version = 999\nseq = 1\n";
        let err = UiCommandFile::parse(toml).expect_err("version guard");
        assert!(matches!(err, ConfigError::UnsupportedVersion(999)));
    }

    #[test]
    fn a_command_file_with_no_command_parses() {
        let file = UiCommandFile::parse("version = 1\nseq = 0\n").expect("parse");
        assert_eq!(file.command, None);
    }

    #[test]
    fn the_client_acts_only_on_a_higher_seq() {
        let file = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 5,
            command: Some(UiCommand::RunQuery),
        };
        assert_eq!(pending_command(&file, 4), Some(&UiCommand::RunQuery));
        // Already acted on: re-reading the same file on the next tick must
        // not run the query a second time.
        assert_eq!(pending_command(&file, 5), None);
    }

    #[test]
    fn a_seq_that_went_backwards_is_ignored() {
        // An older profile restored under a running window. Replaying it
        // would run a query the caller never asked for in this session.
        let file = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 2,
            command: Some(UiCommand::RunQuery),
        };
        assert_eq!(pending_command(&file, 9), None);
    }

    #[test]
    fn the_same_command_twice_is_two_events() {
        // Running the same SQL again is a real request, so the trigger is the
        // sequence number and never the command's value.
        let command = Some(UiCommand::RunQuery);
        let first = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 1,
            command: command.clone(),
        };
        let second = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 2,
            command,
        };
        assert!(pending_command(&first, 0).is_some());
        assert!(pending_command(&second, 1).is_some());
    }

    #[test]
    fn next_seq_clears_both_files() {
        assert_eq!(next_seq(0, 0), 1);
        assert_eq!(next_seq(7, 3), 8);
        // The answer outlived its command file: the next number must still
        // land above the answer, or the caller could match on it.
        assert_eq!(next_seq(0, 12), 13);
    }

    #[test]
    fn next_seq_saturates_rather_than_wrapping() {
        assert_eq!(next_seq(u64::MAX, 0), u64::MAX);
    }

    #[test]
    fn a_result_answers_only_its_own_command() {
        let result = UiResultFile::ok(6, None);
        assert!(result.answers(6));
        assert!(!result.answers(5));
        assert!(!result.answers(7));
    }

    #[test]
    fn the_default_result_answers_nothing() {
        // Sequence numbers start at 1, so an absent or unreadable answer
        // file can never be mistaken for the answer to a live command.
        let result = UiResultFile::default();
        assert!(!result.ok);
        for seq in 1..100 {
            assert!(!result.answers(seq));
        }
    }

    #[test]
    fn a_failed_result_carries_the_reason_and_round_trips() {
        let result = UiResultFile::failed(4, "no query tab is open");
        let toml = toml::to_string(&result).expect("serialize");
        let back = UiResultFile::parse(&toml).expect("parse");
        assert_eq!(back, result);
        assert!(!back.ok);
        assert_eq!(back.error.as_deref(), Some("no query tab is open"));
    }

    #[test]
    fn a_successful_result_omits_the_error_key() {
        let toml = toml::to_string(&UiResultFile::ok(2, None)).expect("serialize");
        assert!(!toml.contains("error"), "got: {toml}");
        assert!(!toml.contains("detail"), "got: {toml}");
    }

    #[test]
    fn a_result_detail_round_trips() {
        let result = UiResultFile::ok(9, Some("3 rows in 12 ms".to_string()));
        let back = UiResultFile::parse(&toml::to_string(&result).unwrap()).expect("parse");
        assert_eq!(back.detail.as_deref(), Some("3 rows in 12 ms"));
    }

    #[test]
    fn result_parse_rejects_an_unknown_version() {
        let err = UiResultFile::parse("version = 42\nseq = 1\nok = true\n").expect_err("guard");
        assert!(matches!(err, ConfigError::UnsupportedVersion(42)));
    }

    #[test]
    fn a_missing_file_loads_as_the_default() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            load_command_or_default(&dir.path().join("ui-command.toml")),
            UiCommandFile::default()
        );
        assert_eq!(
            load_result_or_default(&dir.path().join("ui-command-result.toml")),
            UiResultFile::default()
        );
    }

    #[test]
    fn a_corrupt_file_loads_as_the_default_rather_than_erroring() {
        let dir = TempDir::new().unwrap();
        let command = dir.path().join("ui-command.toml");
        let result = dir.path().join("ui-command-result.toml");
        fs::write(&command, "not = valid = toml").unwrap();
        fs::write(&result, "not = valid = toml").unwrap();
        assert_eq!(load_command_or_default(&command), UiCommandFile::default());
        assert_eq!(load_result_or_default(&result), UiResultFile::default());
    }

    #[test]
    fn save_then_load_round_trips_a_command() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-command.toml");
        let file = UiCommandFile {
            version: UI_COMMAND_VERSION,
            seq: 11,
            command: Some(UiCommand::SetEditorSql {
                sql: "SELECT '日本語';".to_string(),
            }),
        };
        save_command_atomic(&path, &file).expect("save");
        assert_eq!(load_command_or_default(&path), file);
    }

    #[test]
    fn save_then_load_round_trips_a_result() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-command-result.toml");
        let file = UiResultFile::ok(11, Some("done".to_string()));
        save_result_atomic(&path, &file).expect("save");
        assert_eq!(load_result_or_default(&path), file);
    }

    #[test]
    fn saving_twice_overwrites_in_place() {
        // The command file is rewritten on every instruction, so a leftover
        // temp from a killed process must not wedge every later write.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ui-command.toml");
        fs::write(dir.path().join("ui-command.toml.tmp"), "stale").unwrap();

        save_command_atomic(
            &path,
            &UiCommandFile {
                version: UI_COMMAND_VERSION,
                seq: 1,
                command: Some(UiCommand::RunQuery),
            },
        )
        .expect("first save");
        save_command_atomic(
            &path,
            &UiCommandFile {
                version: UI_COMMAND_VERSION,
                seq: 2,
                command: Some(UiCommand::OpenAiPanel),
            },
        )
        .expect("second save");

        let after = load_command_or_default(&path);
        assert_eq!(after.seq, 2);
        assert_eq!(after.command, Some(UiCommand::OpenAiPanel));
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("ui-command.toml");
        save_command_atomic(&path, &UiCommandFile::default()).expect("save");
        assert!(path.exists());
    }

    #[test]
    fn a_full_exchange_matches_command_to_answer() {
        // The whole protocol in one place: read both files, pick the next
        // number, write the command, have the other end answer it, match.
        let dir = TempDir::new().unwrap();
        let command_path = dir.path().join("ui-command.toml");
        let result_path = dir.path().join("ui-command-result.toml");

        let seq = next_seq(
            load_command_or_default(&command_path).seq,
            load_result_or_default(&result_path).seq,
        );
        assert_eq!(seq, 1);
        save_command_atomic(
            &command_path,
            &UiCommandFile {
                version: UI_COMMAND_VERSION,
                seq,
                command: Some(UiCommand::RunQuery),
            },
        )
        .expect("write command");

        // The client end.
        let seen = load_command_or_default(&command_path);
        let pending = pending_command(&seen, 0).expect("pending");
        assert_eq!(pending, &UiCommand::RunQuery);
        save_result_atomic(&result_path, &UiResultFile::ok(seen.seq, None)).expect("answer");

        // Back on the caller's side.
        assert!(load_result_or_default(&result_path).answers(seq));
        // And the next instruction gets a fresh number.
        assert_eq!(
            next_seq(
                load_command_or_default(&command_path).seq,
                load_result_or_default(&result_path).seq,
            ),
            2
        );
    }
}
