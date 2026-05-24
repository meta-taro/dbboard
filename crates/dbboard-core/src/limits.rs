//! Workspace-wide policy limits enforced by every adapter.
//!
//! The cap protects the UI from an accidental `SELECT * FROM huge_table`:
//! every adapter loads results fully into memory (Phase 1 has no
//! streaming, see `docs/roadmap.md`), so an unbounded result set is an
//! unbounded allocation. Exceeding the cap raises [`too_many_rows_error`]
//! rather than truncating, so the UI never silently shows a partial
//! result.

use crate::DbError;

/// Maximum number of rows a single query is allowed to return.
///
/// Sized for a desktop SQL editor where 10k rows is already more than a
/// grid can show usefully — the right answer for a bigger result is a
/// `LIMIT` clause, not a larger buffer. Phase 2 may replace the cap with
/// real pagination or streaming once the adapter trait lands.
pub const MAX_RESULT_ROWS: usize = 10_000;

/// Standard [`DbError::Query`] raised by every adapter when a result set
/// would exceed [`MAX_RESULT_ROWS`]. Centralised so the message stays
/// consistent across adapters (Turso, D1, Postgres) and across the
/// desktop / web siblings.
#[must_use]
pub fn too_many_rows_error() -> DbError {
    DbError::Query(format!(
        "result set exceeds the {MAX_RESULT_ROWS}-row cap; add a LIMIT clause to narrow it"
    ))
}

#[cfg(test)]
mod tests {
    use super::{too_many_rows_error, MAX_RESULT_ROWS};
    use crate::DbError;

    #[test]
    fn too_many_rows_is_a_query_error() {
        assert!(matches!(too_many_rows_error(), DbError::Query(_)));
    }

    #[test]
    fn too_many_rows_message_mentions_the_cap_and_limit_hint() {
        let DbError::Query(msg) = too_many_rows_error() else {
            unreachable!("checked by too_many_rows_is_a_query_error");
        };
        assert!(
            msg.contains(&MAX_RESULT_ROWS.to_string()),
            "message should mention the cap value, got: {msg}"
        );
        assert!(
            msg.to_ascii_uppercase().contains("LIMIT"),
            "message should hint at LIMIT, got: {msg}"
        );
    }
}
