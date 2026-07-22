//! Multi-row `INSERT` assembly for a logical dump (ADR-0049, slice a).
//!
//! Given a table, its column order, and a page of rows, this builds one
//! terminated `INSERT INTO … VALUES (…), (…);` statement. The caller (the
//! later orchestrator slice) decides how many rows go in a batch — small
//! enough to stay under `MAX_RESULT_ROWS` and SQLite's compound-statement
//! limit — so this function just renders whatever page it is handed.

use crate::dump::value_literal;
use crate::row::Row;
use crate::schema::TableInfo;
use crate::value::Value;
use crate::write_back::{qualified_table, quote_ident, SqlDialect};

/// Build one multi-row `INSERT` for `rows` into `table`.
///
/// `columns` is the column order the values are keyed on (index `i` in each
/// row maps to `columns[i]`). Returns `None` when there is nothing to emit —
/// no rows or no columns — so a caller can skip empty tables without emitting
/// a syntactically invalid `INSERT`. A row shorter than `columns` pads the
/// missing trailing cells with `NULL` rather than panicking.
#[must_use]
pub fn build_insert(
    table: &TableInfo,
    columns: &[String],
    rows: &[Row],
    dialect: SqlDialect,
) -> Option<String> {
    if rows.is_empty() || columns.is_empty() {
        return None;
    }

    let table_sql = qualified_table(table, dialect);
    let column_list = columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");

    let tuples = rows
        .iter()
        .map(|row| {
            let cells = (0..columns.len())
                .map(|i| value_literal(row.get(i).unwrap_or(&Value::Null), dialect))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({cells})")
        })
        .collect::<Vec<_>>()
        .join(", ");

    Some(format!(
        "INSERT INTO {table_sql} ({column_list}) VALUES {tuples};"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn single_row_insert_quotes_columns_and_renders_values() {
        let sql = build_insert(
            &TableInfo::unqualified("widgets"),
            &cols(&["id", "name"]),
            &[Row::new(vec![
                Value::Integer(7),
                Value::Text("gadget".into()),
            ])],
            SqlDialect::Sqlite,
        )
        .unwrap();
        assert_eq!(
            sql,
            r#"INSERT INTO "widgets" ("id", "name") VALUES (7, 'gadget');"#
        );
    }

    #[test]
    fn multiple_rows_batch_into_one_statement() {
        let sql = build_insert(
            &TableInfo::unqualified("t"),
            &cols(&["a", "b"]),
            &[
                Row::new(vec![Value::Integer(1), Value::Text("x".into())]),
                Row::new(vec![Value::Integer(2), Value::Text("y".into())]),
            ],
            SqlDialect::Sqlite,
        )
        .unwrap();
        assert_eq!(
            sql,
            r#"INSERT INTO "t" ("a", "b") VALUES (1, 'x'), (2, 'y');"#
        );
    }

    #[test]
    fn postgres_qualifies_the_table_with_its_schema() {
        let sql = build_insert(
            &TableInfo::qualified("public", "users"),
            &cols(&["id"]),
            &[Row::new(vec![Value::Integer(1)])],
            SqlDialect::Postgres,
        )
        .unwrap();
        assert_eq!(sql, r#"INSERT INTO "public"."users" ("id") VALUES (1);"#);
    }

    #[test]
    fn sqlite_ignores_a_schema_that_slipped_into_table_info() {
        let sql = build_insert(
            &TableInfo::qualified("main", "t"),
            &cols(&["a"]),
            &[Row::new(vec![Value::Text("v".into())])],
            SqlDialect::Sqlite,
        )
        .unwrap();
        assert_eq!(sql, r#"INSERT INTO "t" ("a") VALUES ('v');"#);
    }

    #[test]
    fn null_and_blob_values_render_per_dialect() {
        let sql = build_insert(
            &TableInfo::unqualified("t"),
            &cols(&["a", "b"]),
            &[Row::new(vec![Value::Null, Value::Blob(vec![0x01, 0x02])])],
            SqlDialect::Sqlite,
        )
        .unwrap();
        assert_eq!(sql, r#"INSERT INTO "t" ("a", "b") VALUES (NULL, X'0102');"#);
    }

    #[test]
    fn no_rows_emits_nothing() {
        assert_eq!(
            build_insert(
                &TableInfo::unqualified("t"),
                &cols(&["a"]),
                &[],
                SqlDialect::Sqlite
            ),
            None
        );
    }

    #[test]
    fn no_columns_emits_nothing() {
        assert_eq!(
            build_insert(
                &TableInfo::unqualified("t"),
                &[],
                &[Row::new(vec![Value::Integer(1)])],
                SqlDialect::Sqlite,
            ),
            None
        );
    }

    #[test]
    fn a_short_row_pads_missing_trailing_cells_with_null() {
        // Defensive: a row with fewer values than declared columns must not
        // panic — the tail is treated as NULL.
        let sql = build_insert(
            &TableInfo::unqualified("t"),
            &cols(&["a", "b", "c"]),
            &[Row::new(vec![Value::Integer(1)])],
            SqlDialect::Sqlite,
        )
        .unwrap();
        assert_eq!(
            sql,
            r#"INSERT INTO "t" ("a", "b", "c") VALUES (1, NULL, NULL);"#
        );
    }

    #[test]
    fn identifiers_with_double_quotes_are_escaped() {
        let sql = build_insert(
            &TableInfo::unqualified(r#"we"ird"#),
            &cols(&[r#"c"l"#]),
            &[Row::new(vec![Value::Integer(1)])],
            SqlDialect::Sqlite,
        )
        .unwrap();
        assert_eq!(sql, r#"INSERT INTO "we""ird" ("c""l") VALUES (1);"#);
    }
}
