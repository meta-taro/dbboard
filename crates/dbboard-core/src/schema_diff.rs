//! Structural comparison of two database schemas (ADR-0148).
//!
//! Pure and I/O-free: it takes two sets of already-introspected tables and
//! says what they disagree about. Fetching them, and refusing a comparison
//! that makes no sense, belong to the layer that knows what a connection is.
//!
//! **Nothing is normalised.** Types are compared exactly as the engine
//! spelled them, because deciding that `VARCHAR(255)` and `varchar` are the
//! same column needs engine knowledge this layer does not have — and a diff
//! has one failure mode that matters more than the others. Reporting a
//! difference that turns out to be cosmetic costs the reader a second look;
//! *missing* one costs them the migration. Every judgement call here leans
//! the first way.
//!
//! What is deliberately **not** compared: column order (two databases holding
//! the same columns in a different order are the same schema for every
//! practical purpose, and the noise would bury the real findings), and
//! indexes and constraints beyond the primary key (each adapter reaches those
//! differently, and half of the eleven have not been asked yet).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{ColumnInfo, TableInfo, TableSchema};

/// Which attribute of a column the two sides disagree about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnField {
    DeclaredType,
    Nullable,
    Default,
    PrimaryKey,
}

/// One column present on both sides, with at least one differing attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDiff {
    pub name: String,
    pub left: ColumnInfo,
    pub right: ColumnInfo,
    pub fields: Vec<ColumnField>,
}

/// One table present on both sides, with at least one difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableDiff {
    pub table: TableInfo,
    pub columns_only_in_left: Vec<ColumnInfo>,
    pub columns_only_in_right: Vec<ColumnInfo>,
    pub columns_changed: Vec<ColumnDiff>,
    pub primary_key: Option<(Vec<String>, Vec<String>)>,
}

/// Everything two schemas disagree about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaDiff {
    pub tables_only_in_left: Vec<TableInfo>,
    pub tables_only_in_right: Vec<TableInfo>,
    pub tables_changed: Vec<TableDiff>,
}

impl SchemaDiff {
    /// Whether the two schemas matched in every compared respect.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables_only_in_left.is_empty()
            && self.tables_only_in_right.is_empty()
            && self.tables_changed.is_empty()
    }
}

/// Compare two schemas, table by table and column by column.
///
/// Tables are matched on their schema-qualified name — `public.orders` and
/// `staging.orders` are different tables — and columns on their name. A table
/// that exists on one side only is reported once, as missing, and not also as
/// a table full of missing columns: one finding per fact, or the count of
/// differences lies.
///
/// Output is ordered (tables by qualified name, columns by name) because the
/// inputs arrive in each adapter's native order, and a report that reshuffles
/// between runs cannot be compared against the last one.
#[must_use]
pub fn diff_schemas(left: &[TableSchema], right: &[TableSchema]) -> SchemaDiff {
    let left_by_key: BTreeMap<String, &TableSchema> =
        left.iter().map(|t| (table_key(&t.table), t)).collect();
    let right_by_key: BTreeMap<String, &TableSchema> =
        right.iter().map(|t| (table_key(&t.table), t)).collect();

    let only_in_left = left_by_key
        .iter()
        .filter(|(key, _)| !right_by_key.contains_key(*key))
        .map(|(_, t)| t.table.clone())
        .collect();
    let only_in_right = right_by_key
        .iter()
        .filter(|(key, _)| !left_by_key.contains_key(*key))
        .map(|(_, t)| t.table.clone())
        .collect();

    let tables_changed = left_by_key
        .iter()
        .filter_map(|(key, l)| right_by_key.get(key).map(|r| (*l, *r)))
        .filter_map(|(l, r)| diff_table(l, r))
        .collect();

    SchemaDiff {
        tables_only_in_left: only_in_left,
        tables_only_in_right: only_in_right,
        tables_changed,
    }
}

/// The differences between one table's two sides, or `None` when there are
/// none — an unchanged table is absent from the report, which is what keeps a
/// 300-table database with one difference to one entry.
fn diff_table(left: &TableSchema, right: &TableSchema) -> Option<TableDiff> {
    let left_cols: BTreeMap<&str, &ColumnInfo> =
        left.columns.iter().map(|c| (c.name.as_str(), c)).collect();
    let right_cols: BTreeMap<&str, &ColumnInfo> =
        right.columns.iter().map(|c| (c.name.as_str(), c)).collect();

    let only_in_left: Vec<ColumnInfo> = left_cols
        .iter()
        .filter(|(name, _)| !right_cols.contains_key(*name))
        .map(|(_, c)| (*c).clone())
        .collect();
    let only_in_right: Vec<ColumnInfo> = right_cols
        .iter()
        .filter(|(name, _)| !left_cols.contains_key(*name))
        .map(|(_, c)| (*c).clone())
        .collect();

    let changed: Vec<ColumnDiff> = left_cols
        .iter()
        .filter_map(|(name, l)| right_cols.get(name).map(|r| (*name, *l, *r)))
        .filter_map(|(name, l, r)| {
            let fields = differing_fields(l, r);
            (!fields.is_empty()).then(|| ColumnDiff {
                name: name.to_owned(),
                left: l.clone(),
                right: r.clone(),
                fields,
            })
        })
        .collect();

    // Key order is part of the key: `(a, b)` and `(b, a)` index different
    // things, so the vectors are compared as sequences rather than as sets.
    let primary_key = (left.primary_key != right.primary_key)
        .then(|| (left.primary_key.clone(), right.primary_key.clone()));

    if only_in_left.is_empty()
        && only_in_right.is_empty()
        && changed.is_empty()
        && primary_key.is_none()
    {
        return None;
    }
    Some(TableDiff {
        table: left.table.clone(),
        columns_only_in_left: only_in_left,
        columns_only_in_right: only_in_right,
        columns_changed: changed,
        primary_key,
    })
}

/// Which of a column's compared attributes differ, in a fixed order so two
/// runs produce the same list.
///
/// `ordinal` is absent on purpose: it is the column's position, and position
/// alone is not a difference.
fn differing_fields(left: &ColumnInfo, right: &ColumnInfo) -> Vec<ColumnField> {
    let mut fields = Vec::new();
    if left.declared_type != right.declared_type {
        fields.push(ColumnField::DeclaredType);
    }
    if left.nullable != right.nullable {
        fields.push(ColumnField::Nullable);
    }
    if left.default_value != right.default_value {
        fields.push(ColumnField::Default);
    }
    if left.primary_key != right.primary_key {
        fields.push(ColumnField::PrimaryKey);
    }
    fields
}

/// `schema.name` where the engine has schemas, the bare name where it does
/// not — the same shape the annotation store keys tables by.
fn table_key(table: &TableInfo) -> String {
    match &table.schema {
        Some(s) if !s.is_empty() => format!("{s}.{}", table.name),
        _ => table.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_owned(),
            declared_type: Some(ty.to_owned()),
            nullable: true,
            primary_key: false,
            ordinal: 1,
            default_value: None,
        }
    }

    fn table(name: &str, columns: Vec<ColumnInfo>) -> TableSchema {
        TableSchema {
            table: TableInfo::unqualified(name),
            columns,
            primary_key: Vec::new(),
        }
    }

    #[test]
    fn two_identical_schemas_have_no_differences() {
        let left = vec![table("orders", vec![col("id", "INTEGER")])];
        let right = left.clone();
        assert!(diff_schemas(&left, &right).is_empty());
    }

    #[test]
    fn a_table_on_one_side_only_is_reported_on_that_side() {
        let left = vec![table("orders", vec![col("id", "INTEGER")])];
        let right = vec![];
        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.tables_only_in_left.len(), 1);
        assert_eq!(diff.tables_only_in_left[0].name, "orders");
        assert!(diff.tables_only_in_right.is_empty());
        // A table missing entirely is not also reported as a changed table:
        // one finding per fact, or the count of differences lies.
        assert!(diff.tables_changed.is_empty());
        assert!(!diff.is_empty());
    }

    #[test]
    fn a_schema_qualified_table_matches_only_the_same_qualification() {
        // `public.orders` and `staging.orders` are different tables. Matching
        // on the bare name would silently compare one against the other and
        // report their columns as differences.
        let left = vec![TableSchema {
            table: TableInfo::qualified("public", "orders"),
            columns: vec![col("id", "INTEGER")],
            primary_key: Vec::new(),
        }];
        let right = vec![TableSchema {
            table: TableInfo::qualified("staging", "orders"),
            columns: vec![col("id", "INTEGER")],
            primary_key: Vec::new(),
        }];
        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.tables_only_in_left.len(), 1);
        assert_eq!(diff.tables_only_in_right.len(), 1);
        assert!(diff.tables_changed.is_empty());
    }

    #[test]
    fn a_column_on_one_side_only_is_reported_on_that_side() {
        let left = vec![table(
            "orders",
            vec![col("id", "INTEGER"), col("note", "TEXT")],
        )];
        let right = vec![table("orders", vec![col("id", "INTEGER")])];
        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.tables_changed.len(), 1);
        let t = &diff.tables_changed[0];
        assert_eq!(t.columns_only_in_left.len(), 1);
        assert_eq!(t.columns_only_in_left[0].name, "note");
        assert!(t.columns_only_in_right.is_empty());
        assert!(t.columns_changed.is_empty());
    }

    #[test]
    fn a_differing_declared_type_names_that_field() {
        let left = vec![table("orders", vec![col("total", "INTEGER")])];
        let right = vec![table("orders", vec![col("total", "REAL")])];
        let diff = diff_schemas(&left, &right);
        let changed = &diff.tables_changed[0].columns_changed;
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].name, "total");
        assert_eq!(changed[0].fields, vec![ColumnField::DeclaredType]);
        // Both sides are carried so the caller can show what each one says,
        // rather than only that they disagree.
        assert_eq!(changed[0].left.declared_type.as_deref(), Some("INTEGER"));
        assert_eq!(changed[0].right.declared_type.as_deref(), Some("REAL"));
    }

    #[test]
    fn types_are_compared_as_the_engine_spelled_them() {
        // No normalisation: `VARCHAR(255)` and `varchar` may well be the same
        // column, and deciding that needs engine knowledge this layer does not
        // have. Guessing wrong in the "these are the same" direction hides a
        // real difference, which is the one failure a diff must not have.
        let left = vec![table("orders", vec![col("name", "VARCHAR(255)")])];
        let right = vec![table("orders", vec![col("name", "varchar")])];
        let diff = diff_schemas(&left, &right);
        assert_eq!(
            diff.tables_changed[0].columns_changed[0].fields,
            vec![ColumnField::DeclaredType]
        );
    }

    #[test]
    fn nullability_default_and_key_flag_are_each_their_own_field() {
        let mut l = col("id", "INTEGER");
        l.nullable = false;
        l.primary_key = true;
        l.default_value = Some("0".to_owned());
        let mut r = col("id", "INTEGER");
        r.nullable = true;
        r.primary_key = false;
        r.default_value = None;

        let diff = diff_schemas(&[table("t", vec![l])], &[table("t", vec![r])]);
        let fields = &diff.tables_changed[0].columns_changed[0].fields;
        assert_eq!(
            fields,
            &vec![
                ColumnField::DeclaredType,
                ColumnField::Nullable,
                ColumnField::Default,
                ColumnField::PrimaryKey,
            ][1..]
                .to_vec()
        );
    }

    #[test]
    fn column_order_alone_is_not_a_difference() {
        // Two databases that hold the same columns in a different order are
        // the same schema for every practical purpose. Reporting the order
        // would bury the real differences under noise from every table that
        // was ever rebuilt.
        let left = vec![table("t", vec![col("a", "TEXT"), col("b", "TEXT")])];
        let right = vec![table("t", vec![col("b", "TEXT"), col("a", "TEXT")])];
        assert!(diff_schemas(&left, &right).is_empty());
    }

    #[test]
    fn a_differing_primary_key_is_reported_once_for_the_table() {
        let left = vec![TableSchema {
            table: TableInfo::unqualified("t"),
            columns: vec![col("a", "TEXT")],
            primary_key: vec!["a".to_owned()],
        }];
        let right = vec![TableSchema {
            table: TableInfo::unqualified("t"),
            columns: vec![col("a", "TEXT")],
            primary_key: vec!["a".to_owned(), "b".to_owned()],
        }];
        let diff = diff_schemas(&left, &right);
        let t = &diff.tables_changed[0];
        assert_eq!(
            t.primary_key,
            Some((vec!["a".to_owned()], vec!["a".to_owned(), "b".to_owned()]))
        );
    }

    #[test]
    fn key_order_matters_for_a_composite_key() {
        // `(a, b)` and `(b, a)` index different things; a diff that called
        // them equal would hide a real performance and uniqueness difference.
        let left = vec![TableSchema {
            table: TableInfo::unqualified("t"),
            columns: vec![col("a", "TEXT")],
            primary_key: vec!["a".to_owned(), "b".to_owned()],
        }];
        let right = vec![TableSchema {
            table: TableInfo::unqualified("t"),
            columns: vec![col("a", "TEXT")],
            primary_key: vec!["b".to_owned(), "a".to_owned()],
        }];
        assert!(diff_schemas(&left, &right).tables_changed[0]
            .primary_key
            .is_some());
    }

    #[test]
    fn output_is_ordered_so_two_runs_read_the_same() {
        // The inputs arrive in each adapter's native order. Without an order
        // of its own the report would reshuffle between runs, and a diff that
        // reshuffles cannot be compared against the last one.
        let left = vec![
            table("zebra", vec![col("z", "TEXT")]),
            table("alpha", vec![col("b", "TEXT"), col("a", "TEXT")]),
        ];
        let right = vec![];
        let diff = diff_schemas(&left, &right);
        let names: Vec<&str> = diff
            .tables_only_in_left
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "zebra"]);

        let both = diff_schemas(
            &[table("t", vec![col("b", "TEXT"), col("a", "TEXT")])],
            &[table("t", vec![])],
        );
        let cols: Vec<&str> = both.tables_changed[0]
            .columns_only_in_left
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(cols, vec!["a", "b"]);
    }

    #[test]
    fn an_unchanged_table_is_absent_from_the_report() {
        // The report is what differs, not an inventory. A database with 300
        // tables and one difference should produce one entry.
        let left = vec![
            table("same", vec![col("a", "TEXT")]),
            table("differs", vec![col("a", "TEXT")]),
        ];
        let right = vec![
            table("same", vec![col("a", "TEXT")]),
            table("differs", vec![col("a", "INTEGER")]),
        ];
        let diff = diff_schemas(&left, &right);
        assert_eq!(diff.tables_changed.len(), 1);
        assert_eq!(diff.tables_changed[0].table.name, "differs");
    }

    #[test]
    fn comparing_two_empty_schemas_is_empty_not_an_error() {
        assert!(diff_schemas(&[], &[]).is_empty());
    }
}
