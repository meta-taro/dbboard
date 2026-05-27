//! Domain-level errors that every adapter must map onto.
//!
//! Adapters convert driver-specific failures (libsql, sqlx, ...) into
//! one of these variants so the UI can render a stable taxonomy of
//! error categories regardless of which database is connected.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum DbError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("query failed: {0}")]
    Query(String),

    #[error("schema introspection failed: {0}")]
    Schema(String),

    #[error("type conversion failed: {0}")]
    TypeConversion(String),

    /// The adapter does not expose the requested capability (ADR-0012).
    /// Distinguished from `Query` so the UI can hide or grey out the
    /// affected feature instead of presenting it as a user SQL error.
    #[error("capability unavailable: {0}")]
    Capability(String),
}

impl DbError {
    /// Stable, lowercase category string for the HTTP error envelope
    /// (`docs/api-contract.md`). Shared verbatim with dbboard-web.
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            Self::Connection(_) => "connection",
            Self::Query(_) => "query",
            Self::Schema(_) => "schema",
            Self::TypeConversion(_) => "type_conversion",
            Self::Capability(_) => "capability",
        }
    }

    /// The inner detail string, without the category prefix that
    /// [`Display`](std::fmt::Display) prepends. This is what travels in
    /// the wire envelope's `message` field so a round-trip through
    /// [`Self::from_parts`] does not double up the prefix.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Connection(m)
            | Self::Query(m)
            | Self::Schema(m)
            | Self::TypeConversion(m)
            | Self::Capability(m) => m,
        }
    }

    /// Reconstruct a `DbError` from the wire envelope's `category` and
    /// `message`. An unknown category degrades to [`DbError::Query`] so
    /// a contract drift surfaces as a visible error rather than a panic.
    #[must_use]
    pub fn from_parts(category: &str, message: String) -> Self {
        match category {
            "connection" => Self::Connection(message),
            "schema" => Self::Schema(message),
            "type_conversion" => Self::TypeConversion(message),
            "capability" => Self::Capability(message),
            _ => Self::Query(message),
        }
    }
}

pub type DbResult<T> = Result<T, DbError>;

#[cfg(test)]
mod tests {
    use super::DbError;

    #[test]
    fn connection_error_renders_category_and_message() {
        let e = DbError::Connection("host unreachable".into());
        assert_eq!(e.to_string(), "connection failed: host unreachable");
    }

    #[test]
    fn query_error_renders_category_and_message() {
        let e = DbError::Query("syntax near SELEC".into());
        assert_eq!(e.to_string(), "query failed: syntax near SELEC");
    }

    #[test]
    fn schema_error_renders_category_and_message() {
        let e = DbError::Schema("sqlite_master unreadable".into());
        assert_eq!(
            e.to_string(),
            "schema introspection failed: sqlite_master unreadable"
        );
    }

    #[test]
    fn type_conversion_error_renders_category_and_message() {
        let e = DbError::TypeConversion("BLOB into i64".into());
        assert_eq!(e.to_string(), "type conversion failed: BLOB into i64");
    }

    #[test]
    fn capability_error_renders_category_and_message() {
        let e = DbError::Capability("views not supported on turso".into());
        assert_eq!(
            e.to_string(),
            "capability unavailable: views not supported on turso"
        );
    }

    #[test]
    fn category_strings_match_the_contract() {
        assert_eq!(DbError::Connection(String::new()).category(), "connection");
        assert_eq!(DbError::Query(String::new()).category(), "query");
        assert_eq!(DbError::Schema(String::new()).category(), "schema");
        assert_eq!(
            DbError::TypeConversion(String::new()).category(),
            "type_conversion"
        );
        assert_eq!(DbError::Capability(String::new()).category(), "capability");
    }

    #[test]
    fn from_parts_round_trips_category_and_message() {
        for e in [
            DbError::Connection("c".into()),
            DbError::Query("q".into()),
            DbError::Schema("s".into()),
            DbError::TypeConversion("t".into()),
            DbError::Capability("cap".into()),
        ] {
            let back = DbError::from_parts(e.category(), e.message().to_owned());
            assert_eq!(back.category(), e.category());
            assert_eq!(back.message(), e.message());
        }
    }

    #[test]
    fn message_strips_the_category_prefix() {
        let e = DbError::Query("syntax near SELEC".into());
        assert_eq!(e.message(), "syntax near SELEC");
        assert_eq!(e.to_string(), "query failed: syntax near SELEC");
    }

    #[test]
    fn from_parts_degrades_unknown_category_to_query() {
        let e = DbError::from_parts("bogus", "oops".into());
        assert_eq!(e.category(), "query");
    }
}
