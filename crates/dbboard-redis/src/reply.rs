//! Turning a RESP reply into rows and columns.
//!
//! The [`Shape`] the classifier chose decides the columns, so `HGETALL`
//! comes back as `field | value` and `ZRANGE … WITHSCORES` as
//! `member | score` instead of one long column of alternating things. The
//! replies are read the way RESP2 sends them, and also the way RESP3 does
//! (maps, pairs), so a server that answers in either reads the same.

use dbboard_core::{Column, DbError, DbResult, QueryResult, Row, Value as Cell};
use redis::Value;

use crate::read_only::Shape;

/// Rows for `reply`, laid out by `shape`. `next_cursor` is passed in for the
/// scan shapes, whose replies the adapter has already followed.
///
/// # Errors
///
/// [`DbError::TypeConversion`] when the reply does not have the layout the
/// command promises — a proxy that answered something else, say.
pub fn to_result(shape: Shape, reply: Value) -> DbResult<QueryResult> {
    match shape {
        Shape::Value => Ok(table(&["value"], vec![vec![cell(reply)]])),
        Shape::Values => Ok(table(
            &["value"],
            items(reply)?.into_iter().map(|v| vec![cell(v)]).collect(),
        )),
        Shape::FieldValues => Ok(table(&["field", "value"], pairs(reply)?)),
        Shape::MemberScores => Ok(table(&["member", "score"], pairs(reply)?)),
        Shape::Entries => entries(reply),
        Shape::Info => Ok(info(&text(reply))),
        Shape::ScanKeys | Shape::ScanMembers | Shape::ScanFieldValues | Shape::ScanMemberScores => {
            Err(DbError::TypeConversion(
                "a scan reply is read page by page".into(),
            ))
        }
    }
}

/// One page of a scan reply: the next cursor and its items.
///
/// # Errors
///
/// [`DbError::TypeConversion`] when the reply is not `[cursor, [items…]]`.
pub fn scan_page(reply: Value) -> DbResult<(String, Vec<Value>)> {
    let mut parts = items(reply)?.into_iter();
    match (parts.next(), parts.next(), parts.next()) {
        (Some(cursor), Some(page), None) => Ok((text(cursor), items(page)?)),
        _ => Err(shape_error("a scan reply")),
    }
}

/// The rows a scan collected, with the cursor to continue from on each row
/// (`0` when the walk is complete), so the next page is one copy away.
#[must_use]
pub fn scan_result(shape: Shape, collected: Vec<Value>, next_cursor: &str) -> QueryResult {
    let cursor = Cell::Text(next_cursor.to_owned());
    let mut rows: Vec<Vec<Cell>> = match shape {
        Shape::ScanFieldValues | Shape::ScanMemberScores => pairs_of(collected)
            .into_iter()
            .map(|mut r| {
                r.push(cursor.clone());
                r
            })
            .collect(),
        _ => collected
            .into_iter()
            .map(|v| vec![cell(v), cursor.clone()])
            .collect(),
    };
    let names: &[&str] = match shape {
        Shape::ScanKeys => &["key", "next_cursor"],
        Shape::ScanFieldValues => &["field", "value", "next_cursor"],
        Shape::ScanMemberScores => &["member", "score", "next_cursor"],
        _ => &["member", "next_cursor"],
    };
    // An empty page is still an answer: without a row the cursor to go on
    // from would be lost, and an empty page mid-walk is common with MATCH.
    if rows.is_empty() && next_cursor != "0" {
        let mut row = vec![Cell::Null; names.len() - 1];
        row.push(cursor);
        rows.push(row);
    }
    table(names, rows)
}

/// The number a size check answered.
///
/// # Errors
///
/// [`DbError::TypeConversion`] for anything but an integer.
pub fn integer(reply: &Value) -> DbResult<i64> {
    match reply {
        Value::Int(n) => Ok(*n),
        _ => Err(shape_error("a length")),
    }
}

/// A reply as one cell.
#[must_use]
pub fn cell(value: Value) -> Cell {
    match value {
        Value::Nil => Cell::Null,
        Value::Int(n) => Cell::Integer(n),
        Value::Double(d) => Cell::Real(d),
        Value::Boolean(b) => Cell::Integer(i64::from(b)),
        Value::Okay => Cell::Text("OK".into()),
        Value::SimpleString(s) => Cell::Text(s),
        Value::VerbatimString { text, .. } => Cell::Text(text),
        Value::BulkString(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Cell::Text(s),
            Err(e) => Cell::Blob(e.into_bytes()),
        },
        other @ (Value::Array(_) | Value::Set(_) | Value::Map(_)) => Cell::Json(json(other)),
        other => Cell::Text(format!("{other:?}")),
    }
}

fn json(value: Value) -> serde_json::Value {
    use serde_json::Value as J;
    match value {
        Value::Nil => J::Null,
        Value::Int(n) => J::from(n),
        Value::Double(d) => J::from(d),
        Value::Boolean(b) => J::Bool(b),
        Value::Array(vs) | Value::Set(vs) => J::Array(vs.into_iter().map(json).collect()),
        Value::Map(kvs) => J::Object(kvs.into_iter().map(|(k, v)| (text(k), json(v))).collect()),
        other => J::String(text(other)),
    }
}

fn text(value: Value) -> String {
    match cell(value) {
        Cell::Text(s) => s,
        Cell::Integer(n) => n.to_string(),
        Cell::Real(r) => r.to_string(),
        Cell::Null => String::new(),
        Cell::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
        Cell::Json(j) => j.to_string(),
    }
}

fn items(reply: Value) -> DbResult<Vec<Value>> {
    match reply {
        Value::Array(vs) | Value::Set(vs) => Ok(vs),
        Value::Nil => Ok(Vec::new()),
        _ => Err(shape_error("a list")),
    }
}

/// Field-value or member-score pairs, from a RESP3 map, a list of pairs, or
/// the flat alternating list RESP2 sends.
fn pairs(reply: Value) -> DbResult<Vec<Vec<Cell>>> {
    match reply {
        Value::Map(kvs) => Ok(kvs
            .into_iter()
            .map(|(k, v)| vec![cell(k), cell(v)])
            .collect()),
        other => Ok(pairs_of(items(other)?)),
    }
}

fn pairs_of(values: Vec<Value>) -> Vec<Vec<Cell>> {
    if values
        .iter()
        .all(|v| matches!(v, Value::Array(p) if p.len() == 2))
        && !values.is_empty()
    {
        return values
            .into_iter()
            .filter_map(|v| match v {
                Value::Array(p) => {
                    let mut p = p.into_iter();
                    Some(vec![cell(p.next()?), cell(p.next()?)])
                }
                _ => None,
            })
            .collect();
    }
    let mut rows = Vec::with_capacity(values.len() / 2);
    let mut it = values.into_iter();
    while let Some(first) = it.next() {
        rows.push(vec![cell(first), it.next().map_or(Cell::Null, cell)]);
    }
    rows
}

/// Stream entries: `[[id, [field, value, …]], …]` as `id | fields`, the
/// fields as one document so an entry stays one row.
fn entries(reply: Value) -> DbResult<QueryResult> {
    let mut rows = Vec::new();
    for entry in items(reply)? {
        let mut parts = items(entry)?.into_iter();
        let (Some(id), Some(fields)) = (parts.next(), parts.next()) else {
            return Err(shape_error("a stream entry"));
        };
        let object = pairs_of(items(fields)?)
            .into_iter()
            .map(|pair| {
                let mut pair = pair.into_iter();
                let name = match pair.next() {
                    Some(Cell::Text(s)) => s,
                    other => format!("{other:?}"),
                };
                let value = match pair.next() {
                    Some(Cell::Text(s)) => serde_json::Value::String(s),
                    Some(Cell::Integer(n)) => serde_json::Value::from(n),
                    _ => serde_json::Value::Null,
                };
                (name, value)
            })
            .collect();
        rows.push(vec![
            cell(id),
            Cell::Json(serde_json::Value::Object(object)),
        ]);
    }
    Ok(table(&["id", "fields"], rows))
}

/// `INFO` text as `section | name | value`.
fn info(body: &str) -> QueryResult {
    let mut section = String::new();
    let mut rows = Vec::new();
    for line in body.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('#') {
            name.trim().clone_into(&mut section);
        } else if let Some((name, value)) = line.split_once(':') {
            rows.push(vec![
                Cell::Text(section.clone()),
                Cell::Text(name.to_owned()),
                Cell::Text(value.to_owned()),
            ]);
        }
    }
    table(&["section", "name", "value"], rows)
}

fn table(names: &[&str], rows: Vec<Vec<Cell>>) -> QueryResult {
    QueryResult {
        columns: names
            .iter()
            .map(|n| Column {
                name: (*n).to_owned(),
                declared_type: None,
            })
            .collect(),
        rows: rows.into_iter().map(Row::new).collect(),
        rows_affected: 0,
        has_more: false,
        next_cursor: None,
    }
}

fn shape_error(what: &str) -> DbError {
    DbError::TypeConversion(format!("the server's reply was not {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulk(s: &str) -> Value {
        Value::BulkString(s.as_bytes().to_vec())
    }

    fn rows(result: &QueryResult) -> Vec<Vec<Cell>> {
        result.rows.iter().map(|r| r.values().to_vec()).collect()
    }

    fn names(result: &QueryResult) -> Vec<&str> {
        result.columns.iter().map(|c| c.name.as_str()).collect()
    }

    fn t(s: &str) -> Cell {
        Cell::Text(s.into())
    }

    #[test]
    fn a_single_value_is_one_cell() {
        let r = to_result(Shape::Value, bulk("hello")).unwrap();
        assert_eq!(names(&r), ["value"]);
        assert_eq!(rows(&r), [[t("hello")]]);
        assert_eq!(
            rows(&to_result(Shape::Value, Value::Nil).unwrap()),
            [[Cell::Null]]
        );
        assert_eq!(
            rows(&to_result(Shape::Value, Value::Int(-2)).unwrap()),
            [[Cell::Integer(-2)]]
        );
    }

    #[test]
    fn bytes_that_are_not_text_stay_bytes() {
        assert_eq!(
            cell(Value::BulkString(vec![0xff, 0x00])),
            Cell::Blob(vec![0xff, 0x00])
        );
    }

    #[test]
    fn a_hash_reads_as_fields_and_values_in_either_protocol() {
        let resp2 = Value::Array(vec![bulk("name"), bulk("Mug"), bulk("qty"), bulk("3")]);
        let resp3 = Value::Map(vec![(bulk("name"), bulk("Mug")), (bulk("qty"), bulk("3"))]);
        for reply in [resp2, resp3] {
            let r = to_result(Shape::FieldValues, reply).unwrap();
            assert_eq!(names(&r), ["field", "value"]);
            assert_eq!(rows(&r), [[t("name"), t("Mug")], [t("qty"), t("3")]]);
        }
    }

    #[test]
    fn scores_read_as_pairs_in_either_protocol() {
        let resp2 = Value::Array(vec![bulk("a"), bulk("1.5")]);
        let resp3 = Value::Array(vec![Value::Array(vec![bulk("a"), Value::Double(1.5)])]);
        assert_eq!(
            rows(&to_result(Shape::MemberScores, resp2).unwrap()),
            [[t("a"), t("1.5")]]
        );
        assert_eq!(
            rows(&to_result(Shape::MemberScores, resp3).unwrap()),
            [[t("a"), Cell::Real(1.5)]]
        );
    }

    #[test]
    fn a_scan_page_splits_into_cursor_and_items() {
        let reply = Value::Array(vec![bulk("17"), Value::Array(vec![bulk("a"), bulk("b")])]);
        let (cursor, page) = scan_page(reply).unwrap();
        assert_eq!(cursor, "17");
        assert_eq!(page.len(), 2);
        assert!(scan_page(bulk("nope")).is_err());
    }

    #[test]
    fn scanned_keys_carry_the_cursor_to_go_on_from() {
        let r = scan_result(Shape::ScanKeys, vec![bulk("a"), bulk("b")], "0");
        assert_eq!(names(&r), ["key", "next_cursor"]);
        assert_eq!(rows(&r), [[t("a"), t("0")], [t("b"), t("0")]]);
    }

    #[test]
    fn an_empty_page_mid_walk_still_says_where_to_go_on() {
        let r = scan_result(Shape::ScanKeys, Vec::new(), "42");
        assert_eq!(rows(&r), [[Cell::Null, t("42")]]);
        // At the end of the walk there is nothing to say.
        assert!(scan_result(Shape::ScanKeys, Vec::new(), "0")
            .rows
            .is_empty());
    }

    #[test]
    fn a_hash_scan_pairs_its_fields() {
        let r = scan_result(Shape::ScanFieldValues, vec![bulk("f"), bulk("v")], "0");
        assert_eq!(names(&r), ["field", "value", "next_cursor"]);
        assert_eq!(rows(&r), [[t("f"), t("v"), t("0")]]);
    }

    #[test]
    fn stream_entries_keep_one_row_per_entry() {
        let reply = Value::Array(vec![Value::Array(vec![
            bulk("1-0"),
            Value::Array(vec![
                bulk("event"),
                bulk("paid"),
                bulk("amount"),
                bulk("12"),
            ]),
        ])]);
        let r = to_result(Shape::Entries, reply).unwrap();
        assert_eq!(names(&r), ["id", "fields"]);
        assert_eq!(
            rows(&r),
            [[
                t("1-0"),
                Cell::Json(serde_json::json!({"event": "paid", "amount": "12"}))
            ]]
        );
    }

    #[test]
    fn info_reads_as_section_name_value() {
        let body = "# Server\r\nredis_version:7.2.0\r\n\r\n# Keyspace\r\ndb0:keys=3,expires=0\r\n";
        let r = to_result(Shape::Info, bulk(body)).unwrap();
        assert_eq!(
            rows(&r),
            [
                [t("Server"), t("redis_version"), t("7.2.0")],
                [t("Keyspace"), t("db0"), t("keys=3,expires=0")]
            ]
        );
    }

    #[test]
    fn a_reply_of_the_wrong_layout_is_an_error_not_a_guess() {
        assert!(to_result(Shape::Values, bulk("x")).is_err());
        assert!(integer(&bulk("3")).is_err());
        assert_eq!(integer(&Value::Int(3)).unwrap(), 3);
    }
}
