//! Firestore's typed value JSON, mapped onto `dbboard_core::Value`.
//!
//! Firestore does not send a document as plain JSON. Every leaf is wrapped in
//! a one-key object naming its type — `{"stringValue": "a"}`,
//! `{"integerValue": "42"}` — because JSON cannot distinguish an int64 from a
//! double, or a timestamp from the string that spells it. Two conversions come
//! out of that, and they are deliberately different:
//!
//! - [`cell`] produces the *cell* of a result grid. Scalars land in the flat
//!   `Value` variants a SQL adapter would use, so a Firestore string sorts and
//!   exports like any other string.
//! - [`plain_json`] produces the *inside* of a document. Everything is
//!   unwrapped to ordinary JSON, so a nested map reads as `{"a": 1}` rather
//!   than as `{"a": {"integerValue": "1"}}`. Showing the wrapper to a user
//!   would be showing them the transport.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use dbboard_core::{DbError, DbResult, Value};
use serde_json::{Map, Value as JsonValue};

/// The field Firestore reserves for a document's own path. Used as the first
/// column of every result so a row is identifiable even when the documents
/// share no other field.
pub(crate) const NAME_COLUMN: &str = "__name__";

/// Assemble Firestore documents into a result table.
///
/// A collection has no declared shape, so the columns are the union of the
/// top-level fields the returned documents actually carry — an observation,
/// not a schema. [`NAME_COLUMN`] comes first so a row is identifiable even
/// when the documents share no field at all; the rest are sorted by name.
///
/// Sorted rather than in the order Firestore sent them, because that order is
/// not recoverable: `serde_json`'s default `Map` is a `BTreeMap`, so the
/// fields are already alphabetical by the time they get here. Sorting the
/// whole union makes the column order independent of which document happened
/// to arrive first, instead of half-ordered in a way nobody could predict.
///
/// `documents_root` is stripped from each document's resource path, leaving
/// the part that identifies it within this connection.
///
/// # Errors
///
/// Returns [`DbError::TypeConversion`] if any field of any document fails to
/// convert. The whole result fails rather than the one cell: a blank cell
/// would read as "this document has nothing here", which is a different claim
/// from "this could not be read".
pub(crate) fn to_result(
    documents: &[JsonValue],
    documents_root: &str,
) -> DbResult<dbboard_core::QueryResult> {
    use dbboard_core::{Column, QueryResult, Row};

    let mut order: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut converted: Vec<(String, Vec<(String, Value)>)> = Vec::with_capacity(documents.len());

    for document in documents {
        let name = document
            .get("name")
            .and_then(JsonValue::as_str)
            .map(|path| relative_path(path, documents_root))
            .unwrap_or_default();

        let mut cells = Vec::new();
        if let Some(fields) = document.get("fields").and_then(JsonValue::as_object) {
            for (field, value) in fields {
                order.insert(field.clone());
                cells.push((field.clone(), cell(value)?));
            }
        }
        converted.push((name, cells));
    }

    let mut columns = Vec::with_capacity(order.len() + 1);
    columns.push(Column {
        name: NAME_COLUMN.to_owned(),
        declared_type: None,
    });
    columns.extend(order.iter().map(|name| Column {
        name: name.clone(),
        declared_type: None,
    }));

    let rows = converted
        .into_iter()
        .map(|(name, cells)| {
            let mut values = Vec::with_capacity(order.len() + 1);
            values.push(Value::Text(name));
            for field in &order {
                let found = cells
                    .iter()
                    .find(|(candidate, _)| candidate == field)
                    .map(|(_, value)| value.clone());
                // Absent field, not a null one — see `cell`'s `nullValue` arm.
                values.push(found.unwrap_or(Value::Null));
            }
            Row::new(values)
        })
        .collect();

    Ok(QueryResult {
        columns,
        rows,
        rows_affected: 0,
        ..QueryResult::empty()
    })
}

/// Strip `projects/…/databases/…/documents/` from a document's resource path.
/// Left whole if it does not carry the expected prefix, so an unexpected shape
/// is visible rather than mangled.
fn relative_path(path: &str, documents_root: &str) -> String {
    path.strip_prefix(documents_root)
        .map_or(path, |rest| rest.trim_start_matches('/'))
        .to_owned()
}

/// Convert one Firestore typed value into a result-grid cell.
///
/// # Errors
///
/// Returns [`DbError::TypeConversion`] if the value is not a one-key typed
/// object, if the type tag is one this adapter does not know, or if a payload
/// does not parse (a non-numeric `integerValue`, invalid base64 bytes).
pub(crate) fn cell(value: &JsonValue) -> DbResult<Value> {
    let (tag, payload) = type_tag(value)?;
    match tag {
        // A field that is present and null is not an absent field. The caller
        // uses `Value::Null` for absent, so the two stay distinguishable all
        // the way to the grid (ADR-0091, issue 0018).
        "nullValue" => Ok(Value::Json(JsonValue::Null)),
        // `Value` has no boolean variant. Integer(1)/Integer(0) is what the
        // SQLite-family adapters do, because SQLite has no boolean either —
        // but Firestore does, and reporting it as a number would misstate the
        // document's own type.
        "booleanValue" => Ok(Value::Json(JsonValue::Bool(as_bool(payload)?))),
        "integerValue" => Ok(Value::Integer(as_i64(payload)?)),
        "doubleValue" => Ok(Value::Real(as_f64(payload)?)),
        "stringValue" | "timestampValue" | "referenceValue" => {
            Ok(Value::Text(as_str(payload, tag)?.to_owned()))
        }
        "bytesValue" => Ok(Value::Blob(as_bytes(payload)?)),
        "geoPointValue" | "arrayValue" | "mapValue" => Ok(Value::Json(plain_json(value)?)),
        other => Err(unknown_tag(other)),
    }
}

/// Convert one Firestore typed value into ordinary JSON, recursively.
///
/// # Errors
///
/// Same conditions as [`cell`], at any depth.
pub(crate) fn plain_json(value: &JsonValue) -> DbResult<JsonValue> {
    let (tag, payload) = type_tag(value)?;
    match tag {
        "nullValue" => Ok(JsonValue::Null),
        "booleanValue" => Ok(JsonValue::Bool(as_bool(payload)?)),
        // int64 arrives as a string because JSON has no int64. It becomes a
        // JSON number here, which is what `Value::Integer` already serialises
        // to — so a Firestore integer and a SQL BIGINT reach the frontend the
        // same way, including the same >2^53 precision limit.
        "integerValue" => Ok(JsonValue::from(as_i64(payload)?)),
        "doubleValue" => Ok(JsonValue::from(as_f64(payload)?)),
        "stringValue" | "timestampValue" | "referenceValue" => {
            Ok(JsonValue::String(as_str(payload, tag)?.to_owned()))
        }
        // Base64, not a `$blob` tag. The tagged form is the *outer* wire
        // encoding of a cell; re-using it inside a document would make a
        // payload that is supposed to be opaque self-describing, which is
        // exactly what issue 0018 ruled out.
        "bytesValue" => Ok(JsonValue::String(as_str(payload, tag)?.to_owned())),
        "geoPointValue" => Ok(payload.clone()),
        "arrayValue" => {
            // An empty array is `{"arrayValue": {}}` — the `values` key is
            // omitted rather than sent empty.
            let items = payload.get("values").and_then(JsonValue::as_array);
            let converted = items
                .map(|items| items.iter().map(plain_json).collect::<DbResult<Vec<_>>>())
                .transpose()?
                .unwrap_or_default();
            Ok(JsonValue::Array(converted))
        }
        "mapValue" => {
            let fields = payload.get("fields").and_then(JsonValue::as_object);
            let mut out = Map::new();
            if let Some(fields) = fields {
                for (name, inner) in fields {
                    out.insert(name.clone(), plain_json(inner)?);
                }
            }
            Ok(JsonValue::Object(out))
        }
        other => Err(unknown_tag(other)),
    }
}

/// Split a Firestore value into its type tag and payload.
fn type_tag(value: &JsonValue) -> DbResult<(&str, &JsonValue)> {
    let object = value
        .as_object()
        .ok_or_else(|| DbError::TypeConversion("Firestore value is not an object".to_string()))?;
    let mut entries = object.iter();
    let (tag, payload) = entries
        .next()
        .ok_or_else(|| DbError::TypeConversion("Firestore value has no type tag".to_string()))?;
    if entries.next().is_some() {
        return Err(DbError::TypeConversion(format!(
            "Firestore value has {} type tags, expected 1",
            object.len()
        )));
    }
    Ok((tag.as_str(), payload))
}

fn unknown_tag(tag: &str) -> DbError {
    // Refuse rather than guess: a tag we do not know is a Firestore type whose
    // shape we have not agreed on, and silently rendering its raw JSON would
    // put transport wrappers in front of the user.
    DbError::TypeConversion(format!("unknown Firestore value type `{tag}`"))
}

fn as_bool(payload: &JsonValue) -> DbResult<bool> {
    payload
        .as_bool()
        .ok_or_else(|| DbError::TypeConversion("booleanValue is not a boolean".to_string()))
}

/// Firestore encodes int64 as a JSON *string*. Some responses (and most
/// hand-written fixtures) use a bare number instead, so both are accepted.
fn as_i64(payload: &JsonValue) -> DbResult<i64> {
    match payload {
        JsonValue::String(s) => s.parse::<i64>().map_err(|e| {
            DbError::TypeConversion(format!("integerValue `{s}` is not an integer: {e}"))
        }),
        JsonValue::Number(n) => n
            .as_i64()
            .ok_or_else(|| DbError::TypeConversion(format!("integerValue `{n}` is not an i64"))),
        _ => Err(DbError::TypeConversion(
            "integerValue is neither a string nor a number".to_string(),
        )),
    }
}

/// `doubleValue` is a JSON number, except for the three values JSON cannot
/// spell: Firestore sends those as the strings `"NaN"`, `"Infinity"`, and
/// `"-Infinity"`.
fn as_f64(payload: &JsonValue) -> DbResult<f64> {
    match payload {
        JsonValue::Number(n) => n
            .as_f64()
            .ok_or_else(|| DbError::TypeConversion(format!("doubleValue `{n}` is not a double"))),
        JsonValue::String(s) => match s.as_str() {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            other => Err(DbError::TypeConversion(format!(
                "doubleValue `{other}` is not a double"
            ))),
        },
        _ => Err(DbError::TypeConversion(
            "doubleValue is not a number".to_string(),
        )),
    }
}

fn as_str<'a>(payload: &'a JsonValue, tag: &str) -> DbResult<&'a str> {
    payload
        .as_str()
        .ok_or_else(|| DbError::TypeConversion(format!("{tag} is not a string")))
}

fn as_bytes(payload: &JsonValue) -> DbResult<Vec<u8>> {
    let encoded = as_str(payload, "bytesValue")?;
    BASE64
        .decode(encoded)
        .map_err(|e| DbError::TypeConversion(format!("bytesValue is not valid base64: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn columns_of(result: &dbboard_core::QueryResult) -> Vec<&str> {
        result.columns.iter().map(|c| c.name.as_str()).collect()
    }

    const ROOT: &str = "projects/p/databases/(default)/documents";

    /// A collection has no declared shape, so the columns are whatever the
    /// returned documents happened to carry — in first-seen order, so the
    /// same result set always produces the same table.
    #[test]
    fn columns_are_the_union_of_the_returned_documents_fields() {
        let result = to_result(
            &[
                json!({ "name": format!("{ROOT}/users/a"), "fields": {
                    "name": { "stringValue": "ada" },
                    "age": { "integerValue": "36" }
                }}),
                json!({ "name": format!("{ROOT}/users/b"), "fields": {
                    "age": { "integerValue": "9" },
                    "city": { "stringValue": "tokyo" }
                }}),
            ],
            ROOT,
        )
        .unwrap();

        assert_eq!(columns_of(&result), ["__name__", "age", "city", "name"]);
        assert_eq!(result.rows.len(), 2);
    }

    /// The absolute resource path repeats the project and database on every
    /// row — facts the connection already fixes. The relative path is what
    /// identifies the document within it.
    #[test]
    fn the_name_column_holds_the_path_relative_to_the_documents_root() {
        let result = to_result(
            &[json!({ "name": format!("{ROOT}/users/a"), "fields": {} })],
            ROOT,
        )
        .unwrap();
        assert_eq!(result.rows[0].get(0), Some(&Value::Text("users/a".into())));
    }

    /// The distinction issue 0018 preserves, at the row level: `b` has no
    /// `note` field at all, `a` has one that holds null.
    #[test]
    fn an_absent_field_is_null_and_a_present_null_is_a_document_null() {
        let result = to_result(
            &[
                json!({ "name": format!("{ROOT}/users/a"), "fields": {
                    "note": { "nullValue": null }
                }}),
                json!({ "name": format!("{ROOT}/users/b"), "fields": {} }),
            ],
            ROOT,
        )
        .unwrap();

        assert_eq!(columns_of(&result), ["__name__", "note"]);
        assert_eq!(result.rows[0].get(1), Some(&Value::Json(JsonValue::Null)));
        assert_eq!(result.rows[1].get(1), Some(&Value::Null));
    }

    #[test]
    fn a_document_with_no_fields_still_produces_a_row() {
        let result = to_result(&[json!({ "name": format!("{ROOT}/users/a") })], ROOT).unwrap();
        assert_eq!(columns_of(&result), ["__name__"]);
        assert_eq!(result.rows.len(), 1);
    }

    #[test]
    fn no_documents_means_no_rows_and_no_columns_beyond_the_name() {
        let result = to_result(&[], ROOT).unwrap();
        assert_eq!(columns_of(&result), ["__name__"]);
        assert!(result.rows.is_empty());
        assert_eq!(result.rows_affected, 0);
    }

    /// A field that fails to convert fails the query rather than becoming a
    /// blank cell: a silently-empty cell reads as "the document has nothing
    /// here", which is a different and wrong claim.
    #[test]
    fn a_field_that_cannot_be_converted_fails_the_whole_result() {
        let err = to_result(
            &[json!({ "name": format!("{ROOT}/users/a"), "fields": {
                "x": { "quantumValue": 1 }
            }})],
            ROOT,
        )
        .unwrap_err();
        assert!(err.message().contains("quantumValue"), "message: {err}");
    }

    #[test]
    fn string_becomes_text() {
        assert_eq!(
            cell(&json!({ "stringValue": "hello" })).unwrap(),
            Value::Text("hello".into())
        );
    }

    #[test]
    fn integer_arrives_as_a_string_and_becomes_an_integer() {
        assert_eq!(
            cell(&json!({ "integerValue": "42" })).unwrap(),
            Value::Integer(42)
        );
        // int64 beyond what a JSON number could carry losslessly.
        assert_eq!(
            cell(&json!({ "integerValue": "9007199254740993" })).unwrap(),
            Value::Integer(9_007_199_254_740_993)
        );
    }

    #[test]
    fn integer_also_accepts_a_bare_number() {
        assert_eq!(
            cell(&json!({ "integerValue": 7 })).unwrap(),
            Value::Integer(7)
        );
    }

    #[test]
    fn double_becomes_real() {
        assert_eq!(
            cell(&json!({ "doubleValue": 1.5 })).unwrap(),
            Value::Real(1.5)
        );
    }

    #[test]
    fn double_accepts_the_values_json_cannot_spell() {
        let Value::Real(nan) = cell(&json!({ "doubleValue": "NaN" })).unwrap() else {
            panic!("expected Real");
        };
        assert!(nan.is_nan());
        assert_eq!(
            cell(&json!({ "doubleValue": "Infinity" })).unwrap(),
            Value::Real(f64::INFINITY)
        );
        assert_eq!(
            cell(&json!({ "doubleValue": "-Infinity" })).unwrap(),
            Value::Real(f64::NEG_INFINITY)
        );
    }

    /// The distinction issue 0018 exists to preserve: a field that is present
    /// and holds null is not the same as a field that is absent. Absent is
    /// `Value::Null`, applied by the row builder; present-and-null is a
    /// document holding a JSON null.
    #[test]
    fn present_null_is_a_document_null_not_a_sql_null() {
        let value = cell(&json!({ "nullValue": null })).unwrap();
        assert_eq!(value, Value::Json(JsonValue::Null));
        assert!(!value.is_null());
    }

    /// Firestore has a real boolean type. Collapsing it to 1/0 the way the
    /// SQLite-family adapters must would report the wrong type for a value
    /// the document is explicit about.
    #[test]
    fn boolean_stays_a_boolean() {
        assert_eq!(
            cell(&json!({ "booleanValue": true })).unwrap(),
            Value::Json(json!(true))
        );
        assert_eq!(
            cell(&json!({ "booleanValue": false })).unwrap(),
            Value::Json(json!(false))
        );
    }

    #[test]
    fn timestamp_and_reference_stay_text() {
        assert_eq!(
            cell(&json!({ "timestampValue": "2026-08-05T12:00:00Z" })).unwrap(),
            Value::Text("2026-08-05T12:00:00Z".into())
        );
        assert_eq!(
            cell(&json!({ "referenceValue": "projects/p/databases/(default)/documents/users/1" }))
                .unwrap(),
            Value::Text("projects/p/databases/(default)/documents/users/1".into())
        );
    }

    #[test]
    fn bytes_are_decoded_into_a_blob() {
        // "hi" in base64.
        assert_eq!(
            cell(&json!({ "bytesValue": "aGk=" })).unwrap(),
            Value::Blob(b"hi".to_vec())
        );
    }

    #[test]
    fn invalid_base64_is_refused() {
        let err = cell(&json!({ "bytesValue": "not base64!" })).unwrap_err();
        assert!(err.message().contains("base64"), "message: {err}");
    }

    /// A nested map reaches the grid as ordinary JSON. If the type wrappers
    /// leaked through, the user would be reading the transport instead of
    /// their data.
    #[test]
    fn map_is_unwrapped_into_plain_json() {
        let value = cell(&json!({
            "mapValue": {
                "fields": {
                    "name": { "stringValue": "ada" },
                    "age": { "integerValue": "36" },
                    "active": { "booleanValue": true },
                    "note": { "nullValue": null }
                }
            }
        }))
        .unwrap();
        assert_eq!(
            value,
            Value::Json(json!({ "name": "ada", "age": 36, "active": true, "note": null }))
        );
    }

    #[test]
    fn array_is_unwrapped_and_nests() {
        let value = cell(&json!({
            "arrayValue": {
                "values": [
                    { "integerValue": "1" },
                    { "arrayValue": { "values": [{ "stringValue": "deep" }] } },
                    { "mapValue": { "fields": { "k": { "doubleValue": 0.5 } } } }
                ]
            }
        }))
        .unwrap();
        assert_eq!(value, Value::Json(json!([1, ["deep"], { "k": 0.5 }])));
    }

    /// Firestore omits `values` / `fields` entirely when the collection is
    /// empty rather than sending an empty one.
    #[test]
    fn empty_array_and_map_omit_their_payload_key() {
        assert_eq!(
            cell(&json!({ "arrayValue": {} })).unwrap(),
            Value::Json(json!([]))
        );
        assert_eq!(
            cell(&json!({ "mapValue": {} })).unwrap(),
            Value::Json(json!({}))
        );
    }

    #[test]
    fn geo_point_keeps_its_two_fields() {
        assert_eq!(
            cell(&json!({ "geoPointValue": { "latitude": 35.68, "longitude": 139.76 } })).unwrap(),
            Value::Json(json!({ "latitude": 35.68, "longitude": 139.76 }))
        );
    }

    /// Nested bytes cannot stay bytes — JSON has no byte type. They keep the
    /// base64 Firestore sent, and deliberately do *not* get a `$blob` wrapper:
    /// the payload of a `$json` cell is opaque, so a `$blob` key inside it
    /// would be read as literal document content, not as an encoding.
    #[test]
    fn nested_bytes_stay_base64_without_a_blob_tag() {
        let value = cell(&json!({
            "mapValue": { "fields": { "data": { "bytesValue": "aGk=" } } }
        }))
        .unwrap();
        assert_eq!(value, Value::Json(json!({ "data": "aGk=" })));
    }

    #[test]
    fn an_unknown_type_tag_is_refused_rather_than_guessed() {
        let err = cell(&json!({ "quantumValue": 1 })).unwrap_err();
        assert!(err.message().contains("quantumValue"), "message: {err}");
    }

    #[test]
    fn a_value_with_two_tags_is_refused() {
        let err = cell(&json!({ "stringValue": "a", "integerValue": "1" })).unwrap_err();
        assert!(err.message().contains("expected 1"), "message: {err}");
    }

    #[test]
    fn a_bare_scalar_is_not_a_firestore_value() {
        let err = cell(&json!("bare")).unwrap_err();
        assert!(err.message().contains("not an object"), "message: {err}");
    }

    #[test]
    fn an_empty_object_has_no_type_tag() {
        let err = cell(&json!({})).unwrap_err();
        assert!(err.message().contains("no type tag"), "message: {err}");
    }

    #[test]
    fn a_non_numeric_integer_is_refused() {
        let err = cell(&json!({ "integerValue": "twelve" })).unwrap_err();
        assert!(err.message().contains("twelve"), "message: {err}");
    }
}
