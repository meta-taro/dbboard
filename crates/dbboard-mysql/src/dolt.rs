//! Dolt's history, read through its system tables (ADR-0165).
//!
//! Dolt speaks the `MySQL` wire protocol, so it is a `MySQL` connection that
//! answers one more question. This module asks it (`dolt_version()`) and,
//! when the answer is yes, reads `dolt_log`, `dolt_history_<table>`,
//! `dolt_commit_diff_<table>` and `dolt_blame_<table>`.
//!
//! Every statement here is a `SELECT`. Nothing commits, checks out or merges.
//!
//! Columns are found by name, never by position: the column sets of these
//! tables have grown between Dolt releases (`dolt_log` gained `author` and
//! friends in 2.x), and a positional reader would misread an older or newer
//! server without failing.

use std::sync::atomic::AtomicU8;
use std::sync::Arc;

use async_trait::async_trait;
use dbboard_core::{
    value_literal, BlameEntry, Column, Commit, CommitDiff, DbError, DbResult, HistoryReader,
    QueryResult, ReadOnlyStatement, Row, RowChange, RowHistory, RowVersion, SqlDialect, TableInfo,
    Value,
};
use sqlx::MySqlPool;

use super::{quote_ident, run_read_only_txn};

/// Succeeds on Dolt and fails on every other `MySQL`-compatible server.
pub(crate) const DETECT_SQL: &str = "SELECT dolt_version()";

/// The text protocol is the only one that returns printed values, which is
/// what the adapter's cell decoder reads, so values are written into the
/// statement as literals rather than bound. `value_literal` is the same total,
/// escaping literal writer the dump path uses.
fn literal(value: &Value) -> String {
    value_literal(value, SqlDialect::MySql)
}

fn text_literal(text: &str) -> String {
    literal(&Value::Text(text.to_string()))
}

/// Only the columns every Dolt release has; see the module docs.
pub(crate) fn commit_log_sql(limit: usize) -> String {
    format!(
        "SELECT commit_hash, committer, email, date, message FROM dolt_log \
         ORDER BY date DESC LIMIT {limit}"
    )
}

/// The columns `row_history_sql` appends after the table's own columns.
const HISTORY_TAIL: [&str; 5] = [
    "commit_hash",
    "committer",
    "commit_date",
    "email",
    "message",
];

/// `` `schema`.`dolt_<kind>_<table>` ``, or unqualified when the table has no
/// schema, in which case Dolt reads the connection's current database.
fn system_table(table: &TableInfo, kind: &str) -> String {
    let name = quote_ident(&format!("dolt_{kind}_{}", table.name));
    match &table.schema {
        Some(schema) => format!("{}.{name}", quote_ident(schema)),
        None => name,
    }
}

fn log_table(table: &TableInfo) -> String {
    match &table.schema {
        Some(schema) => format!("{}.`dolt_log`", quote_ident(schema)),
        None => "`dolt_log`".to_string(),
    }
}

/// One row's history, joined to the log for the e-mail and message that
/// `dolt_history_<table>` does not carry.
///
/// # Errors
///
/// A key with no columns would select every row's history at once, so it is
/// refused rather than run.
pub(crate) fn row_history_sql(
    table: &TableInfo,
    key: &[(String, Value)],
    limit: usize,
) -> DbResult<String> {
    if key.is_empty() {
        return Err(DbError::Query(
            "a row's history needs its primary-key values".to_string(),
        ));
    }
    let filter = key
        .iter()
        .map(|(column, value)| format!("h.{} = {}", quote_ident(column), literal(value)))
        .collect::<Vec<_>>()
        .join(" AND ");
    Ok(format!(
        "SELECT h.*, l.email, l.message FROM {} h JOIN {} l ON l.commit_hash = h.commit_hash \
         WHERE {filter} ORDER BY h.commit_date DESC LIMIT {limit}",
        system_table(table, "history"),
        log_table(table),
    ))
}

/// The rows that differ between two commits. `HASHOF` resolves a branch,
/// `HEAD~1` or a hash alike, and fails on a ref that does not exist.
pub(crate) fn commit_diff_sql(table: &TableInfo, from: &str, to: &str) -> String {
    format!(
        "SELECT * FROM {} WHERE from_commit = HASHOF({}) AND to_commit = HASHOF({})",
        system_table(table, "commit_diff"),
        text_literal(from),
        text_literal(to),
    )
}

pub(crate) fn blame_sql(table: &TableInfo) -> String {
    format!("SELECT * FROM {}", system_table(table, "blame"))
}

fn index_of(result: &QueryResult, name: &str) -> DbResult<usize> {
    result
        .columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| DbError::Schema(format!("Dolt returned no `{name}` column")))
}

fn text(row: &Row, index: usize) -> String {
    match row.get(index) {
        Some(Value::Text(t)) => t.clone(),
        Some(Value::Integer(i)) => i.to_string(),
        Some(Value::Real(r)) => r.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn parse_commit_log(result: &QueryResult) -> DbResult<Vec<Commit>> {
    let [hash, author, email, date, message] = [
        index_of(result, "commit_hash")?,
        index_of(result, "committer")?,
        index_of(result, "email")?,
        index_of(result, "date")?,
        index_of(result, "message")?,
    ];
    Ok(result
        .rows
        .iter()
        .map(|row| Commit {
            hash: text(row, hash),
            author: text(row, author),
            email: text(row, email),
            date: text(row, date),
            message: text(row, message),
        })
        .collect())
}

/// Turn `row_history_sql` output into versions, newest first.
///
/// `dolt_history_<table>` lists the row at *every* commit, including commits
/// that did not touch it. A version is kept only when it differs from the one
/// before it, so the list answers "when did this row change?". The oldest
/// version is always kept: it is when the row first appears.
pub(crate) fn parse_row_history(result: &QueryResult) -> DbResult<RowHistory> {
    let tail: Vec<usize> = HISTORY_TAIL
        .iter()
        .map(|name| index_of(result, name))
        .collect::<DbResult<_>>()?;
    let width = tail.iter().copied().min().unwrap_or(0);
    let columns: Vec<Column> = result.columns[..width].to_vec();

    let all: Vec<RowVersion> = result
        .rows
        .iter()
        .map(|row| RowVersion {
            commit: Commit {
                hash: text(row, tail[0]),
                author: text(row, tail[1]),
                date: text(row, tail[2]),
                email: text(row, tail[3]),
                message: text(row, tail[4]),
            },
            values: Row::new(row.values()[..width].to_vec()),
        })
        .collect();

    let versions = all
        .iter()
        .enumerate()
        .filter(|(i, version)| match all.get(i + 1) {
            Some(older) => {
                RowChange::between(Some(older.values.clone()), Some(version.values.clone()))
                    .is_some()
            }
            None => true,
        })
        .map(|(_, version)| version.clone())
        .collect();

    Ok(RowHistory { columns, versions })
}

/// Turn `commit_diff_sql` output into row changes.
///
/// The columns arrive as `to_<col>…, to_commit, to_commit_date,
/// from_<col>…, from_commit, from_commit_date, diff_type`.
pub(crate) fn parse_commit_diff(result: &QueryResult) -> DbResult<CommitDiff> {
    let to_end = index_of(result, "to_commit")?;
    let from_start = index_of(result, "to_commit_date")? + 1;
    let from_end = index_of(result, "from_commit")?;
    let kind = index_of(result, "diff_type")?;
    if from_end - from_start != to_end {
        return Err(DbError::Schema(
            "Dolt's diff has different column sets on each side".to_string(),
        ));
    }

    let columns = result.columns[..to_end]
        .iter()
        .map(|c| Column {
            name: c.name.strip_prefix("to_").unwrap_or(&c.name).to_string(),
            declared_type: c.declared_type.clone(),
        })
        .collect();

    let changes = result
        .rows
        .iter()
        .filter_map(|row| {
            let after = Row::new(row.values()[..to_end].to_vec());
            let before = Row::new(row.values()[from_start..from_end].to_vec());
            match text(row, kind).as_str() {
                "added" => RowChange::between(None, Some(after)),
                "removed" => RowChange::between(Some(before), None),
                _ => RowChange::between(Some(before), Some(after)),
            }
        })
        .collect();

    Ok(CommitDiff { columns, changes })
}

/// Turn `blame_sql` output into one entry per row: the primary-key columns
/// come first, then `commit, commit_date, committer, email, message`.
pub(crate) fn parse_blame(result: &QueryResult) -> DbResult<Vec<BlameEntry>> {
    let key_end = index_of(result, "commit")?;
    let [hash, date, author, email, message] = [
        key_end,
        index_of(result, "commit_date")?,
        index_of(result, "committer")?,
        index_of(result, "email")?,
        index_of(result, "message")?,
    ];
    Ok(result
        .rows
        .iter()
        .map(|row| BlameEntry {
            key: Row::new(row.values()[..key_end].to_vec()),
            commit: Commit {
                hash: text(row, hash),
                author: text(row, author),
                email: text(row, email),
                date: text(row, date),
                message: text(row, message),
            },
        })
        .collect())
}

/// The [`HistoryReader`] for a connection that answered [`DETECT_SQL`].
///
/// Every read goes through the adapter's read-only transaction, the same path
/// an agent's query takes, so a history read cannot write even if one of
/// these statements were wrong.
pub(crate) struct DoltHistory {
    pub(crate) pool: MySqlPool,
    pub(crate) timeout_style: Arc<AtomicU8>,
}

impl DoltHistory {
    async fn fetch(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        run_read_only_txn(
            self.pool.clone(),
            Arc::clone(&self.timeout_style),
            sql,
            max_rows,
            ReadOnlyStatement::Query,
        )
        .await
    }
}

/// Diffs and blame are bounded the same way every result set is.
const MAX_HISTORY_ROWS: usize = dbboard_core::MAX_RESULT_ROWS;

#[async_trait]
impl HistoryReader for DoltHistory {
    async fn commit_log(&self, limit: usize) -> DbResult<Vec<Commit>> {
        let limit = limit.min(MAX_HISTORY_ROWS);
        let result = self.fetch(&commit_log_sql(limit), limit).await?;
        parse_commit_log(&result)
    }

    async fn row_history(
        &self,
        table: &TableInfo,
        key: &[(String, Value)],
        limit: usize,
    ) -> DbResult<RowHistory> {
        let limit = limit.min(MAX_HISTORY_ROWS);
        let result = self
            .fetch(&row_history_sql(table, key, limit)?, limit)
            .await?;
        parse_row_history(&result)
    }

    async fn diff(&self, table: &TableInfo, from: &str, to: &str) -> DbResult<CommitDiff> {
        let result = self
            .fetch(&commit_diff_sql(table, from, to), MAX_HISTORY_ROWS)
            .await?;
        parse_commit_diff(&result)
    }

    async fn blame(&self, table: &TableInfo) -> DbResult<Vec<BlameEntry>> {
        let result = self.fetch(&blame_sql(table), MAX_HISTORY_ROWS).await?;
        parse_blame(&result)
    }
}

#[cfg(test)]
mod tests {
    //! The fixtures below are what Dolt 2.6.2 returned for a fictional
    //! `items` table across three commits, reduced to text the way
    //! [`super::super::decode_cell`] reduces every cell.

    use dbboard_core::ChangeKind;

    use super::*;

    fn result(columns: &[&str], rows: &[&[Option<&str>]]) -> QueryResult {
        QueryResult {
            columns: columns
                .iter()
                .map(|name| Column {
                    name: (*name).to_string(),
                    declared_type: None,
                })
                .collect(),
            rows: rows
                .iter()
                .map(|cells| {
                    Row::new(
                        cells
                            .iter()
                            .map(|c| c.map_or(Value::Null, |t| Value::Text(t.to_string())))
                            .collect(),
                    )
                })
                .collect(),
            ..QueryResult::empty()
        }
    }

    fn items() -> TableInfo {
        TableInfo {
            schema: Some("shop".to_string()),
            name: "items".to_string(),
        }
    }

    const HEAD: &str = "l0mvhl7v0kcku6bvtc4c1hp6opsvukf3";
    const SECOND: &str = "5n4cb54ou67ieg2c3pesc8sgpvced48s";
    const FIRST: &str = "o1k9q50b2sffd34e4hai7lrm589phlfe";

    #[test]
    fn system_tables_are_quoted_and_qualified() {
        assert_eq!(
            row_history_sql(&items(), &[("id".to_string(), Value::Integer(1))], 50)
                .expect("builds"),
            "SELECT h.*, l.email, l.message FROM `shop`.`dolt_history_items` h \
             JOIN `shop`.`dolt_log` l ON l.commit_hash = h.commit_hash \
             WHERE h.`id` = 1 ORDER BY h.commit_date DESC LIMIT 50"
        );
        assert_eq!(
            commit_diff_sql(&items(), FIRST, "HEAD"),
            format!(
                "SELECT * FROM `shop`.`dolt_commit_diff_items` \
                 WHERE from_commit = HASHOF('{FIRST}') AND to_commit = HASHOF('HEAD')"
            )
        );
        assert_eq!(
            commit_log_sql(20),
            "SELECT commit_hash, committer, email, date, message FROM dolt_log \
             ORDER BY date DESC LIMIT 20"
        );
        assert_eq!(
            blame_sql(&items()),
            "SELECT * FROM `shop`.`dolt_blame_items`"
        );
    }

    #[test]
    fn a_back_tick_in_a_table_name_cannot_close_the_identifier() {
        let table = TableInfo {
            schema: None,
            name: "x`; DROP TABLE y; --".to_string(),
        };
        assert_eq!(
            blame_sql(&table),
            "SELECT * FROM `dolt_blame_x``; DROP TABLE y; --`"
        );
    }

    #[test]
    fn a_ref_with_a_quote_cannot_close_the_literal() {
        let sql = commit_diff_sql(&items(), "main') OR ('1'='1", "HEAD");
        assert!(sql.contains("HASHOF('main'') OR (''1''=''1')"), "{sql}");
    }

    #[test]
    fn a_text_key_is_escaped_and_an_empty_key_is_refused() {
        let key = [("name".to_string(), Value::Text(r"a\b'c".to_string()))];
        let sql = row_history_sql(&items(), &key, 5).expect("builds");
        assert!(sql.contains(r"h.`name` = 'a\\b''c'"), "{sql}");
        assert!(matches!(
            row_history_sql(&items(), &[], 5),
            Err(DbError::Query(_))
        ));
    }

    #[test]
    fn the_commit_log_is_read_by_column_name() {
        let log = result(
            &["commit_hash", "committer", "email", "date", "message"],
            &[&[
                Some(HEAD),
                Some("Demo Writer"),
                Some("writer@example.invalid"),
                Some("2026-11-02 09:06:06.426"),
                Some("Drop the pot, reprice the mug"),
            ]],
        );
        let commits = parse_commit_log(&log).expect("parses");
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, HEAD);
        assert_eq!(commits[0].author, "Demo Writer");
        assert_eq!(commits[0].message, "Drop the pot, reprice the mug");
    }

    #[test]
    fn a_log_without_a_required_column_is_a_schema_error() {
        let log = result(&["commit_hash", "committer"], &[]);
        assert!(matches!(parse_commit_log(&log), Err(DbError::Schema(_))));
    }

    fn history_columns() -> Vec<&'static str> {
        vec![
            "id",
            "name",
            "qty",
            "price",
            "commit_hash",
            "committer",
            "commit_date",
            "email",
            "message",
        ]
    }

    #[test]
    fn row_history_keeps_only_the_commits_that_changed_the_row() {
        let rows: &[&[Option<&str>]] = &[
            &[
                Some("1"),
                Some("Ceramic mug"),
                Some("2"),
                Some("13"),
                Some(HEAD),
                Some("Demo Writer"),
                Some("2026-11-02 09:06:06.426"),
                Some("writer@example.invalid"),
                Some("Drop the pot, reprice the mug"),
            ],
            &[
                Some("1"),
                Some("Ceramic mug"),
                Some("2"),
                Some("12.5"),
                Some(SECOND),
                Some("Demo Writer"),
                Some("2026-11-02 09:06:06.192"),
                Some("writer@example.invalid"),
                Some("Sell a mug, add a scarf"),
            ],
            // A commit that did not touch the row: same values as the next one.
            &[
                Some("1"),
                Some("Ceramic mug"),
                Some("3"),
                Some("12.5"),
                Some("unrelated000000000000000000000000"),
                Some("Demo Writer"),
                Some("2026-11-02 09:06:06.000"),
                Some("writer@example.invalid"),
                Some("Unrelated"),
            ],
            &[
                Some("1"),
                Some("Ceramic mug"),
                Some("3"),
                Some("12.5"),
                Some(FIRST),
                Some("Demo Writer"),
                Some("2026-11-02 09:06:05.957"),
                Some("writer@example.invalid"),
                Some("Seed two items"),
            ],
        ];
        let history = parse_row_history(&result(&history_columns(), rows)).expect("parses");
        let names: Vec<&str> = history.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "name", "qty", "price"]);
        let hashes: Vec<&str> = history
            .versions
            .iter()
            .map(|v| v.commit.hash.as_str())
            .collect();
        assert_eq!(hashes, [HEAD, SECOND, FIRST]);
        assert_eq!(history.versions[0].commit.email, "writer@example.invalid");
        assert_eq!(history.versions[2].values.len(), 4);
    }

    #[test]
    fn the_diff_marks_added_removed_and_changed_cells() {
        let columns = [
            "to_id",
            "to_name",
            "to_qty",
            "to_price",
            "to_commit",
            "to_commit_date",
            "from_id",
            "from_name",
            "from_qty",
            "from_price",
            "from_commit",
            "from_commit_date",
            "diff_type",
        ];
        let rows: &[&[Option<&str>]] = &[
            &[
                Some("1"),
                Some("Ceramic mug"),
                Some("2"),
                Some("13"),
                Some(HEAD),
                Some("2026-11-02 09:06:06.426000"),
                Some("1"),
                Some("Ceramic mug"),
                Some("3"),
                Some("12.5"),
                Some(FIRST),
                Some("2026-11-02 09:06:05.957000"),
                Some("modified"),
            ],
            &[
                None,
                None,
                None,
                None,
                Some(HEAD),
                Some("2026-11-02 09:06:06.426000"),
                Some("2"),
                Some("Plant pot"),
                Some("5"),
                Some("8"),
                Some(FIRST),
                Some("2026-11-02 09:06:05.957000"),
                Some("removed"),
            ],
            &[
                Some("3"),
                Some("Wool scarf"),
                Some("1"),
                Some("30"),
                Some(HEAD),
                Some("2026-11-02 09:06:06.426000"),
                None,
                None,
                None,
                None,
                Some(FIRST),
                Some("2026-11-02 09:06:05.957000"),
                Some("added"),
            ],
        ];
        let diff = parse_commit_diff(&result(&columns, rows)).expect("parses");
        let names: Vec<&str> = diff.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "name", "qty", "price"]);

        assert_eq!(diff.changes.len(), 3);
        assert_eq!(diff.changes[0].kind, ChangeKind::Modified);
        assert_eq!(diff.changes[0].changed_columns, vec![2, 3]);
        assert_eq!(diff.changes[1].kind, ChangeKind::Removed);
        assert!(diff.changes[1].after.is_none());
        assert_eq!(diff.changes[2].kind, ChangeKind::Added);
        assert!(diff.changes[2].before.is_none());
    }

    #[test]
    fn blame_splits_the_key_from_the_commit() {
        let columns = [
            "id",
            "commit",
            "commit_date",
            "committer",
            "email",
            "message",
        ];
        let rows: &[&[Option<&str>]] = &[
            &[
                Some("1"),
                Some(HEAD),
                Some("2026-11-02 09:06:06.426000"),
                Some("Demo Writer"),
                Some("writer@example.invalid"),
                Some("Drop the pot, reprice the mug"),
            ],
            &[
                Some("3"),
                Some(SECOND),
                Some("2026-11-02 09:06:06.192000"),
                Some("Demo Writer"),
                Some("writer@example.invalid"),
                Some("Sell a mug, add a scarf"),
            ],
        ];
        let blame = parse_blame(&result(&columns, rows)).expect("parses");
        assert_eq!(blame.len(), 2);
        assert_eq!(blame[0].key, Row::new(vec![Value::Text("1".into())]));
        assert_eq!(blame[1].commit.hash, SECOND);
        assert_eq!(blame[1].commit.message, "Sell a mug, add a scarf");
    }
}
