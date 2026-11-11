//! Changes an agent has left for a person to approve (ADR-0176).
//!
//! Some databases let an agent write only to a branch of its own, and only
//! a person can bring that branch into `main`. This module is the shape of
//! that queue: what is waiting, and the person's two answers to it. The
//! diff of a waiting change is read through [`crate::HistoryReader::diff`]
//! from [`PendingChange::base`] to [`PendingChange::head`].
//!
//! Approval is a person's act. Nothing here is offered to an agent: the MCP
//! server and the HTTP server expose no route to [`ChangeReview::approve`]
//! or [`ChangeReview::reject`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::DbError;
use crate::error::DbResult;

/// One branch's change that is not on `main` yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingChange {
    /// The branch the change is on. Sent back with an answer, never shown.
    pub branch: String,
    /// The commit the person is looking at. An answer names it, so a branch
    /// that moved on since is refused rather than approved unseen.
    pub head: String,
    /// The newest commit the branch shares with `main`. The branch's own
    /// change is the diff from here to `head`. `None` when it shares none,
    /// in which case everything on the branch is its change.
    pub base: Option<String>,
    pub authors: Vec<String>,
    pub why: String,
    /// When the branch was first and last written, as RFC 3339.
    pub first_at: String,
    pub last_at: String,
    /// How many keys each table changes, in the order the database gave.
    pub tables: Vec<TableChangeCount>,
    /// `main` has moved since the branch left it.
    pub main_moved: bool,
    /// Approving would conflict with `main`.
    pub conflicts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableChangeCount {
    pub table: String,
    pub changed: u64,
}

/// Why an answer to a waiting change was not taken. Each kind leads the
/// person somewhere different, so they are told apart here rather than
/// left in an error message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewRefusal {
    /// The branch was written to after the person looked.
    #[error("the agent wrote more since you looked")]
    Moved,
    /// Someone has already approved or rejected it.
    #[error("someone has already decided this change")]
    Decided,
    /// Nothing is waiting on that branch at that head.
    #[error("this change is no longer waiting")]
    NotWaiting,
    /// Approving would conflict with `main`; nothing was written.
    #[error("approving would conflict with main on {} key(s)", keys.len())]
    Conflict {
        keys: Vec<String>,
        /// Conflicting keys the database left out of `keys`.
        more: u64,
    },
    /// The connection's token cannot approve this change.
    #[error("this connection's token cannot approve this change: {message}")]
    NoPermission { message: String },
    /// The connection has not authenticated.
    #[error("this connection has not authenticated with a token that can approve")]
    NoAuth,
}

/// The answer to an approval or a rejection.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReviewError {
    #[error(transparent)]
    Refused(#[from] ReviewRefusal),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[async_trait]
pub trait ChangeReview: Send + Sync {
    /// Every change waiting for a person.
    async fn pending(&self) -> DbResult<Vec<PendingChange>>;

    /// Bring `branch` into `main`, provided its head is still `head`.
    /// Returns the merge commit.
    async fn approve(&self, branch: &str, head: &str, reason: &str) -> Result<String, ReviewError>;

    /// Close `branch` without bringing it in, provided its head is still
    /// `head`.
    async fn reject(&self, branch: &str, head: &str, reason: &str) -> Result<(), ReviewError>;
}
