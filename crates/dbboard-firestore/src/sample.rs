//! Inferring a collection's shape from a sample of its documents.
//!
//! A Firestore collection declares no schema — two documents in the same
//! collection can share no field at all. So there is nothing to *read*; the
//! only honest answer is to look at some documents and report what they
//! happen to contain (ADR-0091 §4).
//!
//! That makes the wording load-bearing. `TableSchema` has no field for "this
//! is an inference", so the caveat rides in `declared_type`: every type here
//! carries the sample it came from (`string (12/20 sampled)`), which is both
//! the disclaimer and the evidence for the `nullable` flag beside it.

use std::collections::BTreeMap;

use dbboard_core::{ColumnInfo, TableInfo, TableSchema};
use serde_json::Value as JsonValue;

use crate::document::NAME_COLUMN;

/// What one field looked like across the sample.
#[derive(Default)]
struct Observed {
    /// Type names seen, deduplicated and ordered so the rendering is stable.
    types: std::collections::BTreeSet<String>,
    /// How many sampled documents carried the field at all.
    present: usize,
    /// Whether any document carried it as an explicit `nullValue`.
    explicit_null: bool,
}

/// Describe `table` from the documents in `documents`.
///
/// Infallible on purpose. A value this crate cannot name is reported as
/// `unknown` rather than failing the description: `describe_table` answers
/// "what is in here", and dropping a field — or refusing the whole answer —
/// because one document holds something odd hides exactly the thing worth
/// seeing. [`crate::document`] is where an unreadable value is refused, on the
/// path where the value is actually returned to the caller.
pub(crate) fn infer_schema(table: &TableInfo, documents: &[JsonValue]) -> TableSchema {
    let sampled = documents.len();
    let mut observed: BTreeMap<String, Observed> = BTreeMap::new();

    for document in documents {
        let Some(fields) = document.get("fields").and_then(JsonValue::as_object) else {
            continue;
        };
        for (name, value) in fields {
            let entry = observed.entry(name.clone()).or_default();
            entry.present += 1;
            let type_name = type_name(value);
            entry.explicit_null |= type_name == "null";
            entry.types.insert(type_name);
        }
    }

    let mut columns = Vec::with_capacity(observed.len() + 1);
    columns.push(ColumnInfo {
        name: NAME_COLUMN.to_string(),
        declared_type: Some("name (document path)".to_string()),
        nullable: false,
        primary_key: true,
        ordinal: 1,
        default_value: None,
    });

    for (ordinal, (name, seen)) in observed.into_iter().enumerate() {
        let types = seen.types.into_iter().collect::<Vec<_>>().join("|");
        columns.push(ColumnInfo {
            name,
            declared_type: Some(format!("{types} ({}/{sampled} sampled)", seen.present)),
            // Absent and present-but-null both mean the column can be empty.
            nullable: seen.present < sampled || seen.explicit_null,
            primary_key: false,
            // +2: 1-based, and the document path already took slot 1.
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX - 2) + 2,
            default_value: None,
        });
    }

    TableSchema {
        table: table.clone(),
        columns,
        primary_key: vec![NAME_COLUMN.to_string()],
    }
}

/// The Firestore type tag of a value, without its `Value` suffix.
///
/// Firestore wraps every scalar in a single-key object naming its type
/// (`{"stringValue": "…"}`), so the key *is* the type. Anything that is not
/// one single-key object is not a typed Firestore value at all.
fn type_name(value: &JsonValue) -> String {
    let Some(object) = value.as_object() else {
        return "unknown".to_string();
    };
    let mut keys = object.keys();
    match (keys.next(), keys.next()) {
        (Some(tag), None) => tag.strip_suffix("Value").unwrap_or(tag).to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn users() -> TableInfo {
        TableInfo::unqualified("users")
    }

    fn document(fields: &serde_json::Value) -> JsonValue {
        json!({ "name": "projects/p/databases/(default)/documents/users/a", "fields": fields })
    }

    fn column<'a>(schema: &'a TableSchema, name: &str) -> &'a dbboard_core::ColumnInfo {
        schema
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no column named {name}"))
    }

    #[test]
    fn the_document_path_comes_first_and_is_the_primary_key() {
        let schema = infer_schema(&users(), &[document(&json!({}))]);
        assert_eq!(schema.columns[0].name, NAME_COLUMN);
        assert_eq!(schema.columns[0].ordinal, 1);
        assert!(schema.columns[0].primary_key);
        assert!(!schema.columns[0].nullable);
        assert_eq!(schema.primary_key, vec![NAME_COLUMN.to_string()]);
    }

    /// A collection has no declared schema, so every column here is an
    /// observation. The type string has to say so, because a `TableSchema`
    /// has nowhere else to put the caveat and a bare `string` would read as a
    /// guarantee the database never made.
    #[test]
    fn a_type_is_reported_with_the_sample_it_was_inferred_from() {
        let schema = infer_schema(
            &users(),
            &[
                document(&json!({ "email": { "stringValue": "a@dbboard.example.com" } })),
                document(&json!({ "email": { "stringValue": "b@dbboard.example.com" } })),
            ],
        );
        assert_eq!(
            column(&schema, "email").declared_type.as_deref(),
            Some("string (2/2 sampled)")
        );
    }

    #[test]
    fn a_field_every_sampled_document_carries_is_not_nullable() {
        let schema = infer_schema(
            &users(),
            &[
                document(&json!({ "age": { "integerValue": "1" } })),
                document(&json!({ "age": { "integerValue": "2" } })),
            ],
        );
        assert!(!column(&schema, "age").nullable);
    }

    #[test]
    fn a_field_some_documents_omit_is_nullable() {
        let schema = infer_schema(
            &users(),
            &[
                document(&json!({ "nickname": { "stringValue": "kite" } })),
                document(&json!({})),
            ],
        );
        let nickname = column(&schema, "nickname");
        assert!(nickname.nullable);
        assert_eq!(
            nickname.declared_type.as_deref(),
            Some("string (1/2 sampled)"),
            "the frequency is the evidence for the nullability"
        );
    }

    /// Firestore distinguishes "absent" from "present and null". Both mean the
    /// column can be empty, so both make it nullable.
    #[test]
    fn a_field_that_is_explicitly_null_somewhere_is_nullable() {
        let schema = infer_schema(
            &users(),
            &[
                document(&json!({ "deleted_at": { "timestampValue": "2026-01-01T00:00:00Z" } })),
                document(&json!({ "deleted_at": { "nullValue": null } })),
            ],
        );
        assert!(column(&schema, "deleted_at").nullable);
    }

    /// A collection can hold documents of different shapes; hiding that would
    /// make the inference misleading in exactly the case it matters.
    #[test]
    fn a_field_with_more_than_one_type_lists_every_type_seen() {
        let schema = infer_schema(
            &users(),
            &[
                document(&json!({ "id": { "integerValue": "1" } })),
                document(&json!({ "id": { "stringValue": "two" } })),
            ],
        );
        assert_eq!(
            column(&schema, "id").declared_type.as_deref(),
            Some("integer|string (2/2 sampled)")
        );
    }

    #[test]
    fn fields_are_sorted_and_numbered_after_the_document_path() {
        let schema = infer_schema(
            &users(),
            &[document(
                &json!({ "zulu": { "booleanValue": true }, "alpha": { "booleanValue": false } }),
            )],
        );
        let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec![NAME_COLUMN, "alpha", "zulu"]);
        let ordinals: Vec<u32> = schema.columns.iter().map(|c| c.ordinal).collect();
        assert_eq!(ordinals, vec![1, 2, 3]);
    }

    #[test]
    fn nested_containers_report_their_container_type() {
        let schema = infer_schema(
            &users(),
            &[document(&json!({
                "tags": { "arrayValue": { "values": [] } },
                "profile": { "mapValue": { "fields": {} } },
            }))],
        );
        assert_eq!(
            column(&schema, "tags").declared_type.as_deref(),
            Some("array (1/1 sampled)")
        );
        assert_eq!(
            column(&schema, "profile").declared_type.as_deref(),
            Some("map (1/1 sampled)")
        );
    }

    /// Sampling is descriptive, not authoritative: a value this crate cannot
    /// name should not remove the field from the description. `query` is where
    /// an unreadable value is refused.
    #[test]
    fn a_value_that_is_not_a_typed_firestore_value_is_named_unknown_not_dropped() {
        let schema = infer_schema(&users(), &[document(&json!({ "broken": 7 }))]);
        assert_eq!(
            column(&schema, "broken").declared_type.as_deref(),
            Some("unknown (1/1 sampled)")
        );
    }

    /// An empty collection is a real answer, not a failure — but there is
    /// nothing to infer, so only the one column that always exists is
    /// reported.
    #[test]
    fn an_empty_sample_still_describes_the_document_path() {
        let schema = infer_schema(&users(), &[]);
        assert_eq!(schema.columns.len(), 1);
        assert_eq!(schema.columns[0].name, NAME_COLUMN);
        assert_eq!(schema.table, users());
    }

    #[test]
    fn a_document_with_no_fields_key_is_counted_but_contributes_nothing() {
        let schema = infer_schema(
            &users(),
            &[
                document(&json!({ "a": { "booleanValue": true } })),
                json!({ "name": "projects/p/databases/(default)/documents/users/b" }),
            ],
        );
        assert_eq!(
            column(&schema, "a").declared_type.as_deref(),
            Some("boolean (1/2 sampled)")
        );
        assert!(column(&schema, "a").nullable);
    }
}
