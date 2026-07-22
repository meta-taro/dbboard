//! `Value` → SQL literal rendering for a logical dump (ADR-0049, slice a).
//!
//! Unlike `Value`'s `Display` (which renders reals with no NaN/Inf handling
//! and blobs as a `<blob: N bytes>` placeholder — neither a valid literal),
//! this produces a syntactically valid SQL literal for **every** value in the
//! target dialect, so a dumped `INSERT` re-parses. Text reuses write-back's
//! single-quote escaping; identifiers are quoted by the `INSERT` assembler.

use std::fmt::Write as _;

use crate::value::Value;
use crate::write_back::{quote_str, SqlDialect};

/// Render `value` as a SQL literal valid in `dialect`.
///
/// - `Null` → the bare keyword `NULL`.
/// - `Integer` → bare decimal.
/// - `Real` → shortest round-tripping form when finite; a dialect-specific
///   form for NaN/±Infinity (see [`real_literal`]).
/// - `Text` → single-quoted, with embedded `'` doubled.
/// - `Blob` → `X'…'` (SQLite) or `'\x…'::bytea` (Postgres).
#[must_use]
pub fn value_literal(value: &Value, dialect: SqlDialect) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Integer(n) => n.to_string(),
        Value::Real(x) => real_literal(*x, dialect),
        Value::Text(s) => quote_str(s),
        Value::Blob(bytes) => blob_literal(bytes, dialect),
    }
}

/// A real number as a SQL literal.
///
/// Finite values use Rust's default float formatting, which is the shortest
/// decimal string that round-trips back to the same `f64`. Non-finite values
/// almost never occur in real data — SQLite stores NaN as NULL, and the
/// Postgres adapter returns every cell as text so a `Value::Real` never
/// carries NaN/Inf from that side — but the function stays total rather than
/// emitting a literal that would fail to parse:
///
/// - Postgres has real `'NaN'` / `'Infinity'` / `'-Infinity'` float literals.
/// - SQLite has none: NaN maps to `NULL` (matching SQLite's own storage of
///   NaN), and ±Infinity to the overflowing literal `±9e999`, which SQLite
///   parses to ±Inf.
fn real_literal(x: f64, dialect: SqlDialect) -> String {
    if x.is_finite() {
        return format!("{x}");
    }
    match (dialect, x.is_nan(), x.is_sign_positive()) {
        (SqlDialect::Postgres, true, _) => "'NaN'::double precision".to_owned(),
        (SqlDialect::Postgres, false, true) => "'Infinity'::double precision".to_owned(),
        (SqlDialect::Postgres, false, false) => "'-Infinity'::double precision".to_owned(),
        (SqlDialect::Sqlite, true, _) => "NULL".to_owned(),
        (SqlDialect::Sqlite, false, true) => "9e999".to_owned(),
        (SqlDialect::Sqlite, false, false) => "-9e999".to_owned(),
    }
}

/// A byte string as a SQL blob/bytea literal (lowercase hex).
fn blob_literal(bytes: &[u8], dialect: SqlDialect) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Two lowercase hex digits per byte; both engines accept either case.
        let _ = write!(hex, "{b:02x}");
    }
    match dialect {
        SqlDialect::Sqlite => format!("X'{hex}'"),
        // Standard-conforming strings (default on modern Postgres, so on
        // Supabase and Aurora DSQL): the backslash is literal and bytea's
        // hex input format parses `\xHEX`.
        SqlDialect::Postgres => format!("'\\x{hex}'::bytea"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_is_the_bare_keyword() {
        assert_eq!(value_literal(&Value::Null, SqlDialect::Sqlite), "NULL");
        assert_eq!(value_literal(&Value::Null, SqlDialect::Postgres), "NULL");
    }

    #[test]
    fn integers_are_bare_decimal() {
        assert_eq!(value_literal(&Value::Integer(42), SqlDialect::Sqlite), "42");
        assert_eq!(
            value_literal(&Value::Integer(-7), SqlDialect::Postgres),
            "-7"
        );
        assert_eq!(
            value_literal(&Value::Integer(i64::MIN), SqlDialect::Sqlite),
            i64::MIN.to_string()
        );
    }

    #[test]
    fn finite_reals_round_trip() {
        for x in [0.1_f64, 1.5, -2.25, 1e-7, 1e20, 0.0] {
            let lit = value_literal(&Value::Real(x), SqlDialect::Sqlite);
            let parsed: f64 = lit.parse().expect("literal should parse as f64");
            // Bit-exact round-trip (clippy forbids `==` on floats).
            assert_eq!(
                parsed.to_bits(),
                x.to_bits(),
                "literal {lit:?} did not round-trip"
            );
        }
    }

    #[test]
    fn nan_is_null_on_sqlite_and_a_cast_on_postgres() {
        assert_eq!(
            value_literal(&Value::Real(f64::NAN), SqlDialect::Sqlite),
            "NULL"
        );
        assert_eq!(
            value_literal(&Value::Real(f64::NAN), SqlDialect::Postgres),
            "'NaN'::double precision"
        );
    }

    #[test]
    fn infinities_get_a_dialect_specific_form() {
        assert_eq!(
            value_literal(&Value::Real(f64::INFINITY), SqlDialect::Sqlite),
            "9e999"
        );
        assert_eq!(
            value_literal(&Value::Real(f64::NEG_INFINITY), SqlDialect::Sqlite),
            "-9e999"
        );
        assert_eq!(
            value_literal(&Value::Real(f64::INFINITY), SqlDialect::Postgres),
            "'Infinity'::double precision"
        );
        assert_eq!(
            value_literal(&Value::Real(f64::NEG_INFINITY), SqlDialect::Postgres),
            "'-Infinity'::double precision"
        );
    }

    #[test]
    fn text_is_single_quoted_with_quotes_doubled() {
        assert_eq!(
            value_literal(&Value::Text("hello".into()), SqlDialect::Sqlite),
            "'hello'"
        );
        assert_eq!(
            value_literal(&Value::Text("O'Brien".into()), SqlDialect::Postgres),
            "'O''Brien'"
        );
    }

    #[test]
    fn text_keeps_a_sql_injection_payload_inside_one_literal() {
        let payload = "x'; DROP TABLE t;--";
        assert_eq!(
            value_literal(&Value::Text(payload.into()), SqlDialect::Sqlite),
            "'x''; DROP TABLE t;--'"
        );
    }

    #[test]
    fn empty_text_is_an_empty_quoted_string() {
        assert_eq!(
            value_literal(&Value::Text(String::new()), SqlDialect::Sqlite),
            "''"
        );
    }

    #[test]
    fn blob_is_hex_on_sqlite_and_bytea_on_postgres() {
        let blob = Value::Blob(vec![0x0a, 0xff, 0x00]);
        assert_eq!(value_literal(&blob, SqlDialect::Sqlite), "X'0aff00'");
        assert_eq!(
            value_literal(&blob, SqlDialect::Postgres),
            "'\\x0aff00'::bytea"
        );
    }

    #[test]
    fn empty_blob_renders_an_empty_hex_literal() {
        assert_eq!(
            value_literal(&Value::Blob(Vec::new()), SqlDialect::Sqlite),
            "X''"
        );
        assert_eq!(
            value_literal(&Value::Blob(Vec::new()), SqlDialect::Postgres),
            "'\\x'::bytea"
        );
    }
}
