//! History that a versioned database keeps, described by what it shows
//! (ADR-0165).
//!
//! Dolt is the first source, but nothing here names it: the types describe
//! a commit log, one row's past values, a diff between two commits and who
//! last changed each row. MariaDB system-versioned tables, and later
//! databases with their own history, fill the same shapes.
//!
//! Everything is read-only. Nothing in this module can commit, check out or
//! merge; that needs its own permission design first (ADR-0087).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::DbResult;
use crate::row::{Column, Row};
use crate::schema::TableInfo;
use crate::value::Value;

/// One commit in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub email: String,
    /// When the commit was made, as the database reports it.
    pub date: String,
    pub message: String,
}

/// A row's value as of one commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowVersion {
    pub commit: Commit,
    pub values: Row,
}

/// One row's past values, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowHistory {
    pub columns: Vec<Column>,
    pub versions: Vec<RowVersion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Removed,
}

/// One row that differs between two commits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowChange {
    pub kind: ChangeKind,
    pub before: Option<Row>,
    pub after: Option<Row>,
    /// Indexes of the columns whose value differs. Every column for an added
    /// or removed row, so a screen can mark cells without knowing the kind.
    pub changed_columns: Vec<usize>,
}

impl RowChange {
    /// Classify a change from the row as it was and as it is.
    ///
    /// Returns `None` when there is nothing to show: both sides missing, or
    /// a "modified" row whose values are all equal.
    #[must_use]
    pub fn between(before: Option<Row>, after: Option<Row>) -> Option<Self> {
        let (kind, changed_columns) = match (&before, &after) {
            (None, None) => return None,
            (None, Some(after)) => (ChangeKind::Added, (0..after.len()).collect()),
            (Some(before), None) => (ChangeKind::Removed, (0..before.len()).collect()),
            (Some(before), Some(after)) => {
                let changed = changed_columns(before, after);
                if changed.is_empty() {
                    return None;
                }
                (ChangeKind::Modified, changed)
            }
        };
        Some(Self {
            kind,
            before,
            after,
            changed_columns,
        })
    }
}

/// The rows of one table that differ between two commits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitDiff {
    pub columns: Vec<Column>,
    pub changes: Vec<RowChange>,
}

/// Who last changed one row, identified by its primary key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlameEntry {
    pub key: Row,
    pub commit: Commit,
}

/// The column indexes whose values differ between two rows of one table.
///
/// Rows of different widths compare as far as both reach, and every column
/// past that counts as changed: a column that exists on one side only has
/// changed by definition.
#[must_use]
pub fn changed_columns(before: &Row, after: &Row) -> Vec<usize> {
    let width = before.len().max(after.len());
    (0..width)
        .filter(|&i| match (before.get(i), after.get(i)) {
            (Some(a), Some(b)) => !same_value(a, b),
            _ => true,
        })
        .collect()
}

/// Cell equality for history purposes.
///
/// `PartialEq` on `f64` says `NaN != NaN`, which would mark an unchanged
/// `NaN` cell as changed in every diff. Floats are compared by their stored
/// bits instead, so a value that was not rewritten never shows as changed,
/// and any two `NaN`s count as the same.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Real(x), Value::Real(y)) => {
            x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan())
        }
        _ => a == b,
    }
}

/// Reads the history a versioned database keeps. Read-only by construction:
/// there is no method here that changes anything.
#[async_trait]
pub trait HistoryReader: Send + Sync {
    /// The most recent commits, newest first, at most `limit` of them.
    async fn commit_log(&self, limit: usize) -> DbResult<Vec<Commit>>;

    /// One row's values at each commit that changed it, newest first.
    /// `key` names the row by its primary-key columns and values.
    async fn row_history(
        &self,
        table: &TableInfo,
        key: &[(String, Value)],
        limit: usize,
    ) -> DbResult<RowHistory>;

    /// The rows of `table` that differ between commit `from` and commit `to`.
    async fn diff(&self, table: &TableInfo, from: &str, to: &str) -> DbResult<CommitDiff>;

    /// Who last changed each row of `table`. Only tables with a primary key
    /// can answer; others return a schema error.
    async fn blame(&self, table: &TableInfo) -> DbResult<Vec<BlameEntry>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: Vec<Value>) -> Row {
        Row::new(values)
    }

    #[test]
    fn identical_rows_have_no_changed_columns() {
        let a = row(vec![Value::Integer(1), Value::Text("a".into())]);
        assert!(changed_columns(&a, &a.clone()).is_empty());
    }

    #[test]
    fn only_the_cells_that_differ_are_reported() {
        let before = row(vec![
            Value::Integer(1),
            Value::Text("a".into()),
            Value::Null,
        ]);
        let after = row(vec![
            Value::Integer(1),
            Value::Text("b".into()),
            Value::Integer(3),
        ]);
        assert_eq!(changed_columns(&before, &after), vec![1, 2]);
    }

    #[test]
    fn a_nan_that_stayed_nan_is_not_a_change() {
        let a = row(vec![Value::Real(f64::NAN)]);
        let b = row(vec![Value::Real(f64::NAN)]);
        assert!(changed_columns(&a, &b).is_empty());
    }

    #[test]
    fn columns_present_on_one_side_only_count_as_changed() {
        let before = row(vec![Value::Integer(1)]);
        let after = row(vec![Value::Integer(1), Value::Integer(2)]);
        assert_eq!(changed_columns(&before, &after), vec![1]);
    }

    #[test]
    fn an_added_row_marks_every_cell() {
        let after = row(vec![Value::Integer(1), Value::Integer(2)]);
        let change = RowChange::between(None, Some(after)).expect("a change");
        assert_eq!(change.kind, ChangeKind::Added);
        assert_eq!(change.changed_columns, vec![0, 1]);
    }

    #[test]
    fn a_removed_row_marks_every_cell() {
        let before = row(vec![Value::Integer(1), Value::Integer(2)]);
        let change = RowChange::between(Some(before), None).expect("a change");
        assert_eq!(change.kind, ChangeKind::Removed);
        assert_eq!(change.changed_columns, vec![0, 1]);
    }

    #[test]
    fn a_modified_row_marks_only_what_changed() {
        let before = row(vec![Value::Integer(1), Value::Integer(2)]);
        let after = row(vec![Value::Integer(1), Value::Integer(9)]);
        let change = RowChange::between(Some(before), Some(after)).expect("a change");
        assert_eq!(change.kind, ChangeKind::Modified);
        assert_eq!(change.changed_columns, vec![1]);
    }

    #[test]
    fn an_unchanged_or_empty_pair_is_not_a_change() {
        let same = row(vec![Value::Integer(1)]);
        assert!(RowChange::between(Some(same.clone()), Some(same)).is_none());
        assert!(RowChange::between(None, None).is_none());
    }

    #[test]
    fn change_kinds_travel_as_snake_case() {
        let json = serde_json::to_string(&ChangeKind::Modified).expect("serialises");
        assert_eq!(json, "\"modified\"");
    }
}
