//! Keyset-paginated `SELECT` assembly for a logical dump (ADR-0049, slice
//! d).
//!
//! The orchestrator reads each table one page at a time so no single query
//! ever crosses [`MAX_RESULT_ROWS`](crate::limits::MAX_RESULT_ROWS). When a
//! table has a primary key the pages are *keyset* — ordered by the key,
//! each page fetching rows strictly after the previous page's last key via
//! a row-value comparison. Keyset paging is stable under inserts and stays
//! O(1) per page, unlike `OFFSET` which re-scans from the top each time.
//!
//! Cursor values are rendered as SQL literals (the same
//! [`value_literal`](crate::dump::value_literal) write-back shares), so
//! this composes with the simple-query execution path the dump uses.

use std::fmt::Write as _;

use crate::dump::value_literal;
use crate::schema::TableInfo;
use crate::value::Value;
use crate::write_back::{qualified_table, quote_ident, SqlDialect};

/// Build one page's `SELECT` for `table`.
///
/// - `key_columns` is the primary key in key order. When empty, the page is
///   an unordered `SELECT * … LIMIT n` (the caller must not attempt to page
///   further — there is no stable cursor).
/// - `after` is the previous page's last row's key values, positionally
///   matching `key_columns`. `None` fetches the first page. Ignored when
///   `key_columns` is empty.
/// - `limit` bounds the page size; the caller keeps it under
///   `MAX_RESULT_ROWS`.
///
/// The row-value comparison `(k1, k2) > (v1, v2)` gives correct composite-
/// key ordering on both SQLite (3.15+) and Postgres.
#[must_use]
pub fn build_select_page(
    table: &TableInfo,
    key_columns: &[String],
    dialect: SqlDialect,
    limit: usize,
    after: Option<&[Value]>,
) -> String {
    let mut sql = format!("SELECT * FROM {}", qualified_table(table, dialect));

    if !key_columns.is_empty() {
        let key_list = key_columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");

        // A keyset cursor only applies from the second page on.
        if let Some(cursor) = after {
            let value_list = cursor
                .iter()
                .map(|v| value_literal(v, dialect))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(sql, " WHERE ({key_list}) > ({value_list})");
        }

        let _ = write!(sql, " ORDER BY {key_list}");
    }

    let _ = write!(sql, " LIMIT {limit}");
    sql
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableInfo {
        TableInfo::unqualified("users")
    }

    #[test]
    fn first_page_with_a_single_key_orders_and_limits() {
        let sql = build_select_page(&table(), &["id".into()], SqlDialect::Sqlite, 500, None);
        assert_eq!(sql, "SELECT * FROM \"users\" ORDER BY \"id\" LIMIT 500");
    }

    #[test]
    fn a_later_page_adds_a_keyset_predicate() {
        let sql = build_select_page(
            &table(),
            &["id".into()],
            SqlDialect::Sqlite,
            500,
            Some(&[Value::Integer(42)]),
        );
        assert_eq!(
            sql,
            "SELECT * FROM \"users\" WHERE (\"id\") > (42) ORDER BY \"id\" LIMIT 500"
        );
    }

    #[test]
    fn a_composite_key_uses_a_row_value_comparison() {
        let sql = build_select_page(
            &table(),
            &["order_id".into(), "line_no".into()],
            SqlDialect::Postgres,
            100,
            Some(&[Value::Integer(7), Value::Integer(3)]),
        );
        assert_eq!(
            sql,
            "SELECT * FROM \"users\" WHERE (\"order_id\", \"line_no\") > (7, 3) \
             ORDER BY \"order_id\", \"line_no\" LIMIT 100"
        );
    }

    #[test]
    fn a_text_cursor_is_quoted_as_a_literal() {
        let sql = build_select_page(
            &table(),
            &["email".into()],
            SqlDialect::Postgres,
            50,
            Some(&[Value::Text("a'b".into())]),
        );
        assert_eq!(
            sql,
            "SELECT * FROM \"users\" WHERE (\"email\") > ('a''b') ORDER BY \"email\" LIMIT 50"
        );
    }

    #[test]
    fn no_key_columns_falls_back_to_an_unordered_limited_scan() {
        // A keyless table cannot be keyset-paged; the caller takes a single
        // capped page and records the truncation rather than paging blindly.
        let sql = build_select_page(&table(), &[], SqlDialect::Sqlite, 5000, None);
        assert_eq!(sql, "SELECT * FROM \"users\" LIMIT 5000");
    }

    #[test]
    fn a_postgres_schema_qualifies_the_table() {
        let sql = build_select_page(
            &TableInfo::qualified("public", "users"),
            &["id".into()],
            SqlDialect::Postgres,
            10,
            None,
        );
        assert_eq!(
            sql,
            "SELECT * FROM \"public\".\"users\" ORDER BY \"id\" LIMIT 10"
        );
    }
}
