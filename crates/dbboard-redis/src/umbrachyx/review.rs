//! Umbrachyx's queue of agents' changes waiting for a person (ADR-0176).
//!
//! `UMBRA.PENDING` reads. `UMBRA.APPROVE` and `UMBRA.REJECT` are the only
//! writes dbboard ever sends to Umbrachyx, and they are reached only from a
//! person's button in the window, never from a typed command (they are not
//! on the read-only allowlist) and never from the MCP server.
//!
//! A refusal names its kind in its first word (`NOAUTH`, `NOPERM`,
//! `NOTWAITING`, `CONFLICT`), and `NOTWAITING` names why in its second.
//! Those become a [`ReviewRefusal`] so the screen can act on them. Anything
//! else, such as `ERR no branch named …`, stays an ordinary query error.

use async_trait::async_trait;
use dbboard_core::{
    ChangeReview, DbError, DbResult, PendingChange, ReviewError, ReviewRefusal, TableChangeCount,
};
use redis::aio::ConnectionLike;
use redis::Value as Reply;

use super::{date, UmbrachyxHistory};
use crate::adapter::Connection;

#[async_trait]
impl ChangeReview for UmbrachyxHistory {
    async fn pending(&self) -> DbResult<Vec<PendingChange>> {
        let reply = request(&self.connection, &redis::cmd("UMBRA.PENDING"))
            .await
            .map_err(Failure::into_db)?;
        pending(reply)
    }

    async fn approve(&self, branch: &str, head: &str, reason: &str) -> Result<String, ReviewError> {
        let mut cmd = redis::cmd("UMBRA.APPROVE");
        cmd.arg(branch).arg(head).arg(reason);
        match request(&self.connection, &cmd).await? {
            Reply::BulkString(bytes) => String::from_utf8(bytes)
                .map_err(|_| DbError::TypeConversion("the merge id is not UTF-8".into()).into()),
            Reply::SimpleString(text) => Ok(text),
            _ => Err(DbError::TypeConversion(
                "UMBRA.APPROVE answered something other than the merge id".into(),
            )
            .into()),
        }
    }

    async fn reject(&self, branch: &str, head: &str, reason: &str) -> Result<(), ReviewError> {
        let mut cmd = redis::cmd("UMBRA.REJECT");
        cmd.arg(branch).arg(head).arg(reason);
        request(&self.connection, &cmd)
            .await
            .map(|_| ())
            .map_err(ReviewError::from)
    }
}

/// A failed request, with the server's refusal told apart when it is one.
struct Failure(redis::RedisError);

impl Failure {
    fn into_db(self) -> DbError {
        crate::adapter::query_error(&self.0)
    }
}

impl From<Failure> for ReviewError {
    fn from(failure: Failure) -> Self {
        match refusal(failure.0.code(), failure.0.detail()) {
            Some(refused) => ReviewError::Refused(refused),
            None => ReviewError::Db(failure.into_db()),
        }
    }
}

async fn request(connection: &Connection, cmd: &redis::Cmd) -> Result<Reply, Failure> {
    let reply = match connection.clone() {
        Connection::Single(mut c) => c.req_packed_command(cmd).await,
        Connection::Cluster(mut c) => c.req_packed_command(cmd).await,
    }
    .map_err(Failure)?;
    // redis-rs hands a server's error line back as a value rather than an
    // `Err`, so a refusal would otherwise read as a malformed answer.
    match reply {
        Reply::ServerError(error) => Err(Failure(error.into())),
        other => Ok(other),
    }
}

/// The refusal a server error stands for, from its first word (`code`) and
/// the rest of the line (`detail`). `None` for an error that is not one of
/// the review refusals.
fn refusal(code: Option<&str>, detail: Option<&str>) -> Option<ReviewRefusal> {
    let detail = detail.unwrap_or_default().trim();
    match code? {
        "NOAUTH" => Some(ReviewRefusal::NoAuth),
        "NOPERM" => Some(ReviewRefusal::NoPermission {
            message: detail.to_string(),
        }),
        "NOTWAITING" => Some(match detail.split_whitespace().next() {
            Some("moved") => ReviewRefusal::Moved,
            Some("decided") => ReviewRefusal::Decided,
            _ => ReviewRefusal::NotWaiting,
        }),
        "CONFLICT" => Some(conflict(detail)),
        _ => None,
    }
}

/// `CONFLICT a b … and N more`: the keys, and how many were left out.
fn conflict(detail: &str) -> ReviewRefusal {
    let (listed, more) = match detail.rsplit_once(" and ") {
        Some((keys, tail)) => match tail.strip_suffix(" more").map(str::parse::<u64>) {
            Some(Ok(n)) => (keys, n),
            _ => (detail, 0),
        },
        None => (detail, 0),
    };
    ReviewRefusal::Conflict {
        keys: listed.split_whitespace().map(str::to_string).collect(),
        more,
    }
}

/// `UMBRA.PENDING`: one array per change, field names and values in turn.
fn pending(reply: Reply) -> DbResult<Vec<PendingChange>> {
    let Reply::Array(changes) = reply else {
        return Err(shape("the reply is not a list"));
    };
    changes.into_iter().map(change).collect()
}

fn change(entry: Reply) -> DbResult<PendingChange> {
    let Reply::Array(fields) = entry else {
        return Err(shape("a change is not a list"));
    };
    let mut change = PendingChange {
        branch: String::new(),
        head: String::new(),
        base: None,
        authors: Vec::new(),
        why: String::new(),
        first_at: String::new(),
        last_at: String::new(),
        tables: Vec::new(),
        main_moved: false,
        conflicts: false,
    };
    let mut fields = fields.into_iter();
    while let (Some(name), Some(value)) = (fields.next(), fields.next()) {
        match text(name)?.as_str() {
            "branch" => change.branch = text(value)?,
            "head" => change.head = text(value)?,
            "base" => change.base = optional_text(value)?,
            "authors" => change.authors = texts(value)?,
            "why" => change.why = text(value)?,
            "first" => change.first_at = date(micros(value)?),
            "last" => change.last_at = date(micros(value)?),
            "tables" => change.tables = tables(value)?,
            "main_moved" => change.main_moved = micros(value)? != 0,
            "conflicts" => change.conflicts = micros(value)? != 0,
            // A field added later is not an error; this build simply does
            // not show it.
            _ => {}
        }
    }
    if change.branch.is_empty() || change.head.is_empty() {
        return Err(shape("a change has no branch or head"));
    }
    Ok(change)
}

fn tables(value: Reply) -> DbResult<Vec<TableChangeCount>> {
    let Reply::Array(items) = value else {
        return Err(shape("tables is not a list"));
    };
    let mut items = items.into_iter();
    let mut out = Vec::new();
    while let (Some(table), Some(count)) = (items.next(), items.next()) {
        out.push(TableChangeCount {
            table: text(table)?,
            changed: micros(count)?,
        });
    }
    Ok(out)
}

fn text(value: Reply) -> DbResult<String> {
    match value {
        Reply::BulkString(bytes) => {
            String::from_utf8(bytes).map_err(|_| shape("a field is not UTF-8"))
        }
        Reply::SimpleString(text) => Ok(text),
        _ => Err(shape("a field is not text")),
    }
}

fn optional_text(value: Reply) -> DbResult<Option<String>> {
    match value {
        Reply::Nil => Ok(None),
        other => text(other).map(Some),
    }
}

fn texts(value: Reply) -> DbResult<Vec<String>> {
    match value {
        Reply::Array(items) => items.into_iter().map(text).collect(),
        _ => Err(shape("authors is not a list")),
    }
}

/// A non-negative integer field: a time in microseconds, a count, or a 0/1.
fn micros(value: Reply) -> DbResult<u64> {
    match value {
        Reply::Int(n) => u64::try_from(n).map_err(|_| shape("a number is negative")),
        other => text(other)?
            .parse()
            .map_err(|_| shape("a number is not a number")),
    }
}

fn shape(what: &str) -> DbError {
    DbError::TypeConversion(format!("UMBRA.PENDING: {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulk(s: &str) -> Reply {
        Reply::BulkString(s.as_bytes().to_vec())
    }

    /// Recorded from `umbrachyx serve` (umbrachyx 462d0a0) on fictional
    /// data: one agent changed two `player` keys after `main` moved on.
    fn recorded(base: Reply) -> Reply {
        Reply::Array(vec![Reply::Array(vec![
            bulk("branch"),
            bulk("agent-a"),
            bulk("head"),
            bulk("62f790e6f0052b6d9e36fbce8180685b1ab29bbada9d8ce5cb6e23fdbdf6342a"),
            bulk("base"),
            base,
            bulk("authors"),
            Reply::Array(vec![bulk("agent:support")]),
            bulk("why"),
            bulk("refund"),
            bulk("first"),
            Reply::Int(1_794_412_568_601_343),
            bulk("last"),
            Reply::Int(1_794_412_568_601_343),
            bulk("tables"),
            Reply::Array(vec![bulk("player"), Reply::Int(2)]),
            bulk("main_moved"),
            Reply::Int(1),
            bulk("conflicts"),
            Reply::Int(0),
        ])])
    }

    #[test]
    fn a_waiting_change_reads_every_field() {
        let base = "564502e2a783442c89dd56129b5e678a4a99990745128f6753b1119c1bb8c8f1";
        let changes = pending(recorded(bulk(base))).expect("parsed");
        assert_eq!(changes.len(), 1);
        let c = &changes[0];
        assert_eq!(c.branch, "agent-a");
        assert!(c.head.starts_with("62f790e6f005"));
        assert_eq!(c.base.as_deref(), Some(base));
        assert_eq!(c.authors, vec!["agent:support"]);
        assert_eq!(c.why, "refund");
        assert!(c.first_at.starts_with("2026-"), "got {}", c.first_at);
        assert_eq!(
            c.tables,
            vec![TableChangeCount {
                table: "player".into(),
                changed: 2
            }]
        );
        assert!(c.main_moved);
        assert!(!c.conflicts);
    }

    /// A branch that shares no commit with `main` has no base: everything
    /// on it is its change.
    #[test]
    fn a_branch_sharing_nothing_with_main_has_no_base() {
        let changes = pending(recorded(Reply::Nil)).expect("parsed");
        assert_eq!(changes[0].base, None);
    }

    #[test]
    fn nothing_waiting_is_an_empty_list() {
        assert!(pending(Reply::Array(Vec::new()))
            .expect("parsed")
            .is_empty());
    }

    #[test]
    fn a_field_this_build_does_not_know_is_skipped() {
        let Reply::Array(mut changes) = recorded(Reply::Nil) else {
            unreachable!()
        };
        let Reply::Array(fields) = &mut changes[0] else {
            unreachable!()
        };
        fields.extend([bulk("reviewers_needed"), Reply::Int(2)]);
        assert_eq!(pending(Reply::Array(changes)).expect("parsed").len(), 1);
    }

    // The refusals below are the lines `umbrachyx serve` sent, split the
    // way redis-rs splits them: the first word is the code.

    #[test]
    fn each_refusal_is_told_apart() {
        assert_eq!(
            refusal(Some("NOAUTH"), Some("Authentication required.")),
            Some(ReviewRefusal::NoAuth)
        );
        assert_eq!(
            refusal(
                Some("NOPERM"),
                Some("the request is outside what the token allows")
            ),
            Some(ReviewRefusal::NoPermission {
                message: "the request is outside what the token allows".into()
            })
        );
        assert_eq!(
            refusal(
                Some("NOTWAITING"),
                Some(
                    "moved no change waiting at d97191f06995: the branch has been written to since"
                )
            ),
            Some(ReviewRefusal::Moved)
        );
        assert_eq!(
            refusal(
                Some("NOTWAITING"),
                Some("decided no change waiting at 6549c6d27b83: it was approved or rejected already")
            ),
            Some(ReviewRefusal::Decided)
        );
        assert_eq!(
            refusal(Some("NOTWAITING"), Some("none no change waiting")),
            Some(ReviewRefusal::NotWaiting)
        );
    }

    #[test]
    fn a_conflict_lists_its_keys_and_how_many_more() {
        assert_eq!(
            refusal(Some("CONFLICT"), Some("player/9/gems")),
            Some(ReviewRefusal::Conflict {
                keys: vec!["player/9/gems".into()],
                more: 0
            })
        );
        assert_eq!(
            refusal(Some("CONFLICT"), Some("a/1 b/2 and 37 more")),
            Some(ReviewRefusal::Conflict {
                keys: vec!["a/1".into(), "b/2".into()],
                more: 37
            })
        );
    }

    /// `ERR no branch named …` is a plain error, not a refusal the screen
    /// has a place for.
    #[test]
    fn an_ordinary_error_is_not_a_refusal() {
        assert_eq!(
            refusal(Some("ERR"), Some("no branch named \"agent-zz\"")),
            None
        );
        assert_eq!(refusal(None, None), None);
    }
}
