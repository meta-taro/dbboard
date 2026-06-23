//! Emit a `history.jsonl` fixture for the `dbboard-web` sibling's
//! cross-implementation round-trip test (see web's handoff brief
//! `2026-06-23-history-fixture-emit-outgoing.md` and ADR-0017 §6).
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run --example emit_history_fixture -p dbboard-ui > desktop-history.jsonl
//! ```
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

use std::io::{self, Write};

use dbboard_ui::{fixture, HistoryEntry, HistoryError, HistoryStatus};

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

fn main() -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    run(&mut handle)
}

#[cfg(test)]
mod tests {
    use super::run;

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
}
