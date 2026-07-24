//! Database schema metadata returned by adapter introspection.
//!
//! These types are deliberately minimal in Phase 1 — enough to render
//! a "tables in this database" sidebar in the UI. Richer fields
//! (constraints, indexes, foreign keys) land alongside the adapter
//! trait in Phase 2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableInfo {
    /// Schema namespace (e.g. `"public"` in PostgreSQL). `None` for
    /// SQLite/libSQL where there is no schema concept.
    pub schema: Option<String>,
    pub name: String,
}

impl TableInfo {
    #[must_use]
    pub fn unqualified(name: impl Into<String>) -> Self {
        Self {
            schema: None,
            name: name.into(),
        }
    }

    #[must_use]
    pub fn qualified(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: Some(schema.into()),
            name: name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub declared_type: Option<String>,
    pub nullable: bool,
    pub primary_key: bool,
    /// 1-based position within the table. Postgres reports
    /// `ordinal_position` (already 1-based); SQLite's `PRAGMA
    /// table_info.cid` is 0-based and adapters normalise it to 1-based
    /// (ADR-0028 Decision 3). `#[serde(default)]` keeps pre-ADR-0028
    /// payloads parseable — they deserialize as `0` ("unknown").
    #[serde(default)]
    pub ordinal: u32,
    /// Raw DDL default expression exactly as the engine reports it,
    /// e.g. `nextval('users_id_seq'::regclass)` or `CURRENT_TIMESTAMP`.
    /// `None` when the column has no default clause. Kept as literal
    /// text: typed parsing would be lossy for sequence calls and
    /// engine-specific expressions (ADR-0028 Decision 3).
    #[serde(default)]
    pub default_value: Option<String>,
}

/// Full per-table description returned by
/// [`DatabaseAdapter::describe_table`] (ADR-0028).
///
/// [`DatabaseAdapter::describe_table`]: crate::DatabaseAdapter::describe_table
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSchema {
    /// The `TableInfo` the caller passed to `describe_table` — echoed
    /// back verbatim, schema-qualified only where the engine has
    /// schemas (SQLite/libSQL tables stay unqualified).
    pub table: TableInfo,
    /// Columns ordered by ordinal position (each adapter's native order).
    pub columns: Vec<ColumnInfo>,
    /// Composite primary-key column names in key order; empty when the
    /// table has no primary key. Adapters keep this consistent with the
    /// per-column `primary_key` flags — readers may trust either.
    pub primary_key: Vec<String>,
}

/// One foreign-key constraint on a table, returned by
/// [`DatabaseAdapter::foreign_keys`] (ADR-0054).
///
/// A composite key spans several columns: `columns` and
/// `referenced_columns` are aligned 1:1 in key order (the first local
/// column references the first referenced column, and so on). Both are
/// non-empty for a well-formed constraint.
///
/// This carries only what relationship discovery needs — the endpoints
/// of the edge. Referential actions (`ON DELETE` / `ON UPDATE`) are
/// deliberately out of scope (ADR-0054); a caller that needs them can
/// read the table's DDL via [`DatabaseAdapter::table_ddl`].
///
/// [`DatabaseAdapter::foreign_keys`]: crate::DatabaseAdapter::foreign_keys
/// [`DatabaseAdapter::table_ddl`]: crate::DatabaseAdapter::table_ddl
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKey {
    /// Local (referencing) columns, in key order.
    pub columns: Vec<String>,
    /// The referenced (parent) table. Schema-qualified only where the
    /// engine has schemas — SQLite/libSQL/D1 references stay unqualified.
    pub referenced_table: TableInfo,
    /// Referenced (parent) columns, aligned 1:1 with [`columns`](Self::columns).
    pub referenced_columns: Vec<String>,
    /// The constraint name where the engine reports one (Postgres);
    /// `None` for SQLite/libSQL/D1, whose `PRAGMA foreign_key_list` does
    /// not name the constraint.
    pub constraint_name: Option<String>,
}

/// Resolve the referenced (parent) columns of a SQLite-family foreign key.
///
/// `PRAGMA foreign_key_list` reports each referenced column in `to`, but
/// leaves it `None` when the DDL omitted the parent column list — an
/// implicit reference to the parent's primary key. `to` and `parent_pk` are
/// both in key order; for each position this prefers the explicit `to`
/// value, falls back to the parent PK at that position, and finally to
/// `"rowid"` — SQLite's actual implicit reference target for a rowid table
/// that declares no named primary key.
///
/// Shared by the Turso and D1 adapters so their identical PRAGMA-shaped
/// resolution cannot drift. Postgres reports referenced columns directly and
/// does not use this.
#[must_use]
pub fn resolve_referenced_columns(to: &[Option<String>], parent_pk: &[String]) -> Vec<String> {
    to.iter()
        .enumerate()
        .map(|(i, col)| {
            col.clone()
                .or_else(|| parent_pk.get(i).cloned())
                .unwrap_or_else(|| "rowid".to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{resolve_referenced_columns, ColumnInfo, ForeignKey, TableInfo, TableSchema};

    #[test]
    fn unqualified_table_has_no_schema() {
        let t = TableInfo::unqualified("users");
        assert_eq!(t.schema, None);
        assert_eq!(t.name, "users");
    }

    #[test]
    fn qualified_table_carries_schema_and_name() {
        let t = TableInfo::qualified("public", "users");
        assert_eq!(t.schema.as_deref(), Some("public"));
        assert_eq!(t.name, "users");
    }

    #[test]
    fn unqualified_table_serializes_schema_as_null() {
        let json = serde_json::to_string(&TableInfo::unqualified("users")).unwrap();
        assert_eq!(json, r#"{"schema":null,"name":"users"}"#);
    }

    #[test]
    fn table_info_round_trips_both_forms() {
        for t in [
            TableInfo::unqualified("users"),
            TableInfo::qualified("public", "users"),
        ] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(serde_json::from_str::<TableInfo>(&json).unwrap(), t);
        }
    }

    #[test]
    fn column_info_holds_metadata_flags() {
        let c = ColumnInfo {
            name: "id".into(),
            declared_type: Some("INTEGER".into()),
            nullable: false,
            primary_key: true,
            ordinal: 1,
            default_value: Some("nextval('users_id_seq'::regclass)".into()),
        };
        assert!(!c.nullable);
        assert!(c.primary_key);
        assert_eq!(c.ordinal, 1);
        assert_eq!(
            c.default_value.as_deref(),
            Some("nextval('users_id_seq'::regclass)")
        );
    }

    #[test]
    fn column_info_without_new_fields_deserializes_with_defaults() {
        // Pre-ADR-0028 wire shape: no `ordinal`, no `default_value`.
        let json = r#"{"name":"id","declared_type":"INTEGER","nullable":false,"primary_key":true}"#;
        let c: ColumnInfo = serde_json::from_str(json).unwrap();
        assert_eq!(c.ordinal, 0);
        assert_eq!(c.default_value, None);
    }

    fn sample_column(name: &str, ordinal: u32, primary_key: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            declared_type: Some("TEXT".into()),
            nullable: !primary_key,
            primary_key,
            ordinal,
            default_value: None,
        }
    }

    #[test]
    fn table_schema_round_trips_through_json() {
        let schema = TableSchema {
            table: TableInfo::qualified("public", "order_items"),
            columns: vec![
                sample_column("order_id", 1, true),
                sample_column("line_no", 2, true),
                sample_column("sku", 3, false),
            ],
            primary_key: vec!["order_id".into(), "line_no".into()],
        };
        let json = serde_json::to_string(&schema).unwrap();
        assert_eq!(serde_json::from_str::<TableSchema>(&json).unwrap(), schema);
    }

    #[test]
    fn table_schema_allows_empty_primary_key() {
        let schema = TableSchema {
            table: TableInfo::unqualified("audit_log"),
            columns: vec![sample_column("entry", 1, false)],
            primary_key: Vec::new(),
        };
        assert!(schema.primary_key.is_empty());
        assert!(!schema.columns[0].primary_key);
    }

    #[test]
    fn foreign_key_round_trips_through_json() {
        let fk = ForeignKey {
            columns: vec!["customer_id".into()],
            referenced_table: TableInfo::qualified("public", "customers"),
            referenced_columns: vec!["id".into()],
            constraint_name: Some("orders_customer_id_fkey".into()),
        };
        let json = serde_json::to_string(&fk).unwrap();
        assert_eq!(serde_json::from_str::<ForeignKey>(&json).unwrap(), fk);
    }

    #[test]
    fn foreign_key_carries_composite_key_columns_in_order() {
        // A two-column key: the local/referenced columns are paired by
        // position, so order is load-bearing.
        let fk = ForeignKey {
            columns: vec!["order_id".into(), "line_no".into()],
            referenced_table: TableInfo::unqualified("order_lines"),
            referenced_columns: vec!["order".into(), "line".into()],
            constraint_name: None,
        };
        assert_eq!(fk.columns.len(), fk.referenced_columns.len());
        assert_eq!(fk.columns[1], "line_no");
        assert_eq!(fk.referenced_columns[1], "line");
        assert_eq!(fk.referenced_table.name, "order_lines");
    }

    #[test]
    fn resolve_referenced_columns_uses_explicit_targets() {
        // Every column named explicitly (the DDL gave a parent column list):
        // the parent PK is not consulted.
        let to = [Some("a".to_string()), Some("b".to_string())];
        assert_eq!(
            resolve_referenced_columns(&to, &[]),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn resolve_referenced_columns_falls_back_to_parent_pk_in_order() {
        // Implicit reference (parent column list omitted): resolve each
        // position against the parent's primary key, in key order.
        let to = [None, None];
        let pk = ["id".to_string(), "region".to_string()];
        assert_eq!(
            resolve_referenced_columns(&to, &pk),
            vec!["id".to_string(), "region".to_string()]
        );
    }

    #[test]
    fn resolve_referenced_columns_defaults_to_rowid_without_a_named_pk() {
        // A rowid table has no named primary key, so an implicit single-column
        // reference targets SQLite's rowid rather than an empty string.
        assert_eq!(
            resolve_referenced_columns(&[None], &[]),
            vec!["rowid".to_string()]
        );
    }
}
