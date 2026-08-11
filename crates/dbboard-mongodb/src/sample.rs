//! Inferring a collection's shape from a sample of its documents.
//!
//! A `MongoDB` collection declares no schema — two documents in the same
//! collection can share no field but `_id`. So there is nothing to *read*; the
//! only honest answer is to look at some documents and report what they happen
//! to contain (ADR-0091 §4).
//!
//! That makes the wording load-bearing. `TableSchema` has no field for "this
//! is an inference", so the caveat rides in `declared_type`: every type here
//! carries the sample it came from (`string (12/20 sampled)`), which is both
//! the disclaimer and the evidence for the `nullable` flag beside it.
//!
//! The type names are `MongoDB`'s own `$type` aliases rather than Rust names,
//! so a description can be pasted straight into the query that acts on it:
//! seeing `long` in a column tells you `{"$type": "long"}` will match it.

use std::collections::{BTreeMap, BTreeSet};

use dbboard_core::{ColumnInfo, TableInfo, TableSchema};
use mongodb::bson::{Bson, Document};

use crate::document::ID_COLUMN;

/// What one field looked like across the sample.
#[derive(Default)]
struct Observed {
    /// Type names seen, deduplicated and ordered so the rendering is stable.
    types: BTreeSet<&'static str>,
    /// How many sampled documents carried the field at all.
    present: usize,
    /// Whether any document carried it as an explicit BSON null.
    explicit_null: bool,
}

/// Describe `table` from the documents in `documents`.
///
/// Infallible on purpose: `describe_table` answers "what is in here", and
/// refusing the whole answer because one document holds something odd hides
/// exactly the thing worth seeing.
pub(crate) fn infer_schema(table: &TableInfo, documents: &[Document]) -> TableSchema {
    let sampled = documents.len();
    let mut observed: BTreeMap<&str, Observed> = BTreeMap::new();

    for document in documents {
        for (name, value) in document {
            let entry = observed.entry(name.as_str()).or_default();
            entry.present += 1;
            let named = type_name(value);
            entry.explicit_null |= named == "null";
            entry.types.insert(named);
        }
    }

    // Split out so the id keeps slot 1 whatever the sample looked like, and so
    // the loop below does not have to special-case it twice.
    let id = observed.remove(ID_COLUMN);
    let mut columns = Vec::with_capacity(observed.len() + 1);
    columns.push(ColumnInfo {
        name: ID_COLUMN.to_string(),
        declared_type: id.map(|seen| rendered_type(&seen, sampled)),
        // Every document has one. A sample that suggests otherwise is a sample
        // of something that is not a collection.
        nullable: false,
        primary_key: true,
        ordinal: 1,
        default_value: None,
    });

    for (ordinal, (name, seen)) in observed.into_iter().enumerate() {
        columns.push(ColumnInfo {
            name: name.to_string(),
            declared_type: Some(rendered_type(&seen, sampled)),
            // Absent and present-but-null both mean the column can be empty.
            nullable: seen.present < sampled || seen.explicit_null,
            primary_key: false,
            // +2: 1-based, and the id already took slot 1.
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX - 2) + 2,
            default_value: None,
        });
    }

    TableSchema {
        table: table.clone(),
        columns,
        primary_key: vec![ID_COLUMN.to_string()],
    }
}

/// The types seen, with the sample they came from — the disclaimer and the
/// evidence in one string.
fn rendered_type(seen: &Observed, sampled: usize) -> String {
    let types = seen.types.iter().copied().collect::<Vec<_>>().join("|");
    format!("{types} ({}/{sampled} sampled)", seen.present)
}

/// `MongoDB`'s `$type` alias for a value.
fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::Binary(_) => "binData",
        Bson::Undefined => "undefined",
        Bson::ObjectId(_) => "objectId",
        Bson::Boolean(_) => "bool",
        Bson::DateTime(_) => "date",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::DbPointer(_) => "dbPointer",
        Bson::JavaScriptCode(_) => "javascript",
        Bson::Symbol(_) => "symbol",
        Bson::JavaScriptCodeWithScope(_) => "javascriptWithScope",
        Bson::Int32(_) => "int",
        Bson::Timestamp(_) => "timestamp",
        Bson::Int64(_) => "long",
        Bson::Decimal128(_) => "decimal",
        Bson::MinKey => "minKey",
        Bson::MaxKey => "maxKey",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId};

    fn users() -> TableInfo {
        TableInfo::unqualified("users")
    }

    fn column<'a>(schema: &'a TableSchema, name: &str) -> &'a ColumnInfo {
        schema
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no column named {name}"))
    }

    #[test]
    fn the_id_comes_first_and_is_the_primary_key() {
        let schema = infer_schema(&users(), &[doc! { "_id": 1, "name": "a" }]);
        assert_eq!(schema.columns[0].name, ID_COLUMN);
        assert!(schema.columns[0].primary_key);
        assert_eq!(schema.primary_key, vec![ID_COLUMN.to_string()]);
    }

    #[test]
    fn the_id_is_never_nullable() {
        // Every document has one, and a sample that says otherwise is a
        // sample of something that is not a collection.
        let schema = infer_schema(&users(), &[doc! { "_id": 1 }, doc! { "_id": 2 }]);
        assert!(!column(&schema, ID_COLUMN).nullable);
    }

    #[test]
    fn a_field_every_document_carries_is_not_nullable() {
        let documents = [
            doc! { "_id": 1, "name": "a" },
            doc! { "_id": 2, "name": "b" },
        ];
        let schema = infer_schema(&users(), &documents);
        let name = column(&schema, "name");
        assert!(!name.nullable);
        assert_eq!(name.declared_type.as_deref(), Some("string (2/2 sampled)"));
    }

    #[test]
    fn a_field_only_some_documents_carry_is_nullable_and_says_how_many() {
        let documents = [doc! { "_id": 1, "nickname": "a" }, doc! { "_id": 2 }];
        let schema = infer_schema(&users(), &documents);
        let nickname = column(&schema, "nickname");
        assert!(nickname.nullable);
        assert_eq!(
            nickname.declared_type.as_deref(),
            Some("string (1/2 sampled)")
        );
    }

    #[test]
    fn a_field_carried_as_null_everywhere_is_still_nullable() {
        let documents = [doc! { "_id": 1, "deleted_at": Bson::Null }];
        let schema = infer_schema(&users(), &documents);
        assert!(column(&schema, "deleted_at").nullable);
    }

    #[test]
    fn a_field_with_two_types_reports_both() {
        let documents = [
            doc! { "_id": 1, "zip": 100 },
            doc! { "_id": 2, "zip": "100" },
        ];
        let schema = infer_schema(&users(), &documents);
        assert_eq!(
            column(&schema, "zip").declared_type.as_deref(),
            Some("int|string (2/2 sampled)")
        );
    }

    #[test]
    fn the_type_names_are_the_ones_a_type_query_takes() {
        let id = ObjectId::parse_str("507f1f77bcf86cd799439011").unwrap();
        let documents = [doc! {
            "_id": id,
            "n": 1_i64,
            "x": 1.5,
            "ok": true,
            "tags": ["a"],
            "address": { "city": "Tokyo" },
        }];
        let schema = infer_schema(&users(), &documents);
        for (field, expected) in [
            (ID_COLUMN, "objectId"),
            ("n", "long"),
            ("x", "double"),
            ("ok", "bool"),
            ("tags", "array"),
            ("address", "object"),
        ] {
            assert_eq!(
                column(&schema, field).declared_type.as_deref(),
                Some(format!("{expected} (1/1 sampled)").as_str()),
                "wrong type name for {field}"
            );
        }
    }

    #[test]
    fn the_columns_after_the_id_are_sorted_and_numbered_from_two() {
        let schema = infer_schema(&users(), &[doc! { "_id": 1, "b": 1, "a": 1 }]);
        let names: Vec<_> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["_id", "a", "b"]);
        assert_eq!(schema.columns[0].ordinal, 1);
        assert_eq!(schema.columns[1].ordinal, 2);
        assert_eq!(schema.columns[2].ordinal, 3);
    }

    #[test]
    fn an_empty_collection_describes_as_its_id_alone() {
        // Not an error: the collection exists and holds nothing, which is a
        // real answer and a different one from "the collection is missing".
        let schema = infer_schema(&users(), &[]);
        let names: Vec<_> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec![ID_COLUMN]);
    }
}
