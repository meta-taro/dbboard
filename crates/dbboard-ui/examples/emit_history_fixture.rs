//! Emit a `history.jsonl` fixture for the `dbboard-web` sibling's
//! cross-implementation round-trip test (see web's handoff brief
//! `2026-06-23-history-fixture-emit-outgoing.md` and ADR-0017 §6).
//!
//! Run from the workspace root. Two modes:
//!
//! ```text
//! cargo run --example emit_history_fixture -p dbboard-ui -- --output desktop-history.jsonl
//! cargo run --example emit_history_fixture -p dbboard-ui                    # stdout
//! ```
//!
//! Prefer `--output PATH` (or its short alias `-o PATH`): the bytes go
//! through `File::create` + `write_all` so no shell ever re-encodes
//! them. PowerShell's `>` redirection in particular defaults to
//! UTF-16 LE + CRLF on Windows PowerShell 5.x and to UTF-8 + CRLF on
//! PowerShell 7+, both of which silently violate web's byte
//! equivalence check. The stdout mode is retained for piping into
//! bytewise-safe shells (Git Bash, `cmd /c "... > file"`) and for the
//! in-memory smoke test.
//!
//! The output bytes go through the production [`RecordWire`] serialiser
//! via [`dbboard_ui::history::fixture`] so they are byte-identical to
//! what `PersistentHistoryStore::record_completion` would append on
//! disk. Field order matches the `RecordWire` declaration:
//! `v, ts, conn, actor, sql, status, duration_ms, rows, rows_affected,
//! error`.
//!
//! Output conventions (per web's brief):
//!
//! * One JSON object per line, terminated with `\n` (LF, never CRLF).
//! * File ends with a trailing `\n` after the last record.
//! * No whitespace inside the JSON objects.
//! * `actor` / `rows` / `rows_affected` / `error` are emitted as `null`
//!   when not applicable — keys are never omitted, because web's byte
//!   equivalence check requires the same key set on both sides.
//!
//! Coverage:
//!
//! 1. `status="ok"` with `rows` (SELECT-shaped success).
//! 2. `status="ok"` with `rows_affected` (DML-shaped success).
//! 3. `status="ok"` with both `null` (EXPLAIN / SET / BEGIN — legitimate
//!    per ADR-0017 §2).
//! 4. One `status="error"` line per [`CategorizedError`] category:
//!    `query`, `connection`, `schema`, `type_conversion`, `capability`.
//! 5. A forward-compat record carrying an unknown top-level field
//!    (`"unknown_field":"value-from-the-future"`) — web's Zod schema
//!    silently strips it on re-emit per ADR-0017 §6.
//! 6. One line with a populated `actor` (desktop normally writes
//!    `null`; the fixture exercises the field once so web verifies it
//!    round-trips).
//! 7. A `duration_ms` range (`0`, `42`, `1234`) so number-formatting
//!    edge cases are covered.
//!
//! [`CategorizedError`]: dbboard_core
//! [`RecordWire`]: dbboard_ui::history

use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dbboard_ui::{fixture, HistoryEntry, HistoryError, HistoryStatus};

const USAGE: &str = "\
usage: emit_history_fixture [--output PATH | -o PATH]

Emit the dbboard ADR-0017 cross-implementation round-trip fixture.

With --output PATH the bytes are written to PATH via File::create + \
write_all, bypassing any shell re-encoding. Without it, the bytes are \
written to stdout (use a byte-safe shell — Git Bash or `cmd /c`).
";

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Stdout,
    File(PathBuf),
    Help,
}

#[derive(Debug, PartialEq, Eq)]
enum ParseError {
    UnknownArg(String),
    MissingValue(&'static str),
    TrailingArg(String),
}

fn parse_args<I, S>(args: I) -> Result<Mode, ParseError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut iter = args.into_iter().map(Into::into);
    let mut mode = Mode::Stdout;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Mode::Help),
            "-o" | "--output" => {
                let value = iter.next().ok_or(ParseError::MissingValue("--output"))?;
                mode = Mode::File(PathBuf::from(value));
            }
            other => {
                // Reject unknown flags so a typo like `--out` cannot
                // silently fall through to stdout and corrupt the
                // ship-it-to-web step.
                return Err(if other.starts_with('-') {
                    ParseError::UnknownArg(other.to_owned())
                } else {
                    ParseError::TrailingArg(other.to_owned())
                });
            }
        }
    }
    Ok(mode)
}

fn ok_with_rows(ts: &str, sql: &str, duration_ms: u64, rows: u64) -> HistoryEntry {
    HistoryEntry {
        sql: sql.to_string(),
        ts: ts.to_string(),
        conn: "prod-pg".to_string(),
        status: HistoryStatus::Ok,
        duration_ms,
        rows: Some(rows),
        rows_affected: None,
        error: None,
    }
}

fn ok_with_rows_affected(ts: &str, sql: &str, duration_ms: u64, affected: u64) -> HistoryEntry {
    HistoryEntry {
        sql: sql.to_string(),
        ts: ts.to_string(),
        conn: "prod-pg".to_string(),
        status: HistoryStatus::Ok,
        duration_ms,
        rows: None,
        rows_affected: Some(affected),
        error: None,
    }
}

fn ok_with_both_null(ts: &str, sql: &str, duration_ms: u64) -> HistoryEntry {
    HistoryEntry {
        sql: sql.to_string(),
        ts: ts.to_string(),
        conn: "prod-pg".to_string(),
        status: HistoryStatus::Ok,
        duration_ms,
        rows: None,
        rows_affected: None,
        error: None,
    }
}

fn error_for(ts: &str, sql: &str, duration_ms: u64, category: &str, message: &str) -> HistoryEntry {
    HistoryEntry {
        sql: sql.to_string(),
        ts: ts.to_string(),
        conn: "prod-pg".to_string(),
        status: HistoryStatus::Error,
        duration_ms,
        rows: None,
        rows_affected: None,
        error: Some(HistoryError {
            category: category.to_string(),
            message: message.to_string(),
        }),
    }
}

fn emit_line(out: &mut impl Write, line: &str) -> io::Result<()> {
    // `write_all` + explicit `\n` guarantees LF on every platform; we
    // bypass `println!` so the Windows console's text-mode CRLF
    // translation never fires when stdout is piped to a file.
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

fn emit_ok_cases(out: &mut impl Write) -> io::Result<()> {
    // SELECT-shaped success with rows + duration_ms=42 (mid-range).
    emit_line(
        out,
        &fixture::serialize(
            &ok_with_rows("2026-06-23T10:00:00.000Z", "SELECT 1", 42, 1),
            None,
        ),
    )?;
    // DML-shaped success with rows_affected + duration_ms=0
    // (lower-bound number-formatting case).
    emit_line(
        out,
        &fixture::serialize(
            &ok_with_rows_affected(
                "2026-06-23T10:00:01.000Z",
                "UPDATE users SET active=true WHERE id=1",
                0,
                1,
            ),
            None,
        ),
    )?;
    // Both-null success (EXPLAIN / SET / BEGIN — legitimate per
    // ADR-0017 §2). duration_ms=1234 exercises the upper-bound format.
    emit_line(
        out,
        &fixture::serialize(
            &ok_with_both_null("2026-06-23T10:00:02.000Z", "BEGIN", 1234),
            None,
        ),
    )?;
    Ok(())
}

fn emit_error_cases(out: &mut impl Write) -> io::Result<()> {
    // One line per CategorizedError category (ADR-0017 §2).
    let cases: [(&str, &str, u64, &str, &str); 5] = [
        (
            "2026-06-23T10:00:03.000Z",
            "SELCT 1",
            3,
            "query",
            "syntax error at or near 'SELCT'",
        ),
        (
            "2026-06-23T10:00:04.000Z",
            "SELECT 1",
            7,
            "connection",
            "connection refused: dial tcp 127.0.0.1:5432",
        ),
        (
            "2026-06-23T10:00:05.000Z",
            "SELECT * FROM nonexistent",
            4,
            "schema",
            "relation \"nonexistent\" does not exist",
        ),
        (
            "2026-06-23T10:00:06.000Z",
            "SELECT '2026-13-99'::date",
            2,
            "type_conversion",
            "invalid input syntax for type date: \"2026-13-99\"",
        ),
        (
            "2026-06-23T10:00:07.000Z",
            "LISTEN channel",
            1,
            "capability",
            "LISTEN/NOTIFY not supported by this adapter",
        ),
    ];
    for (ts, sql, duration_ms, category, message) in cases {
        emit_line(
            out,
            &fixture::serialize(&error_for(ts, sql, duration_ms, category, message), None),
        )?;
    }
    Ok(())
}

fn emit_special_cases(out: &mut impl Write) -> io::Result<()> {
    // Forward-compat record with an unknown top-level field. The
    // fixture appends this *after* the standard envelope so a future
    // reader that drops unknown keys still recovers the known payload
    // (ADR-0017 §6).
    emit_line(
        out,
        &fixture::serialize_with_extra(
            &ok_with_rows("2026-06-23T10:00:08.000Z", "SELECT 2", 5, 1),
            "unknown_field",
            "value-from-the-future",
        ),
    )?;
    // Actor populated. Desktop never writes a non-null actor in
    // production (ADR-0016 / ADR-0017 single-user, single-process); the
    // fixture overrides it once so web can verify the field round-trips.
    emit_line(
        out,
        &fixture::serialize(
            &ok_with_rows("2026-06-23T10:00:09.000Z", "SELECT 3", 11, 1),
            Some("alice@example.com"),
        ),
    )?;
    Ok(())
}

fn run(out: &mut impl Write) -> io::Result<()> {
    emit_ok_cases(out)?;
    emit_error_cases(out)?;
    emit_special_cases(out)?;
    out.flush()
}

fn run_to_path(path: &Path) -> io::Result<()> {
    // `File::create` truncates an existing target so a re-run after a
    // shell-corrupted attempt cleanly replaces the bytes. `BufWriter`
    // batches the per-line writes; `run` already flushes at the end.
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    run(&mut writer)
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    let mode = match parse_args(argv) {
        Ok(mode) => mode,
        Err(err) => {
            let detail = match err {
                ParseError::UnknownArg(a) => format!("unknown argument: {a}"),
                ParseError::MissingValue(flag) => format!("{flag} needs a PATH"),
                ParseError::TrailingArg(a) => format!("unexpected positional argument: {a}"),
            };
            eprintln!("error: {detail}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let result = match mode {
        Mode::Help => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Mode::Stdout => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            run(&mut handle)
        }
        Mode::File(path) => run_to_path(&path),
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{parse_args, run, run_to_path, Mode, ParseError};
    use std::path::PathBuf;

    /// End-to-end smoke test: every line we emit must be valid UTF-8
    /// JSON, the file must end in `\n`, no `\r` anywhere, and the
    /// declared cases must all be present. Pins both the line count
    /// and the categorical coverage so a future refactor of the
    /// fixture (e.g. adding cases) is a deliberate change.
    #[test]
    fn fixture_output_matches_brief_conventions() {
        let mut buf: Vec<u8> = Vec::new();
        run(&mut buf).expect("run must succeed against an in-memory writer");
        let text = std::str::from_utf8(&buf).expect("fixture must be valid UTF-8");

        assert!(text.ends_with('\n'), "fixture must end with a trailing LF");
        assert!(
            !text.contains('\r'),
            "fixture must be LF-only, not CRLF: {text:?}"
        );

        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.len(),
            10,
            "fixture must emit 10 lines (3 ok + 5 error + 1 forward-compat + 1 actor)"
        );

        for line in &lines {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("every line must be valid JSON");
            // Envelope shape guard — these keys must always be present
            // (web's byte-equivalence check depends on the key set).
            for required in [
                "v",
                "ts",
                "conn",
                "actor",
                "sql",
                "status",
                "duration_ms",
                "rows",
                "rows_affected",
                "error",
            ] {
                assert!(
                    parsed.get(required).is_some(),
                    "{required} key missing in line: {line}"
                );
            }
            assert_eq!(parsed["v"], 1);
        }

        // Categorical coverage: one line per error category.
        for category in [
            "query",
            "connection",
            "schema",
            "type_conversion",
            "capability",
        ] {
            let needle = format!(r#""category":"{category}""#);
            assert!(
                text.contains(&needle),
                "fixture must cover error category {category}"
            );
        }

        // Forward-compat unknown field appears exactly once.
        assert_eq!(
            text.matches(r#""unknown_field":"value-from-the-future""#)
                .count(),
            1,
            "forward-compat case must appear exactly once"
        );

        // Actor override appears exactly once.
        assert_eq!(
            text.matches(r#""actor":"alice@example.com""#).count(),
            1,
            "actor override must appear exactly once"
        );

        // duration_ms range coverage (0 / 42 / 1234 all appear).
        for duration in [
            r#""duration_ms":0"#,
            r#""duration_ms":42"#,
            r#""duration_ms":1234"#,
        ] {
            assert!(text.contains(duration), "fixture must exercise {duration}");
        }
    }

    /// `run_to_path` must produce byte-identical output to the
    /// in-memory `run`. This is the contract that lets the maintainer
    /// trust `--output PATH` as a drop-in replacement for the
    /// shell-redirect path — the bytes web's round-trip check sees
    /// are exactly the bytes the in-memory smoke test pins above.
    #[test]
    fn run_to_path_is_byte_identical_to_in_memory_run() {
        let mut in_memory: Vec<u8> = Vec::new();
        run(&mut in_memory).expect("in-memory run must succeed");

        let dir = tempfile::tempdir().expect("tempdir must succeed");
        let path = dir.path().join("fixture.jsonl");
        run_to_path(&path).expect("run_to_path must succeed");
        let on_disk = std::fs::read(&path).expect("must read back the fixture");

        assert_eq!(
            on_disk, in_memory,
            "run_to_path bytes must equal run bytes byte-for-byte"
        );
        // Belt-and-braces: assert LF-only on the on-disk bytes
        // explicitly, in case a future writer wrapper sneaks in CRLF
        // translation. (Vec<u8> can't sneak that, but a future
        // `File::create` wrapper or buffered text mode could.)
        assert!(
            !on_disk.contains(&b'\r'),
            "on-disk fixture must be LF-only, not CRLF"
        );
        assert_eq!(
            on_disk.last(),
            Some(&b'\n'),
            "on-disk fixture must end with a trailing LF"
        );
    }

    /// `run_to_path` must truncate an existing target so a re-run
    /// cleanly replaces the bytes — important because the documented
    /// recovery path from a shell-corrupted attempt is "just re-run
    /// with --output". A leftover-byte concat would silently violate
    /// web's byte-equivalence check.
    #[test]
    fn run_to_path_truncates_existing_target() {
        let dir = tempfile::tempdir().expect("tempdir must succeed");
        let path = dir.path().join("fixture.jsonl");
        std::fs::write(&path, b"stale bytes that should disappear\n")
            .expect("seed write must succeed");

        run_to_path(&path).expect("run_to_path must succeed");
        let on_disk = std::fs::read(&path).expect("must read back the fixture");

        assert!(
            !on_disk.starts_with(b"stale bytes"),
            "run_to_path must truncate; saw stale bytes at start: {:?}",
            String::from_utf8_lossy(&on_disk[..32.min(on_disk.len())])
        );

        let mut fresh: Vec<u8> = Vec::new();
        run(&mut fresh).expect("fresh run must succeed");
        assert_eq!(on_disk, fresh, "re-run must equal a fresh run");
    }

    #[test]
    fn parse_args_no_args_is_stdout() {
        assert_eq!(parse_args(Vec::<String>::new()), Ok(Mode::Stdout));
    }

    #[test]
    fn parse_args_long_output_flag() {
        assert_eq!(
            parse_args(vec!["--output", "fixture.jsonl"]),
            Ok(Mode::File(PathBuf::from("fixture.jsonl")))
        );
    }

    #[test]
    fn parse_args_short_output_flag() {
        assert_eq!(
            parse_args(vec!["-o", "fixture.jsonl"]),
            Ok(Mode::File(PathBuf::from("fixture.jsonl")))
        );
    }

    #[test]
    fn parse_args_help_long_and_short() {
        assert_eq!(parse_args(vec!["--help"]), Ok(Mode::Help));
        assert_eq!(parse_args(vec!["-h"]), Ok(Mode::Help));
    }

    #[test]
    fn parse_args_unknown_flag_is_rejected() {
        // Typo guard — a future maintainer who types `--out` must not
        // silently fall through to stdout and ship corrupted bytes.
        assert_eq!(
            parse_args(vec!["--out", "fixture.jsonl"]),
            Err(ParseError::UnknownArg("--out".to_owned()))
        );
    }

    #[test]
    fn parse_args_missing_output_value_is_rejected() {
        assert_eq!(
            parse_args(vec!["--output"]),
            Err(ParseError::MissingValue("--output"))
        );
        assert_eq!(
            parse_args(vec!["-o"]),
            Err(ParseError::MissingValue("--output"))
        );
    }

    #[test]
    fn parse_args_positional_is_rejected() {
        // Force the explicit flag form rather than overloading
        // positionals — keeps the call sites self-documenting in
        // PowerShell / cmd / bash transcripts.
        assert_eq!(
            parse_args(vec!["fixture.jsonl"]),
            Err(ParseError::TrailingArg("fixture.jsonl".to_owned()))
        );
    }

    #[test]
    fn parse_args_help_short_circuits_before_other_flags() {
        // `--help` first means show help and exit, regardless of
        // anything that follows.
        assert_eq!(
            parse_args(vec!["--help", "--output", "fixture.jsonl"]),
            Ok(Mode::Help)
        );
    }

    #[test]
    fn parse_args_last_output_wins() {
        // Conventional CLI semantics — the last `--output` wins so a
        // wrapper script can append `--output` to override an earlier
        // default without surprising errors.
        assert_eq!(
            parse_args(vec!["--output", "first.jsonl", "-o", "second.jsonl"]),
            Ok(Mode::File(PathBuf::from("second.jsonl")))
        );
    }
}
