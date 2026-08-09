//! Turning approved command text into the document that goes on the wire.
//!
//! The classifier in [`crate::read_only`] reads the command with `serde_json`,
//! on purpose: it is pure, driver-free, and reviewable on its own terms. What
//! actually travels to the server is BSON, and getting there needs two things
//! `serde_json::Value` cannot give:
//!
//! - **Field order at every depth.** `serde_json`'s map is sorted, and a
//!   `{"sort": {"a": 1, "b": -1}}` that arrives as `{"b": -1, "a": 1}` sorts by
//!   a different key. `bson::Document` keeps insertion order, and its
//!   `Deserialize` fills it in the order the text is read.
//! - **Extended JSON.** `{"_id": {"$oid": "…"}}` has to become an `ObjectId`,
//!   or a query by id matches nothing. The same deserializer does that.
//!
//! So the text is parsed twice, by the same parser into two different types.
//! That is sound in the direction that matters: `serde_json` and `bson` both
//! keep the *last* of a duplicated key, and the classifier walks every key it
//! sees — including both halves of a duplicate, which it holds in a `Vec`. So
//! everything the server is asked to run has been through the classifier.

use dbboard_core::{DbError, DbResult};
use mongodb::bson::{doc, Document};

use crate::read_only::ReadCommand;

/// Parse approved command text into the document to send.
///
/// `command` is what the classifier made of the same text; it decides whether
/// the reply is a cursor, and a cursor command that names no `cursor` option
/// gets an empty one — `aggregate` and `listCollections` are errors without it,
/// and the caller asking for their rows plainly meant to receive them.
///
/// # Errors
///
/// Returns [`DbError::Query`] if the text is not a document. The classifier
/// has already refused anything that is not, so this is the second half of a
/// belt and braces rather than the check itself.
pub(crate) fn to_wire(text: &str, command: ReadCommand) -> DbResult<Document> {
    // The parse error is discarded rather than reported: it quotes the input,
    // and the input is caller-controlled (ADR-0095 §6).
    let mut document: Document = serde_json::from_str(text)
        .map_err(|_| DbError::Query("the command is not a JSON document".to_owned()))?;

    // `find` is excluded without naming it: it streams, but `cursor` is not one
    // of its options, and sending one would make the server refuse the command.
    if command.returns_cursor()
        && command.allows_option("cursor")
        && !document.contains_key("cursor")
    {
        document.insert("cursor", doc! {});
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::Bson;

    fn wire(text: &str, command: ReadCommand) -> Document {
        to_wire(text, command).expect("the text should have parsed")
    }

    #[test]
    fn the_command_name_stays_first() {
        let document = wire(r#"{"find": "users", "limit": 1}"#, ReadCommand::Find);
        assert_eq!(document.keys().next().map(String::as_str), Some("find"));
    }

    #[test]
    fn a_nested_document_keeps_the_order_it_was_written_in() {
        // Not cosmetic: `{"a": 1, "b": -1}` and `{"b": -1, "a": 1}` are
        // different sorts, and serde_json's map would have sorted this one.
        let document = wire(
            r#"{"find": "users", "sort": {"b": -1, "a": 1}}"#,
            ReadCommand::Find,
        );
        let sort = document.get_document("sort").unwrap();
        let keys: Vec<_> = sort.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["b", "a"]);
    }

    #[test]
    fn a_pipeline_stage_keeps_its_order_too() {
        let document = wire(
            r#"{"aggregate": "users", "pipeline": [{"$sort": {"b": -1, "a": 1}}]}"#,
            ReadCommand::Aggregate,
        );
        let stage = document
            .get_array("pipeline")
            .unwrap()
            .first()
            .and_then(Bson::as_document)
            .unwrap();
        let keys: Vec<_> = stage
            .get_document("$sort")
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["b", "a"]);
    }

    #[test]
    fn extended_json_becomes_the_type_it_names() {
        let document = wire(
            r#"{"find": "users", "filter": {"_id": {"$oid": "507f1f77bcf86cd799439011"}}}"#,
            ReadCommand::Find,
        );
        let id = document.get_document("filter").unwrap().get("_id").unwrap();
        assert!(
            matches!(id, Bson::ObjectId(_)),
            "an $oid should have become an ObjectId, not a subdocument: {id:?}"
        );
    }

    #[test]
    fn a_cursor_command_without_a_cursor_option_gets_one() {
        let document = wire(
            r#"{"aggregate": "users", "pipeline": []}"#,
            ReadCommand::Aggregate,
        );
        assert_eq!(document.get("cursor"), Some(&Bson::Document(doc! {})));
    }

    #[test]
    fn a_cursor_option_the_caller_wrote_is_left_alone() {
        let document = wire(
            r#"{"aggregate": "users", "pipeline": [], "cursor": {"batchSize": 10}}"#,
            ReadCommand::Aggregate,
        );
        assert_eq!(
            document
                .get_document("cursor")
                .unwrap()
                .get_i32("batchSize"),
            Ok(10)
        );
    }

    #[test]
    fn a_find_gets_no_cursor_option() {
        // `find` returns a cursor without being asked, and `cursor` is not one
        // of its options — adding it would make the server refuse the command.
        let document = wire(r#"{"find": "users"}"#, ReadCommand::Find);
        assert!(document.get("cursor").is_none());
    }

    #[test]
    fn a_plain_command_gets_no_cursor_option() {
        let document = wire(r#"{"count": "users"}"#, ReadCommand::Count);
        assert!(document.get("cursor").is_none());
    }

    #[test]
    fn text_that_is_not_a_document_is_refused() {
        assert!(to_wire("[1, 2]", ReadCommand::Find).is_err());
        assert!(to_wire("not json", ReadCommand::Find).is_err());
    }

    #[test]
    fn a_refusal_does_not_echo_the_command() {
        let secret = "s3cret-collection-name";
        let error = to_wire(&format!("[\"{secret}\"]"), ReadCommand::Find).unwrap_err();
        assert!(
            !error.to_string().contains(secret),
            "the message repeated the caller's text: {error}"
        );
    }
}
