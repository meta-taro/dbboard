//! BSON, mapped onto `dbboard_core::Value`.
//!
//! Unlike Firestore's typed-JSON wire form, BSON arrives already typed, so
//! nothing here can fail: every `Bson` has a rendering. The two conversions
//! are still deliberately different, for the same reason they are there:
//!
//! - [`cell`] produces the *cell* of a result grid. Scalars land in the flat
//!   `Value` variants a SQL adapter would use, so a `MongoDB` string sorts and
//!   exports like any other string.
//! - [`plain_json`] produces the *inside* of a document. A nested map reads as
//!   `{"a": 1}` rather than as `{"a": {"$numberInt": "1"}}`, because showing
//!   the extended-JSON wrapper to a user would be showing them the transport
//!   (issue 0018).
//!
//! Values with no plain rendering — a regex, a min-key, a server timestamp —
//! keep their canonical extended JSON. Flattening those to a string would
//! lose which of them it was, and there is no honest scalar to flatten them
//! to.

use std::collections::BTreeSet;

use dbboard_core::{Column, QueryResult, Row, Value};
use mongodb::bson::{Bson, Document};
use serde_json::Value as JsonValue;

/// The field `MongoDB` gives every document. Used as the first column of every
/// result so a row is identifiable even when the documents share no other
/// field.
pub(crate) const ID_COLUMN: &str = "_id";

/// Keys the server adds to a command reply that describe the *reply*, not the
/// answer. Dropped from a plain command's single row for the same reason
/// Firestore's `:runQuery` bookkeeping entries are skipped: they are
/// transport, and showing them as data invites reading them as data.
const REPLY_BOOKKEEPING: &[&str] = &["ok", "$clusterTime", "operationTime", "$db"];

/// Assemble documents into a result table.
///
/// A collection has no declared shape, so the columns are the union of the
/// top-level fields the returned documents actually carry — an observation,
/// not a schema. [`ID_COLUMN`] comes first so a row is identifiable even when
/// the documents share no other field; the rest are sorted by name, so the
/// column order does not depend on which document happened to arrive first.
///
/// A field a document does not carry is [`Value::Null`]. A field it carries as
/// BSON null is `Json(null)`, and the two stay distinguishable all the way to
/// the grid (ADR-0091, issue 0018).
pub(crate) fn to_result(documents: &[Document]) -> QueryResult {
    let mut order: BTreeSet<&str> = BTreeSet::new();
    for document in documents {
        order.extend(
            document
                .keys()
                .map(String::as_str)
                .filter(|k| *k != ID_COLUMN),
        );
    }

    let carries_id = documents.iter().any(|d| d.contains_key(ID_COLUMN));
    let mut columns = Vec::with_capacity(order.len() + 1);
    if carries_id {
        columns.push(Column {
            name: ID_COLUMN.to_owned(),
            declared_type: None,
        });
    }
    columns.extend(order.iter().map(|name| Column {
        name: (*name).to_owned(),
        declared_type: None,
    }));

    let rows = documents
        .iter()
        .map(|document| {
            let mut values = Vec::with_capacity(columns.len());
            if carries_id {
                values.push(document.get(ID_COLUMN).map_or(Value::Null, cell));
            }
            values.extend(
                order
                    .iter()
                    .map(|name| document.get(*name).map_or(Value::Null, cell)),
            );
            Row::new(values)
        })
        .collect::<Vec<_>>();

    QueryResult {
        rows_affected: 0,
        columns,
        rows,
        ..QueryResult::empty()
    }
}

/// Present a command reply that is not a cursor — a `count`, a `distinct` —
/// as the single row it is.
pub(crate) fn reply_to_result(reply: &Document) -> QueryResult {
    let answer: Document = reply
        .iter()
        .filter(|(key, _)| !REPLY_BOOKKEEPING.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    to_result(std::slice::from_ref(&answer))
}

/// Convert one BSON value into a result-grid cell.
pub(crate) fn cell(value: &Bson) -> Value {
    match value {
        // Present-and-null is not an absent field; `to_result` uses
        // `Value::Null` for absent.
        Bson::Null => Value::Json(JsonValue::Null),
        // `Value` has no boolean variant, and reporting a BSON bool as 1/0
        // would misstate the document's own type.
        Bson::Boolean(b) => Value::Json(JsonValue::Bool(*b)),
        Bson::Int32(n) => Value::Integer(i64::from(*n)),
        Bson::Int64(n) => Value::Integer(*n),
        Bson::Double(x) => Value::Real(*x),
        Bson::String(s) => Value::Text(s.clone()),
        // The hex is what a user copies to identify a document. Pasting it
        // back into a filter needs `{"$oid": "…"}`, which the command parse
        // accepts — see [`crate::command`].
        Bson::ObjectId(id) => Value::Text(id.to_hex()),
        Bson::DateTime(at) => Value::Text(timestamp(*at)),
        Bson::Decimal128(d) => Value::Text(d.to_string()),
        Bson::Binary(b) => Value::Blob(b.bytes.clone()),
        other => Value::Json(plain_json(other)),
    }
}

/// Convert one BSON value into ordinary JSON, recursively.
pub(crate) fn plain_json(value: &Bson) -> JsonValue {
    match value {
        Bson::Null => JsonValue::Null,
        Bson::Boolean(b) => JsonValue::Bool(*b),
        Bson::Int32(n) => JsonValue::from(*n),
        Bson::Int64(n) => JsonValue::from(*n),
        Bson::Double(x) => JsonValue::from(*x),
        Bson::String(s) => JsonValue::String(s.clone()),
        Bson::ObjectId(id) => JsonValue::String(id.to_hex()),
        Bson::DateTime(at) => JsonValue::String(timestamp(*at)),
        Bson::Decimal128(d) => JsonValue::String(d.to_string()),
        // Base64, not a `$binary` tag: the tagged form is the outer wire
        // encoding, and re-using it inside a document would make a payload
        // that is supposed to be opaque self-describing (issue 0018).
        Bson::Binary(b) => JsonValue::String(base64(&b.bytes)),
        Bson::Array(items) => JsonValue::Array(items.iter().map(plain_json).collect()),
        Bson::Document(fields) => JsonValue::Object(
            fields
                .iter()
                .map(|(key, nested)| (key.clone(), plain_json(nested)))
                .collect(),
        ),
        // A regex, a min-key, a server timestamp. There is no scalar these
        // flatten to without losing which one they were, so they keep the
        // extended JSON that names them.
        other => other.clone().into_relaxed_extjson(),
    }
}

/// RFC 3339, falling back to the debug form for a date outside the range
/// RFC 3339 can spell — `MongoDB`'s epoch milliseconds reach further than the
/// year 9999.
fn timestamp(at: mongodb::bson::DateTime) -> String {
    at.try_to_rfc3339_string()
        .unwrap_or_else(|_| at.to_string())
}

/// Base64, hand-rolled rather than pulling `base64` in for one call site.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let packed = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(packed >> shift) as usize & 0x3f]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId, spec::BinarySubtype, Binary, DateTime, Regex};

    fn only_cell(documents: &[Document], column: &str) -> Value {
        let result = to_result(documents);
        let index = result
            .columns
            .iter()
            .position(|c| c.name == column)
            .unwrap_or_else(|| panic!("no column named {column}"));
        result.rows[0].get(index).unwrap().clone()
    }

    fn cell_of(value: Bson) -> Value {
        only_cell(&[doc! { "field": value }], "field")
    }

    // --- shape ------------------------------------------------------------

    #[test]
    fn the_id_is_the_first_column() {
        let result = to_result(&[doc! { "name": "a", "_id": 1 }]);
        assert_eq!(result.columns[0].name, ID_COLUMN);
        assert_eq!(result.columns[1].name, "name");
    }

    #[test]
    fn the_columns_are_the_union_of_the_documents_fields() {
        let result = to_result(&[doc! { "_id": 1, "b": 1 }, doc! { "_id": 2, "a": 1 }]);
        let names: Vec<_> = result.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["_id", "a", "b"]);
    }

    #[test]
    fn a_field_a_document_does_not_carry_is_null() {
        let documents = [doc! { "_id": 1, "a": 1 }, doc! { "_id": 2 }];
        let result = to_result(&documents);
        let a = result.columns.iter().position(|c| c.name == "a").unwrap();
        assert_eq!(result.rows[1].get(a), Some(&Value::Null));
    }

    #[test]
    fn a_present_null_is_not_an_absent_field() {
        assert_eq!(cell_of(Bson::Null), Value::Json(JsonValue::Null));
    }

    #[test]
    fn documents_without_an_id_get_no_id_column() {
        let result = to_result(&[doc! { "n": 1 }]);
        let names: Vec<_> = result.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["n"]);
    }

    #[test]
    fn an_empty_result_has_no_columns_and_no_rows() {
        let result = to_result(&[]);
        assert!(result.columns.is_empty());
        assert!(result.rows.is_empty());
    }

    #[test]
    fn a_result_reports_no_rows_affected() {
        // Nothing here writes, so the count is not "unknown", it is zero.
        assert_eq!(to_result(&[doc! { "_id": 1 }]).rows_affected, 0);
    }

    // --- scalars ----------------------------------------------------------

    #[test]
    fn integers_are_flat_whichever_width_they_arrived_in() {
        assert_eq!(cell_of(Bson::Int32(7)), Value::Integer(7));
        assert_eq!(cell_of(Bson::Int64(1 << 40)), Value::Integer(1 << 40));
    }

    #[test]
    fn a_double_is_real_and_a_string_is_text() {
        assert_eq!(cell_of(Bson::Double(1.5)), Value::Real(1.5));
        assert_eq!(
            cell_of(Bson::String("hello".into())),
            Value::Text("hello".into())
        );
    }

    #[test]
    fn a_boolean_stays_a_boolean() {
        assert_eq!(
            cell_of(Bson::Boolean(true)),
            Value::Json(JsonValue::Bool(true))
        );
    }

    #[test]
    fn an_object_id_reads_as_its_hex() {
        let id = ObjectId::parse_str("507f1f77bcf86cd799439011").unwrap();
        assert_eq!(
            cell_of(Bson::ObjectId(id)),
            Value::Text("507f1f77bcf86cd799439011".into())
        );
    }

    #[test]
    fn a_date_reads_as_rfc_3339() {
        let at = DateTime::from_millis(0);
        assert_eq!(
            cell_of(Bson::DateTime(at)),
            Value::Text("1970-01-01T00:00:00Z".into())
        );
    }

    #[test]
    fn binary_data_becomes_a_blob() {
        let binary = Binary {
            subtype: BinarySubtype::Generic,
            bytes: vec![1, 2, 3],
        };
        assert_eq!(cell_of(Bson::Binary(binary)), Value::Blob(vec![1, 2, 3]));
    }

    // --- trees ------------------------------------------------------------

    #[test]
    fn a_nested_document_arrives_as_json() {
        let cell = cell_of(Bson::Document(doc! { "city": "Tokyo", "zip": 100 }));
        assert_eq!(
            cell,
            Value::Json(serde_json::json!({ "city": "Tokyo", "zip": 100 }))
        );
    }

    #[test]
    fn an_array_arrives_as_json() {
        let cell = cell_of(Bson::Array(vec![Bson::Int32(1), Bson::String("a".into())]));
        assert_eq!(cell, Value::Json(serde_json::json!([1, "a"])));
    }

    #[test]
    fn a_nested_object_id_is_hex_rather_than_its_wrapper() {
        let id = ObjectId::parse_str("507f1f77bcf86cd799439011").unwrap();
        let cell = cell_of(Bson::Document(doc! { "ref": id }));
        assert_eq!(
            cell,
            Value::Json(serde_json::json!({ "ref": "507f1f77bcf86cd799439011" }))
        );
    }

    #[test]
    fn nested_binary_is_base64_rather_than_a_tagged_object() {
        let binary = Binary {
            subtype: BinarySubtype::Generic,
            bytes: b"hello".to_vec(),
        };
        let cell = cell_of(Bson::Document(doc! { "blob": binary }));
        assert_eq!(cell, Value::Json(serde_json::json!({ "blob": "aGVsbG8=" })));
    }

    #[test]
    fn a_value_with_no_plain_rendering_keeps_its_extended_json() {
        let regex = Bson::RegularExpression(Regex {
            pattern: "^a".into(),
            options: "i".into(),
        });
        let Value::Json(json) = cell_of(regex) else {
            panic!("a regex has no flat rendering, so it should have stayed JSON");
        };
        assert!(
            json.get("$regularExpression").is_some(),
            "the extended JSON should still name the type: {json}"
        );
    }

    // --- plain command replies --------------------------------------------

    #[test]
    fn a_reply_drops_the_servers_bookkeeping() {
        let reply = doc! { "n": 3, "ok": 1.0, "operationTime": 12_i64 };
        let result = reply_to_result(&reply);
        let names: Vec<_> = result.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["n"]);
        assert_eq!(result.rows[0].get(0), Some(&Value::Integer(3)));
    }

    #[test]
    fn a_reply_is_one_row() {
        let reply = doc! { "values": ["a", "b"], "ok": 1.0 };
        assert_eq!(reply_to_result(&reply).rows.len(), 1);
    }
}
