//! Prove a `MongoDB` command document is a read-only command (ADR-0046 §6 for
//! the SQL equivalent, issue 0020 for why this one had to be written fresh).
//!
//! `dbboard_core::read_only` cannot be reused: it is `sqlparser`-based and
//! says so in its first line, and a classifier that cannot parse its input
//! must fail closed — which for `MongoDB` would mean refusing everything.
//!
//! Unlike Firestore, the transport carries no information here. Every command
//! goes through the same `runCommand` path, so "which endpoint" cannot decide
//! anything. The rule is an **allowlist of read commands**, with everything
//! unlisted refused, exactly as D1 fails closed on unparseable SQL.
//!
//! Three things make this more than a verb check:
//!
//! - **The command name is the document's *first* field.** That is `MongoDB`'s
//!   own rule, so the classifier has to honour it — and that means the parse
//!   has to preserve field order, which `serde_json::Value` does not (its map
//!   is sorted). Reading a sorted map's first key would let
//!   `{"filter": …, "find": …}` classify as something the server never saw.
//! - **`$out` and `$merge` are aggregation stages that write.** `aggregate` is
//!   on the read list and can still mutate, so the document is walked rather
//!   than its verb read — the same reason the SQL classifier refuses to do
//!   `starts_with("SELECT")` matching.
//! - **`$where`, `$function` and `$accumulator` run JavaScript server-side.**
//!   They do not write by themselves, and they are refused anyway: this is the
//!   classifier behind a surface an agent drives (ADR-0087), and arbitrary
//!   server-side code execution is not something to arrive at by omission.

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use dbboard_core::DbError;

/// Commands this classifier will pass, each with the option fields it may
/// carry. Anything else is refused — a command that has never been reviewed
/// here does not get in, and neither does an option that has not.
///
/// Options are allowlisted rather than write-verbs denylisted because a
/// denylist has to be complete to be sound, and `MongoDB` adds commands. With an
/// allowlist, `{"find": …, "insert": …, "documents": …}` is refused because
/// `insert` is not a `find` option, without anyone having to remember that
/// `insert` writes.
///
/// Deliberately absent: `writeConcern` and `bypassDocumentValidation`, which
/// only mean anything to a pipeline that writes, and every one of those is
/// refused below.
const READ_COMMANDS: &[(&str, ReadCommand, &[&str])] = &[
    (
        "aggregate",
        ReadCommand::Aggregate,
        &[
            "pipeline",
            "cursor",
            "explain",
            "allowDiskUse",
            "maxTimeMS",
            "readConcern",
            "collation",
            "hint",
            "comment",
            "let",
        ],
    ),
    (
        "count",
        ReadCommand::Count,
        &[
            "query",
            "limit",
            "skip",
            "hint",
            "readConcern",
            "collation",
            "comment",
            "maxTimeMS",
        ],
    ),
    (
        "distinct",
        ReadCommand::Distinct,
        &[
            "key",
            "query",
            "readConcern",
            "collation",
            "comment",
            "maxTimeMS",
        ],
    ),
    (
        "find",
        ReadCommand::Find,
        &[
            "filter",
            "sort",
            "projection",
            "hint",
            "skip",
            "limit",
            "batchSize",
            "singleBatch",
            "comment",
            "maxTimeMS",
            "readConcern",
            "max",
            "min",
            "returnKey",
            "showRecordId",
            "tailable",
            "awaitData",
            "noCursorTimeout",
            "allowPartialResults",
            "allowDiskUse",
            "collation",
            "let",
        ],
    ),
    (
        "listCollections",
        ReadCommand::ListCollections,
        &[
            "filter",
            "nameOnly",
            "authorizedCollections",
            "cursor",
            "comment",
        ],
    ),
    (
        "listIndexes",
        ReadCommand::ListIndexes,
        &["cursor", "comment", "maxTimeMS"],
    ),
];

/// Field names that make a command a write, or a code-execution surface,
/// wherever they appear in the document.
///
/// Checked against *keys* only. A document whose `name` happens to equal
/// `"$out"` is an ordinary read, and refusing it would be a false positive on
/// real data.
const FORBIDDEN_KEYS: &[&str] = &["$accumulator", "$function", "$merge", "$out", "$where"];

/// Why a command document was refused.
///
/// Names the *category* of the problem and never echoes the command back, so a
/// rejection cannot reflect caller-controlled text into a log or error
/// surface. Same rule as `dbboard_core::read_only::ReadOnlyViolation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandViolation {
    reason: String,
}

impl CommandViolation {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// The category-level explanation, without any leading prefix.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for CommandViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a read-only MongoDB command: {}", self.reason)
    }
}

impl std::error::Error for CommandViolation {}

impl From<CommandViolation> for DbError {
    /// A refusal surfaces as a query error: the command was rejected before
    /// execution. The category travels in the message; the command does not.
    fn from(violation: CommandViolation) -> Self {
        DbError::Query(violation.to_string())
    }
}

/// The kind of read command a document was proven to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCommand {
    Aggregate,
    Count,
    Distinct,
    Find,
    ListCollections,
    ListIndexes,
}

impl ReadCommand {
    /// Whether the server answers this command with a cursor rather than with
    /// a single reply document. `count` and `distinct` answer in one document;
    /// everything else on the read list streams.
    #[must_use]
    pub fn returns_cursor(self) -> bool {
        !matches!(self, Self::Count | Self::Distinct)
    }

    /// Whether `option` is one of the fields this command may carry.
    ///
    /// The answer comes from the same table the classifier decides with, so
    /// there is one list to keep right rather than two.
    #[must_use]
    pub fn allows_option(self, option: &str) -> bool {
        READ_COMMANDS
            .iter()
            .find(|(_, command, _)| *command == self)
            .is_some_and(|(_, _, options)| options.contains(&option))
    }
}

/// A command document with its field order intact.
///
/// `serde_json::Value`'s map is sorted, and `MongoDB` takes the *first* field as
/// the command name, so a `Value` cannot answer "what command is this?" at
/// all. This type keeps the fields in the order they were written.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandDoc {
    fields: Vec<(String, Value)>,
}

impl CommandDoc {
    /// Parse a command document from JSON text.
    ///
    /// # Errors
    ///
    /// Refuses text that is not JSON, and JSON that is not an object: a
    /// command is a document, and anything else has no command name to read.
    pub fn parse(text: &str) -> Result<Self, CommandViolation> {
        serde_json::from_str(text).map_err(|_| {
            // The parse error is not repeated: it quotes the input, and the
            // input is caller-controlled.
            CommandViolation::new("the command is not a JSON document")
        })
    }

    /// The command name — `MongoDB`'s first field.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.fields.first().map(|(name, _)| name.as_str())
    }

    /// The fields, in the order they were written.
    #[must_use]
    pub fn fields(&self) -> &[(String, Value)] {
        &self.fields
    }
}

impl<'de> Deserialize<'de> for CommandDoc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedFields;

        impl<'de> Visitor<'de> for OrderedFields {
            type Value = CommandDoc;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a MongoDB command document")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<CommandDoc, A::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry::<String, Value>()? {
                    fields.push(entry);
                }
                Ok(CommandDoc { fields })
            }
        }

        deserializer.deserialize_map(OrderedFields)
    }
}

/// Prove a command document is read-only, and say which read it is.
///
/// # Errors
///
/// Refuses anything it cannot prove read-only: unparseable text, a command
/// name that is not on [`READ_COMMANDS`], and any document carrying a
/// forbidden key at any depth.
pub fn classify_read_only(text: &str) -> Result<ReadCommand, CommandViolation> {
    let doc = CommandDoc::parse(text)?;

    let Some(name) = doc.name() else {
        return Err(CommandViolation::new("the command document has no fields"));
    };
    let Some((_, command, options)) = READ_COMMANDS.iter().find(|(verb, _, _)| *verb == name)
    else {
        // The name is not repeated: MongoDB command names are case-sensitive
        // and caller-supplied, so this string is not ours to echo.
        return Err(CommandViolation::new(format!(
            "the command is not one this connection may run; the read commands are {}",
            joined(READ_COMMANDS.iter().map(|(verb, _, _)| *verb))
        )));
    };

    for (position, (field, value)) in doc.fields().iter().enumerate() {
        // The first field is the command name, already checked above; every
        // other one has to be an option this classifier has reviewed.
        if position > 0 && !options.contains(&field.as_str()) {
            return Err(CommandViolation::new(format!(
                "the command carries a field that is not a reviewed `{name}` option; \
                 the reviewed ones are {}",
                joined(options.iter().copied())
            )));
        }
        if let Some(forbidden) = forbidden_key_within(value) {
            return Err(refusal_for(forbidden));
        }
    }

    Ok(*command)
}

/// The first forbidden key anywhere inside a value, if there is one.
///
/// Keys only. A value that happens to spell `$out` is data, and refusing it
/// would be a false positive on a perfectly ordinary query.
///
/// Recursion is bounded by the parse: `serde_json` refuses to build a `Value`
/// nested past its own recursion limit, so a hostile document fails in
/// [`CommandDoc::parse`] rather than arriving here deep enough to matter.
fn forbidden_key_within(value: &Value) -> Option<&'static str> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, nested)| {
            FORBIDDEN_KEYS
                .iter()
                .find(|forbidden| **forbidden == key.as_str())
                .copied()
                .or_else(|| forbidden_key_within(nested))
        }),
        Value::Array(items) => items.iter().find_map(forbidden_key_within),
        _ => None,
    }
}

/// Say *why* a forbidden key is forbidden. The two groups fail for different
/// reasons, and a user who is told "it writes" when it does not will go
/// looking for a write that is not there.
fn refusal_for(key: &'static str) -> CommandViolation {
    let because = match key {
        "$out" | "$merge" => "writes a collection",
        _ => "runs JavaScript on the server",
    };
    CommandViolation::new(format!("`{key}` {because}"))
}

fn joined<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

/// Prove a command document is read-only, discarding which read it is.
///
/// # Errors
///
/// As [`classify_read_only`].
pub fn check_read_only(text: &str) -> Result<(), CommandViolation> {
    classify_read_only(text).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(text: &str) -> String {
        classify_read_only(text)
            .expect_err("a command that should have been refused was accepted")
            .reason()
            .to_string()
    }

    #[test]
    fn a_find_is_a_read() {
        let command = r#"{"find": "orders", "filter": {"status": "open"}, "limit": 100}"#;
        assert_eq!(classify_read_only(command), Ok(ReadCommand::Find));
    }

    #[test]
    fn the_other_allowed_verbs_classify_as_themselves() {
        for (command, want) in [
            (
                r#"{"aggregate": "orders", "pipeline": [], "cursor": {}}"#,
                ReadCommand::Aggregate,
            ),
            (r#"{"count": "orders"}"#, ReadCommand::Count),
            (
                r#"{"distinct": "orders", "key": "status"}"#,
                ReadCommand::Distinct,
            ),
            (r#"{"listCollections": 1}"#, ReadCommand::ListCollections),
            (r#"{"listIndexes": "orders"}"#, ReadCommand::ListIndexes),
        ] {
            assert_eq!(classify_read_only(command), Ok(want), "for {command}");
        }
    }

    #[test]
    fn an_aggregate_that_only_reads_is_allowed() {
        let command = r#"{
            "aggregate": "orders",
            "pipeline": [
                {"$match": {"status": "open"}},
                {"$group": {"_id": "$customer", "total": {"$sum": "$amount"}}},
                {"$sort": {"total": -1}}
            ],
            "cursor": {}
        }"#;
        assert_eq!(classify_read_only(command), Ok(ReadCommand::Aggregate));
    }

    #[test]
    fn a_write_command_is_refused() {
        for command in [
            r#"{"insert": "orders", "documents": [{"a": 1}]}"#,
            r#"{"update": "orders", "updates": [{"q": {}, "u": {"$set": {"a": 1}}}]}"#,
            r#"{"delete": "orders", "deletes": [{"q": {}, "limit": 0}]}"#,
            r#"{"findAndModify": "orders", "query": {}, "remove": true}"#,
            r#"{"drop": "orders"}"#,
            r#"{"dropDatabase": 1}"#,
            r#"{"createIndexes": "orders", "indexes": []}"#,
            r#"{"renameCollection": "a.b", "to": "a.c"}"#,
        ] {
            assert!(
                classify_read_only(command).is_err(),
                "accepted a write: {command}"
            );
        }
    }

    /// `mapReduce` has an `out` clause that writes a collection. It is refused
    /// because it is not on the allowlist, which is the whole point of an
    /// allowlist: a command nobody reviewed does not get in.
    #[test]
    fn an_unreviewed_command_is_refused_even_if_it_looks_like_a_read() {
        for command in [
            r#"{"mapReduce": "orders", "map": "f", "reduce": "g", "out": "totals"}"#,
            r#"{"eval": "function () { return 1; }"}"#,
            r#"{"getMore": 1, "collection": "orders"}"#,
        ] {
            assert!(
                classify_read_only(command).is_err(),
                "accepted an unreviewed command: {command}"
            );
        }
    }

    #[test]
    fn an_aggregate_that_writes_through_out_is_refused() {
        let command = r#"{
            "aggregate": "orders",
            "pipeline": [{"$match": {"status": "open"}}, {"$out": "snapshot"}],
            "cursor": {}
        }"#;
        assert!(refusal(command).contains("$out"));
    }

    #[test]
    fn an_aggregate_that_writes_through_merge_is_refused() {
        let command = r#"{
            "aggregate": "orders",
            "pipeline": [{"$merge": {"into": "totals"}}],
            "cursor": {}
        }"#;
        assert!(refusal(command).contains("$merge"));
    }

    /// A writing stage does not have to be at the top of the pipeline. `$facet`,
    /// `$lookup` and `$unionWith` all carry pipelines of their own, so a walk
    /// that stops at the first level would wave these through.
    #[test]
    fn a_writing_stage_nested_in_another_stage_is_refused() {
        for command in [
            r#"{"aggregate": "o", "pipeline": [{"$facet": {"a": [{"$out": "x"}]}}], "cursor": {}}"#,
            r#"{"aggregate": "o", "pipeline": [{"$lookup": {"from": "b", "pipeline": [{"$merge": {"into": "x"}}], "as": "j"}}], "cursor": {}}"#,
            r#"{"aggregate": "o", "pipeline": [{"$unionWith": {"coll": "b", "pipeline": [{"$out": "x"}]}}], "cursor": {}}"#,
        ] {
            assert!(
                classify_read_only(command).is_err(),
                "accepted a nested write: {command}"
            );
        }
    }

    /// Server-side JavaScript is refused deliberately, not by omission — see
    /// this module's header.
    #[test]
    fn server_side_javascript_is_refused() {
        for command in [
            r#"{"find": "orders", "filter": {"$where": "this.a == 1"}}"#,
            r#"{"count": "orders", "query": {"$where": "true"}}"#,
            r#"{"aggregate": "o", "pipeline": [{"$addFields": {"x": {"$function": {"body": "f", "args": [], "lang": "js"}}}}], "cursor": {}}"#,
            r#"{"aggregate": "o", "pipeline": [{"$group": {"_id": 1, "x": {"$accumulator": {"init": "f", "lang": "js"}}}}], "cursor": {}}"#,
        ] {
            assert!(
                classify_read_only(command).is_err(),
                "accepted server-side JavaScript: {command}"
            );
        }
    }

    /// The forbidden names are checked against keys. A document whose *value*
    /// is the string `"$out"` is ordinary data, and refusing it would break
    /// real queries for nothing.
    #[test]
    fn a_forbidden_name_appearing_as_a_value_is_not_a_write() {
        let command = r#"{"find": "orders", "filter": {"label": "$out", "note": "$where"}}"#;
        assert_eq!(classify_read_only(command), Ok(ReadCommand::Find));
    }

    /// `MongoDB` reads the command name from the *first* field. `serde_json`'s
    /// map is sorted, so a classifier built on `Value` would read `find` here
    /// and pass a document the server would reject — or, worse, one it would
    /// interpret differently.
    #[test]
    fn field_order_survives_the_parse() {
        let doc = CommandDoc::parse(r#"{"zebra": 1, "find": "orders", "alpha": 2}"#)
            .expect("a well-formed document was refused");
        let names: Vec<&str> = doc.fields().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["zebra", "find", "alpha"]);
        assert_eq!(doc.name(), Some("zebra"));
    }

    #[test]
    fn a_command_name_that_is_not_first_is_refused() {
        let command = r#"{"filter": {"status": "open"}, "find": "orders"}"#;
        assert!(
            classify_read_only(command).is_err(),
            "the command name must be the first field, as MongoDB reads it"
        );
    }

    /// Smuggling a second verb past the first one. The server would run the
    /// first field and treat the rest as options, but an option named `insert`
    /// is not something a read-only surface should be relaying either.
    #[test]
    fn a_write_verb_hidden_behind_a_read_verb_is_refused() {
        let command = r#"{"find": "orders", "insert": "orders", "documents": [{"a": 1}]}"#;
        assert!(
            classify_read_only(command).is_err(),
            "accepted a document carrying a write verb alongside a read one"
        );
    }

    #[test]
    fn input_that_is_not_a_command_document_is_refused() {
        for command in [
            "",
            "   ",
            "not json at all",
            "[]",
            r#""find""#,
            "42",
            "null",
            "{}",
            r#"{"find": "orders""#,
        ] {
            assert!(
                classify_read_only(command).is_err(),
                "accepted something that is not a command document: {command:?}"
            );
        }
    }

    /// Command names are case-sensitive in `MongoDB`, so the allowlist is too.
    #[test]
    fn the_allowlist_is_case_sensitive() {
        assert!(classify_read_only(r#"{"FIND": "orders"}"#).is_err());
        assert!(classify_read_only(r#"{"Find": "orders"}"#).is_err());
    }

    /// A refusal is read by a user and often logged. Echoing the command back
    /// would put caller-controlled text — possibly a filter holding real data
    /// — into that log.
    #[test]
    fn a_refusal_does_not_echo_the_command() {
        let secret = "a-value-that-must-not-be-logged";
        let command = format!(r#"{{"insert": "orders", "documents": [{{"a": "{secret}"}}]}}"#);
        let violation = classify_read_only(&command).expect_err("a write was accepted");
        assert!(
            !violation.to_string().contains(secret),
            "the refusal echoed the command: {violation}"
        );
    }

    #[test]
    fn check_read_only_agrees_with_classify() {
        assert!(check_read_only(r#"{"find": "orders"}"#).is_ok());
        assert!(check_read_only(r#"{"drop": "orders"}"#).is_err());
    }

    #[test]
    fn a_violation_becomes_a_query_error() {
        let violation = CommandViolation::new("a reason");
        let error: DbError = violation.into();
        assert!(matches!(error, DbError::Query(_)));
    }
}
