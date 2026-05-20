//! Row and column types returned by adapters from a SELECT query.

use crate::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The type the driver reported for this column. `None` when the
    /// driver cannot determine a type (e.g. expressions in SQLite).
    pub declared_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// Number of rows the statement affected. Populated for DML/DDL
    /// (`INSERT`, `UPDATE`, `DELETE`, `CREATE`, ...). For row-returning
    /// statements (`SELECT`, `WITH`, ...) the adapter leaves this at 0
    /// and exposes the rows via [`Self::rows`] instead.
    pub rows_affected: u64,
}

impl QueryResult {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 0,
        }
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
        };
        assert_eq!(result.columns[0].name, "id");
        assert_eq!(result.rows[0].get(0), Some(&Value::Integer(1)));
    }

    #[test]
    fn query_result_records_affected_count_for_dml() {
        let result = QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: 3,
        };
        assert_eq!(result.rows_affected, 3);
    }
}
