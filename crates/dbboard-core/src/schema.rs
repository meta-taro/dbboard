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

#[cfg(test)]
mod tests {
    use super::{ColumnInfo, TableInfo, TableSchema};

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
}
