//! Row and column types returned by adapters from a SELECT query.

use serde::{Deserialize, Serialize};

use crate::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// The type the driver reported for this column. `None` when the
    /// driver cannot determine a type (e.g. expressions in SQLite).
    pub declared_type: Option<String>,
}

/// Serialized as a bare JSON array of values (`[v1, v2, ...]`) via
/// `#[serde(transparent)]`, matching the API contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Row {
    values: Vec<Value>,
}

impl Row {
    #[must_use]
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    #[must_use]
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// Number of rows the statement affected. Populated for DML/DDL
    /// (`INSERT`, `UPDATE`, `DELETE`, `CREATE`, ...). For row-returning
    /// statements (`SELECT`, `WITH`, ...) the adapter leaves this at 0
    /// and exposes the rows via [`Self::rows`] instead.
    pub rows_affected: u64,
    /// Whether rows exist beyond this page (ADR-0145).
    ///
    /// Set by the paging use-case, never by an adapter: an adapter answers
    /// the statement it was given and does not know it was one page of
    /// several. A result that is not a page leaves this `false`, which is
    /// also what its absence from the JSON means.
    #[serde(default, skip_serializing_if = "is_not_more")]
    pub has_more: bool,
    /// The key values of this page's last row, in primary-key order, to be
    /// passed back as the next page's `after` (ADR-0145).
    ///
    /// `None` when there is no next page *or* no stable cursor: a table
    /// with no primary key can report [`Self::has_more`] truthfully and
    /// still have nowhere to go, which is the honest form of "first page
    /// only" rather than a silent `OFFSET`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Vec<Value>>,
}

/// `skip_serializing_if` for [`QueryResult::has_more`]: keeps a non-paged
/// result's JSON byte-identical to what it was before paging existed.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_not_more(has_more: &bool) -> bool {
    !*has_more
}

impl QueryResult {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 0,
            has_more: false,
            next_cursor: None,
        }
    }

    /// Drop any rows beyond `max_rows`, keeping the columns intact.
    ///
    /// The read-only query path (ADR-0046) caps an agent's result set by
    /// *truncating* to a soft bound rather than erroring like the
    /// workspace-wide [`MAX_RESULT_ROWS`](crate::MAX_RESULT_ROWS): a
    /// broad `SELECT *` returns its first `max_rows` rows plus a
    /// `truncated` signal, instead of failing outright.
    pub fn truncate_rows(&mut self, max_rows: usize) {
        self.rows.truncate(max_rows);
    }
}

#[cfg(test)]
mod tests {
    use super::{Column, QueryResult, Row};
    use crate::Value;

    #[test]
    fn row_get_returns_value_at_index() {
        let row = Row::new(vec![Value::Integer(1), Value::Text("a".into())]);
        assert_eq!(row.get(0), Some(&Value::Integer(1)));
        assert_eq!(row.get(1), Some(&Value::Text("a".into())));
        assert_eq!(row.get(2), None);
    }

    #[test]
    fn row_len_reflects_value_count() {
        assert_eq!(Row::new(vec![]).len(), 0);
        assert!(Row::new(vec![]).is_empty());
        assert_eq!(Row::new(vec![Value::Null, Value::Null]).len(), 2);
    }

    #[test]
    fn empty_query_result_has_no_columns_rows_or_affected_count() {
        let result = QueryResult::empty();
        assert!(result.columns.is_empty());
        assert!(result.rows.is_empty());
        assert_eq!(result.rows_affected, 0);
    }

    #[test]
    fn query_result_carries_columns_and_rows() {
        let result = QueryResult {
            columns: vec![Column {
                name: "id".into(),
                declared_type: Some("INTEGER".into()),
            }],
            rows: vec![Row::new(vec![Value::Integer(1)])],
            rows_affected: 0,
            ..QueryResult::empty()
        };
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.rows[0].get(0), Some(&Value::Integer(1)));
    }

    // --- Paging (ADR-0145) ------------------------------------------
    //
    // The two fields are additive to the shape in `docs/api-contract.md`,
    // which freezes at v1.0. A result that is not a page must serialise
    // exactly as it did before they existed, and a payload written before
    // they existed must still parse.

    #[test]
    fn a_result_that_is_not_a_page_serialises_without_the_paging_keys() {
        let json = serde_json::to_value(QueryResult::empty()).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(!object.contains_key("has_more"));
        assert!(!object.contains_key("next_cursor"));
    }

    #[test]
    fn a_payload_written_before_paging_existed_still_parses() {
        let json = r#"{"columns":[],"rows":[],"rows_affected":3}"#;
        let result: QueryResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.rows_affected, 3);
        assert!(!result.has_more);
        assert_eq!(result.next_cursor, None);
    }

    #[test]
    fn a_page_carries_its_cursor_through_json() {
        let page = QueryResult {
            has_more: true,
            next_cursor: Some(vec![Value::Integer(42), Value::Text("b".into())]),
            ..QueryResult::empty()
        };
        let round_tripped: QueryResult =
            serde_json::from_str(&serde_json::to_string(&page).expect("serialise")).expect("parse");
        assert_eq!(round_tripped, page);
    }

    #[test]
    fn the_last_page_says_so_without_offering_a_cursor() {
        let last = QueryResult {
            has_more: false,
            next_cursor: None,
            ..QueryResult::empty()
        };
        let json = serde_json::to_value(&last).expect("serialise");
        // `has_more: false` is the absence of a next page, which is what
        // every non-paged result already says by saying nothing.
        assert!(!json.as_object().expect("object").contains_key("has_more"));
    }

    #[test]
    fn truncate_rows_keeps_only_the_first_max_rows() {
        let mut result = QueryResult {
            columns: vec![Column {
                name: "id".into(),
                declared_type: None,
            }],
            rows: (0..5).map(|i| Row::new(vec![Value::Integer(i)])).collect(),
            rows_affected: 0,
            ..QueryResult::empty()
        };
        result.truncate_rows(2);
        assert_eq!(result.rows.len(), 2);
        // Columns are untouched by the row cap.
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.rows[0].get(0), Some(&Value::Integer(0)));
        assert_eq!(result.rows[1].get(0), Some(&Value::Integer(1)));
    }

    #[test]
    fn truncate_rows_is_a_noop_when_under_the_cap() {
        let mut result = QueryResult {
            columns: Vec::new(),
            rows: vec![Row::new(vec![Value::Integer(1)])],
            rows_affected: 0,
            ..QueryResult::empty()
        };
        result.truncate_rows(10);
        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn query_result_records_affected_count_for_dml() {
        let result = QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 3,
            ..QueryResult::empty()
        };
        assert_eq!(result.rows_affected, 3);
    }

    #[test]
    fn row_serializes_as_a_bare_array() {
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("a".into()),
            Value::Null,
        ]);
        assert_eq!(serde_json::to_string(&row).unwrap(), r#"[1,"a",null]"#);
    }

    #[test]
    fn query_result_round_trips_through_json() {
        let result = QueryResult {
            columns: vec![
                Column {
                    name: "id".into(),
                    declared_type: Some("INTEGER".into()),
                },
                Column {
                    name: "expr".into(),
                    declared_type: None,
                },
            ],
            rows: vec![Row::new(vec![Value::Integer(1), Value::Null])],
            rows_affected: 0,
            ..QueryResult::empty()
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: QueryResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }
}
