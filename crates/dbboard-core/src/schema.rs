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
}

#[cfg(test)]
mod tests {
    use super::{ColumnInfo, TableInfo};

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
        };
        assert!(!c.nullable);
        assert!(c.primary_key);
    }
}
