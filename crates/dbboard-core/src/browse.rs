//! Keyset-paged table browsing (ADR-0145).
//!
//! The grid used to show a table's first `n` rows and stop there: the bound
//! was a truncation, never a first page, so row `n + 1` was unreachable.
//! This module turns that bound into a page.
//!
//! It is the same keyset mechanism the logical dump has used since
//! ADR-0049, and deliberately the same code:
//! [`build_select_page`](crate::build_select_page) renders the statement and
//! [`cursor_from_last_row`](crate::cursor_from_last_row) reads the cursor
//! back out of the result. Nothing is held between pages — the cursor is
//! the previous page's last key, not a database cursor — so a dropped
//! connection, a closed tab, or a window left open overnight leave nothing
//! to clean up, and a row inserted between two pages cannot shift the rows
//! either side of the boundary the way `OFFSET` lets it.
//!
//! Only the *generated* browse query pages. A statement a person typed is
//! run as written (ADR-0145): rewriting it would make the tool argue with
//! the `LIMIT` they wrote, in the one place a database client must not be
//! clever.

use crate::adapter::DatabaseAdapter;
use crate::dump::{build_select_page, cursor_from_last_row};
use crate::error::DbResult;
use crate::limits::MAX_RESULT_ROWS;
use crate::row::QueryResult;
use crate::schema::TableInfo;
use crate::value::Value;
use crate::write_back::SqlDialect;

/// Read one page of `table`.
///
/// `key_columns` is the primary key in key order, empty for a table that
/// has none. `after` is the previous page's
/// [`next_cursor`](QueryResult::next_cursor); `None` reads the first page.
///
/// The returned [`QueryResult`] carries at most `page_rows` rows plus the
/// two paging fields. A keyless table can still report
/// [`has_more`](QueryResult::has_more) while offering no
/// [`next_cursor`](QueryResult::next_cursor): there is no stable order to
/// resume from, and saying so is the honest form of "first page only".
///
/// # Errors
///
/// Surfaces whatever the adapter's read-only path returns.
pub async fn browse_page(
    adapter: &dyn DatabaseAdapter,
    dialect: SqlDialect,
    table: &TableInfo,
    key_columns: &[String],
    page_rows: usize,
    after: Option<&[Value]>,
) -> DbResult<QueryResult> {
    // One row past the page distinguishes a full page from a last page
    // without a second round trip. It is also why the page itself stops one
    // short of the workspace cap: the probe has to stay legal.
    let page_rows = page_rows.clamp(1, MAX_RESULT_ROWS - 1);
    let probe = page_rows + 1;

    let sql = build_select_page(table, key_columns, dialect, probe, after);
    let mut result = adapter.query_read_only(&sql, probe).await?;

    let has_more = result.rows.len() > page_rows;
    result.truncate_rows(page_rows);

    // A keyless table has no stable order to resume from, so it reports the
    // rows it can see and no way to reach the rest — rather than an
    // `OFFSET` that would silently skip or repeat rows under an insert.
    let next_cursor = if has_more && !key_columns.is_empty() {
        let columns: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
        cursor_from_last_row(&result.rows, &columns, key_columns)
    } else {
        None
    };

    result.has_more = has_more;
    result.next_cursor = next_cursor;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capabilities;
    use crate::error::DbError;
    use crate::row::{Column, Row};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// Records the SQL it was asked to run and replies with `available`
    /// rows of a two-column `(id, name)` table, honouring the statement's
    /// row cap the way a real adapter's read-only path does.
    struct RecordingAdapter {
        available: usize,
        seen: Mutex<Vec<String>>,
    }

    impl RecordingAdapter {
        fn with_rows(available: usize) -> Self {
            Self {
                available,
                seen: Mutex::new(Vec::new()),
            }
        }

        fn last_sql(&self) -> String {
            self.seen.lock().expect("lock").last().cloned().unwrap()
        }
    }

    #[async_trait]
    impl DatabaseAdapter for RecordingAdapter {
        fn id(&self) -> &'static str {
            "recording"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(Vec::new())
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            Err(DbError::Query("unused".into()))
        }
        async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
            self.seen.lock().expect("lock").push(sql.to_owned());
            let rows = (0..self.available.min(max_rows))
                .map(|i| {
                    let n = i64::try_from(i).expect("fits");
                    Row::new(vec![Value::Integer(n), Value::Text(format!("row{n}"))])
                })
                .collect();
            Ok(QueryResult {
                columns: vec![
                    Column {
                        name: "id".into(),
                        declared_type: Some("INTEGER".into()),
                    },
                    Column {
                        name: "name".into(),
                        declared_type: Some("TEXT".into()),
                    },
                ],
                rows,
                rows_affected: 0,
                ..QueryResult::empty()
            })
        }
    }

    fn users() -> TableInfo {
        TableInfo::unqualified("users")
    }

    fn id_key() -> Vec<String> {
        vec!["id".to_owned()]
    }

    #[tokio::test]
    async fn a_full_page_reports_more_and_offers_the_last_row_as_the_cursor() {
        let adapter = RecordingAdapter::with_rows(500);
        let page = browse_page(&adapter, SqlDialect::Sqlite, &users(), &id_key(), 100, None)
            .await
            .expect("page");

        assert_eq!(page.rows.len(), 100, "the extra probe row is not returned");
        assert!(page.has_more);
        assert_eq!(page.next_cursor, Some(vec![Value::Integer(99)]));
    }

    #[tokio::test]
    async fn the_probe_row_is_asked_for_but_never_shown() {
        let adapter = RecordingAdapter::with_rows(500);
        browse_page(&adapter, SqlDialect::Sqlite, &users(), &id_key(), 100, None)
            .await
            .expect("page");

        // One row past the page tells a full page from a last page without
        // a second round trip, exactly as `run_read_query` does for its
        // truncation flag.
        assert!(adapter.last_sql().ends_with("LIMIT 101"));
    }

    #[tokio::test]
    async fn a_short_page_is_the_last_one() {
        let adapter = RecordingAdapter::with_rows(40);
        let page = browse_page(&adapter, SqlDialect::Sqlite, &users(), &id_key(), 100, None)
            .await
            .expect("page");

        assert_eq!(page.rows.len(), 40);
        assert!(!page.has_more);
        assert_eq!(page.next_cursor, None);
    }

    #[tokio::test]
    async fn an_exactly_full_table_does_not_promise_a_page_that_is_empty() {
        let adapter = RecordingAdapter::with_rows(100);
        let page = browse_page(&adapter, SqlDialect::Sqlite, &users(), &id_key(), 100, None)
            .await
            .expect("page");

        assert_eq!(page.rows.len(), 100);
        assert!(!page.has_more, "the probe row is what distinguishes this");
        assert_eq!(page.next_cursor, None);
    }

    #[tokio::test]
    async fn the_next_page_resumes_strictly_after_the_cursor() {
        let adapter = RecordingAdapter::with_rows(500);
        browse_page(
            &adapter,
            SqlDialect::Sqlite,
            &users(),
            &id_key(),
            100,
            Some(&[Value::Integer(99)]),
        )
        .await
        .expect("page");

        let sql = adapter.last_sql();
        assert!(sql.contains(r#"WHERE ("id") > (99)"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "id""#), "{sql}");
    }

    #[tokio::test]
    async fn a_keyless_table_admits_there_is_more_and_offers_nowhere_to_go() {
        let adapter = RecordingAdapter::with_rows(500);
        let page = browse_page(&adapter, SqlDialect::Sqlite, &users(), &[], 100, None)
            .await
            .expect("page");

        assert_eq!(page.rows.len(), 100);
        assert!(
            page.has_more,
            "the rows exist and saying otherwise would lie"
        );
        assert_eq!(
            page.next_cursor, None,
            "no key means no stable order to resume from"
        );
        assert!(!adapter.last_sql().contains("ORDER BY"));
    }

    #[tokio::test]
    async fn the_page_size_stays_under_the_workspace_row_cap() {
        let adapter = RecordingAdapter::with_rows(1);
        browse_page(
            &adapter,
            SqlDialect::Sqlite,
            &users(),
            &id_key(),
            MAX_RESULT_ROWS * 2,
            None,
        )
        .await
        .expect("page");

        // The probe is one row past the page, so the page itself has to
        // stop one short of the cap for the probe to stay legal.
        assert!(adapter
            .last_sql()
            .ends_with(&format!("LIMIT {MAX_RESULT_ROWS}")));
    }

    #[tokio::test]
    async fn a_cursor_whose_key_column_is_missing_yields_no_next_page() {
        // `SELECT *` cannot drop a key column, but a view or a projection
        // reaching this helper could. Reporting no cursor keeps the caller
        // from looping on the same page forever.
        let adapter = RecordingAdapter::with_rows(500);
        let page = browse_page(
            &adapter,
            SqlDialect::Sqlite,
            &users(),
            &["absent".to_owned()],
            100,
            None,
        )
        .await
        .expect("page");

        assert!(page.has_more);
        assert_eq!(page.next_cursor, None);
    }
}
