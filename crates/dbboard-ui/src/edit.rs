//! Pure planning for inline cell editing (issue 0013 slice b).
//!
//! The egui state machine in [`crate`]'s `lib.rs` owns the mutable grid
//! state (which cell is open, which cells are staged, an in-flight save
//! queue) and drives it frame by frame. Everything that decides *what SQL
//! to run* lives here instead, with no egui in sight, so it can be unit
//! tested exhaustively — CLAUDE.md forbids business logic in UI handlers.
//!
//! This layer sits on top of the pure `dbboard-core::write_back` core
//! (ADR-0042 slice a): it maps staged grid cells back to column names and
//! the row's original primary-key values, then defers the actual escaping
//! and `UPDATE` string to [`dbboard_core::build_update_sql`].

use std::collections::BTreeMap;

use dbboard_core::{
    build_update_sql, CellValue, QueryResult, RowIdentity, RowKey, SqlDialect, TableInfo,
    TableSchema, UpdatePlan, WriteBackError,
};

/// A staged (仮登録) new value for one cell: text typed into the editor,
/// or an explicit SQL `NULL`. Mirrors core [`CellValue`] but lives here
/// because it is view-model state the grid mutates directly (an empty
/// string is a real empty string, never a stand-in for null).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedValue {
    /// Explicit SQL `NULL`.
    Null,
    /// Text from the editor.
    Text(String),
}

impl StagedValue {
    fn to_cell(&self) -> CellValue {
        match self {
            StagedValue::Null => CellValue::Null,
            StagedValue::Text(s) => CellValue::Text(s.clone()),
        }
    }
}

/// Map an adapter id ([`dbboard_core::DatabaseAdapter::id`]) to its SQL
/// dialect family. Unknown ids yield `None` — editing is then disabled
/// rather than guessing a dialect and building the wrong SQL.
#[must_use]
pub fn dialect_for_adapter_id(id: &str) -> Option<SqlDialect> {
    match id {
        "turso" | "d1" => Some(SqlDialect::Sqlite),
        "postgres" | "neon" | "supabase" | "aurora-dsql" => Some(SqlDialect::Postgres),
        _ => None,
    }
}

/// Whether a result backed by `schema`/`dialect` can be inline-edited in
/// the UI.
///
/// The UI is deliberately stricter than the core: it requires a **declared
/// primary key**. A browse `SELECT *` returns the primary-key columns (so
/// their identity values sit in the grid) but never the implicit SQLite
/// `rowid`, so rowid-only tables stay read-only here even though the core
/// would resolve [`RowIdentity::SqliteRowid`]. Widening this is future
/// work (it needs the browse query to also project `rowid`).
#[must_use]
pub fn is_editable(schema: &TableSchema, dialect: SqlDialect) -> bool {
    matches!(
        RowIdentity::resolve(schema, dialect, false),
        Some(RowIdentity::PrimaryKey(_))
    )
}

/// A single-row `UPDATE` ready to run, plus the staged `(row, col)` cells
/// it commits so the caller can clear their dirty state once the engine
/// confirms exactly one row changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedUpdate {
    /// Result-grid row index this update targets.
    pub row: usize,
    /// Result-grid column indices whose staged edits this update writes.
    pub columns: Vec<usize>,
    /// The fully-escaped `UPDATE … SET … WHERE …` statement.
    pub sql: String,
}

/// Why staged edits could not be turned into runnable `UPDATE`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The table has no primary key the UI can key an update on.
    NotEditable,
    /// A primary-key column is absent from the result's columns, so the
    /// row's identity value cannot be read from the grid.
    MissingKeyColumn(String),
    /// A staged column index is out of range for the result.
    UnknownColumn(usize),
    /// A staged row index is out of range for the result.
    UnknownRow(usize),
    /// The core write-back layer rejected the plan.
    WriteBack(WriteBackError),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEditable => write!(f, "this result has no primary key to edit on"),
            Self::MissingKeyColumn(c) => {
                write!(f, "primary-key column {c:?} is not in the result")
            }
            Self::UnknownColumn(i) => write!(f, "edited column index {i} is out of range"),
            Self::UnknownRow(i) => write!(f, "edited row index {i} is out of range"),
            Self::WriteBack(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EditError {}

/// Turn the staged edits into one `UPDATE` per touched row.
///
/// `schema` supplies the primary key; `result` supplies both the column
/// names (to map staged indices to names) and each row's original
/// primary-key values (to key the `WHERE`). Rows and columns are emitted
/// in ascending index order so the output — and history — are
/// deterministic. Returns an empty `Vec` when nothing is staged.
///
/// # Errors
///
/// Returns [`EditError`] when the table is not editable, a staged
/// row/column is out of range, a primary-key column is missing from the
/// result, or the core rejects the generated plan.
pub fn build_update_plans(
    table: &TableInfo,
    schema: &TableSchema,
    dialect: SqlDialect,
    result: &QueryResult,
    staged: &BTreeMap<(usize, usize), StagedValue>,
) -> Result<Vec<PlannedUpdate>, EditError> {
    let pk_cols = match RowIdentity::resolve(schema, dialect, false) {
        Some(RowIdentity::PrimaryKey(cols)) => cols,
        // SqliteRowid can't be keyed from a `SELECT *` grid, and `None`
        // means no safe identity at all — neither is editable here.
        Some(RowIdentity::SqliteRowid) | None => return Err(EditError::NotEditable),
    };

    // Resolve each primary-key column to its position in the result once;
    // every row reuses the same layout.
    let pk_indices = pk_cols
        .iter()
        .map(|pk| {
            result
                .columns
                .iter()
                .position(|c| &c.name == pk)
                .map(|idx| (pk.clone(), idx))
                .ok_or_else(|| EditError::MissingKeyColumn(pk.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Group staged cells by row (BTreeMap keeps rows, then columns, in
    // ascending order).
    let mut by_row: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &(row, col) in staged.keys() {
        by_row.entry(row).or_default().push(col);
    }

    let mut plans = Vec::with_capacity(by_row.len());
    for (row, columns) in by_row {
        let row_values = result.rows.get(row).ok_or(EditError::UnknownRow(row))?;

        let edits = columns
            .iter()
            .map(|&col| {
                let name = result
                    .columns
                    .get(col)
                    .ok_or(EditError::UnknownColumn(col))?
                    .name
                    .clone();
                // Present by construction: `col` came from `staged`'s keys.
                let value = staged[&(row, col)].to_cell();
                Ok((name, value))
            })
            .collect::<Result<Vec<_>, EditError>>()?;

        let key = pk_indices
            .iter()
            .map(|(pk, idx)| {
                let value = row_values.get(*idx).ok_or(EditError::UnknownColumn(*idx))?;
                Ok((pk.clone(), value.clone()))
            })
            .collect::<Result<Vec<_>, EditError>>()?;

        let plan = UpdatePlan {
            table: table.clone(),
            key: RowKey::Columns(key),
            edits,
        };
        let sql = build_update_sql(&plan, dialect).map_err(EditError::WriteBack)?;
        plans.push(PlannedUpdate { row, columns, sql });
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_core::{Column, Row, Value};

    fn col(name: &str, pk: bool) -> dbboard_core::ColumnInfo {
        dbboard_core::ColumnInfo {
            name: name.to_owned(),
            declared_type: Some("TEXT".to_owned()),
            nullable: !pk,
            primary_key: pk,
            ordinal: 0,
            default_value: None,
        }
    }

    fn schema(cols: &[(&str, bool)]) -> TableSchema {
        TableSchema {
            table: TableInfo::unqualified("t"),
            columns: cols.iter().map(|(n, pk)| col(n, *pk)).collect(),
            primary_key: cols
                .iter()
                .filter(|(_, pk)| *pk)
                .map(|(n, _)| (*n).to_owned())
                .collect(),
        }
    }

    fn result(col_names: &[&str], rows: Vec<Vec<Value>>) -> QueryResult {
        QueryResult {
            columns: col_names
                .iter()
                .map(|n| Column {
                    name: (*n).to_owned(),
                    declared_type: None,
                })
                .collect(),
            rows: rows.into_iter().map(Row::new).collect(),
            rows_affected: 0,
        }
    }

    fn staged(cells: &[(usize, usize, StagedValue)]) -> BTreeMap<(usize, usize), StagedValue> {
        cells
            .iter()
            .map(|(r, c, v)| ((*r, *c), v.clone()))
            .collect()
    }

    // ---- dialect_for_adapter_id ----------------------------------------

    #[test]
    fn adapter_ids_map_to_their_dialect_families() {
        for id in ["turso", "d1"] {
            assert_eq!(dialect_for_adapter_id(id), Some(SqlDialect::Sqlite));
        }
        for id in ["postgres", "neon", "supabase", "aurora-dsql"] {
            assert_eq!(dialect_for_adapter_id(id), Some(SqlDialect::Postgres));
        }
    }

    #[test]
    fn unknown_adapter_id_disables_editing() {
        assert_eq!(dialect_for_adapter_id("mystery"), None);
    }

    // ---- is_editable ----------------------------------------------------

    #[test]
    fn a_table_with_a_primary_key_is_editable_on_both_dialects() {
        let s = schema(&[("id", true), ("name", false)]);
        assert!(is_editable(&s, SqlDialect::Sqlite));
        assert!(is_editable(&s, SqlDialect::Postgres));
    }

    #[test]
    fn a_sqlite_table_without_a_pk_is_not_editable_in_the_ui() {
        // The core resolves SqliteRowid, but the UI can't read a rowid
        // from a `SELECT *` grid, so editing stays off.
        let s = schema(&[("a", false)]);
        assert!(!is_editable(&s, SqlDialect::Sqlite));
    }

    #[test]
    fn a_postgres_table_without_a_pk_is_not_editable() {
        let s = schema(&[("a", false)]);
        assert!(!is_editable(&s, SqlDialect::Postgres));
    }

    // ---- build_update_plans: happy paths --------------------------------

    #[test]
    fn one_staged_cell_yields_one_keyed_update() {
        let s = schema(&[("id", true), ("name", false)]);
        let r = result(
            &["id", "name"],
            vec![vec![Value::Integer(7), Value::Text("old".into())]],
        );
        let st = staged(&[(0, 1, StagedValue::Text("new".into()))]);
        let plans = build_update_plans(
            &TableInfo::unqualified("t"),
            &s,
            SqlDialect::Sqlite,
            &r,
            &st,
        )
        .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].row, 0);
        assert_eq!(plans[0].columns, vec![1]);
        assert_eq!(
            plans[0].sql,
            r#"UPDATE "t" SET "name" = 'new' WHERE "id" = 7"#
        );
    }

    #[test]
    fn several_cells_on_one_row_collapse_into_a_single_update() {
        let s = schema(&[("id", true), ("a", false), ("b", false)]);
        let r = result(
            &["id", "a", "b"],
            vec![vec![
                Value::Integer(1),
                Value::Text("x".into()),
                Value::Text("y".into()),
            ]],
        );
        let st = staged(&[
            (0, 1, StagedValue::Text("A".into())),
            (0, 2, StagedValue::Null),
        ]);
        let plans = build_update_plans(
            &TableInfo::unqualified("t"),
            &s,
            SqlDialect::Sqlite,
            &r,
            &st,
        )
        .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].columns, vec![1, 2]);
        assert_eq!(
            plans[0].sql,
            r#"UPDATE "t" SET "a" = 'A', "b" = NULL WHERE "id" = 1"#
        );
    }

    #[test]
    fn edits_on_different_rows_produce_one_update_each_in_row_order() {
        let s = schema(&[("id", true), ("v", false)]);
        let r = result(
            &["id", "v"],
            vec![
                vec![Value::Integer(10), Value::Text("a".into())],
                vec![Value::Integer(20), Value::Text("b".into())],
            ],
        );
        // Insert the higher row first to prove ordering is by index.
        let st = staged(&[
            (1, 1, StagedValue::Text("B".into())),
            (0, 1, StagedValue::Text("A".into())),
        ]);
        let plans = build_update_plans(
            &TableInfo::unqualified("t"),
            &s,
            SqlDialect::Sqlite,
            &r,
            &st,
        )
        .unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].row, 0);
        assert_eq!(plans[0].sql, r#"UPDATE "t" SET "v" = 'A' WHERE "id" = 10"#);
        assert_eq!(plans[1].row, 1);
        assert_eq!(plans[1].sql, r#"UPDATE "t" SET "v" = 'B' WHERE "id" = 20"#);
    }

    #[test]
    fn composite_key_uses_every_pk_column_from_the_row() {
        let s = schema(&[("order_id", true), ("line_no", true), ("qty", false)]);
        let r = result(
            &["order_id", "line_no", "qty"],
            vec![vec![
                Value::Integer(5),
                Value::Integer(2),
                Value::Integer(9),
            ]],
        );
        let st = staged(&[(0, 2, StagedValue::Text("99".into()))]);
        let plans = build_update_plans(
            &TableInfo::qualified("public", "lines"),
            &s,
            SqlDialect::Postgres,
            &r,
            &st,
        )
        .unwrap();
        assert_eq!(
            plans[0].sql,
            r#"UPDATE "public"."lines" SET "qty" = '99' WHERE "order_id" = 5 AND "line_no" = 2"#
        );
    }

    #[test]
    fn a_pk_column_ordered_after_the_edited_column_still_keys_correctly() {
        // PK column sits at index 1 in the grid; the edit is at index 0.
        let s = schema(&[("name", false), ("id", true)]);
        let r = result(
            &["name", "id"],
            vec![vec![Value::Text("old".into()), Value::Integer(3)]],
        );
        let st = staged(&[(0, 0, StagedValue::Text("new".into()))]);
        let plans = build_update_plans(
            &TableInfo::unqualified("t"),
            &s,
            SqlDialect::Sqlite,
            &r,
            &st,
        )
        .unwrap();
        assert_eq!(
            plans[0].sql,
            r#"UPDATE "t" SET "name" = 'new' WHERE "id" = 3"#
        );
    }

    #[test]
    fn nothing_staged_yields_no_plans() {
        let s = schema(&[("id", true)]);
        let r = result(&["id"], vec![vec![Value::Integer(1)]]);
        let plans = build_update_plans(
            &TableInfo::unqualified("t"),
            &s,
            SqlDialect::Sqlite,
            &r,
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(plans.is_empty());
    }

    // ---- build_update_plans: refusals -----------------------------------

    #[test]
    fn a_table_without_a_pk_refuses_to_plan() {
        let s = schema(&[("a", false)]);
        let r = result(&["a"], vec![vec![Value::Text("x".into())]]);
        let st = staged(&[(0, 0, StagedValue::Text("y".into()))]);
        assert_eq!(
            build_update_plans(
                &TableInfo::unqualified("t"),
                &s,
                SqlDialect::Postgres,
                &r,
                &st
            ),
            Err(EditError::NotEditable)
        );
    }

    #[test]
    fn a_pk_column_missing_from_the_result_is_reported() {
        // Schema says `id` is the PK, but the projected result lacks it.
        let s = schema(&[("id", true), ("v", false)]);
        let r = result(&["v"], vec![vec![Value::Text("x".into())]]);
        let st = staged(&[(0, 0, StagedValue::Text("y".into()))]);
        assert_eq!(
            build_update_plans(
                &TableInfo::unqualified("t"),
                &s,
                SqlDialect::Sqlite,
                &r,
                &st
            ),
            Err(EditError::MissingKeyColumn("id".to_owned()))
        );
    }

    #[test]
    fn a_staged_column_out_of_range_is_reported() {
        let s = schema(&[("id", true), ("v", false)]);
        let r = result(
            &["id", "v"],
            vec![vec![Value::Integer(1), Value::Text("x".into())]],
        );
        let st = staged(&[(0, 5, StagedValue::Text("y".into()))]);
        assert_eq!(
            build_update_plans(
                &TableInfo::unqualified("t"),
                &s,
                SqlDialect::Sqlite,
                &r,
                &st
            ),
            Err(EditError::UnknownColumn(5))
        );
    }

    #[test]
    fn a_staged_row_out_of_range_is_reported() {
        let s = schema(&[("id", true), ("v", false)]);
        let r = result(
            &["id", "v"],
            vec![vec![Value::Integer(1), Value::Text("x".into())]],
        );
        let st = staged(&[(9, 1, StagedValue::Text("y".into()))]);
        assert_eq!(
            build_update_plans(
                &TableInfo::unqualified("t"),
                &s,
                SqlDialect::Sqlite,
                &r,
                &st
            ),
            Err(EditError::UnknownRow(9))
        );
    }
}
