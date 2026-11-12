//! The changes agents leave for a person to approve (ADR-0176), read and
//! answered through the service for the desktop window.
//!
//! A child of `service` so it shares the adapter cache. **The MCP server
//! exposes none of this.** Approval is a person's act, so `server.rs` has no
//! tool that reaches [`McpService::approve_change`] or
//! [`McpService::reject_change`], and a test there fails if one appears.

use dbboard_core::{PendingChange, ReviewError, ReviewRefusal};
use serde::Serialize;

use super::{McpService, ServiceError};
use crate::target::Target;

/// Result of [`McpService::pending_changes`]. `available` is false for a
/// connection that keeps no such queue, which is an answer rather than an
/// error, as with the commit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingView {
    pub connection_id: String,
    pub available: bool,
    pub changes: Vec<PendingChange>,
}

/// What became of an approval or a rejection. A refusal is an answer the
/// screen acts on, not a failure, so it is data here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ReviewAnswer {
    /// Approved; `merge` is the commit that brought it into `main`.
    Approved {
        merge: String,
    },
    Rejected,
    Refused {
        refusal: ReviewRefusal,
    },
}

impl McpService {
    /// Every change waiting for a person on this connection.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] when the database refuses the read.
    pub async fn pending_changes<'t>(
        &self,
        target: impl Into<Target<'t>>,
    ) -> Result<PendingView, ServiceError> {
        let target = target.into();
        let connection_id = target.connection_id.to_string();
        let adapter = self.adapter_for_target(target).await?;
        let Some(review) = adapter.review() else {
            return Ok(PendingView {
                connection_id,
                available: false,
                changes: Vec::new(),
            });
        };
        Ok(PendingView {
            connection_id,
            available: true,
            changes: review.pending().await?,
        })
    }

    /// Bring `branch` into `main`, provided its head is still `head`.
    ///
    /// # Errors
    ///
    /// [`ServiceError::InvalidRequest`] without a reason,
    /// [`ServiceError::NoHistory`] for a connection with no queue, or
    /// [`ServiceError::Db`] for a failure that is not a refusal.
    pub async fn approve_change<'t>(
        &self,
        target: impl Into<Target<'t>>,
        branch: &str,
        head: &str,
        reason: &str,
    ) -> Result<ReviewAnswer, ServiceError> {
        let reason = required_reason(reason)?;
        let adapter = self.adapter_for_target(target.into()).await?;
        let review = adapter
            .review()
            .ok_or_else(|| ServiceError::NoHistory(adapter.id().to_string()))?;
        answer(
            review
                .approve(branch, head, reason)
                .await
                .map(|merge| ReviewAnswer::Approved { merge }),
        )
    }

    /// Close `branch` without bringing it in, provided its head is still
    /// `head`.
    ///
    /// # Errors
    ///
    /// As [`Self::approve_change`].
    pub async fn reject_change<'t>(
        &self,
        target: impl Into<Target<'t>>,
        branch: &str,
        head: &str,
        reason: &str,
    ) -> Result<ReviewAnswer, ServiceError> {
        let reason = required_reason(reason)?;
        let adapter = self.adapter_for_target(target.into()).await?;
        let review = adapter
            .review()
            .ok_or_else(|| ServiceError::NoHistory(adapter.id().to_string()))?;
        answer(
            review
                .reject(branch, head, reason)
                .await
                .map(|()| ReviewAnswer::Rejected),
        )
    }
}

/// Every decision carries a reason: it is what the agent's operator reads
/// when they ask why.
fn required_reason(reason: &str) -> Result<&str, ServiceError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(ServiceError::InvalidRequest(
            "approving or rejecting a change needs a reason".into(),
        ));
    }
    Ok(reason)
}

fn answer(result: Result<ReviewAnswer, ReviewError>) -> Result<ReviewAnswer, ServiceError> {
    match result {
        Ok(done) => Ok(done),
        Err(ReviewError::Refused(refusal)) => Ok(ReviewAnswer::Refused { refusal }),
        Err(ReviewError::Db(e)) => Err(ServiceError::Db(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decision_without_a_reason_is_refused_before_it_is_sent() {
        assert!(matches!(
            required_reason("   "),
            Err(ServiceError::InvalidRequest(_))
        ));
        assert_eq!(
            required_reason("  looks right ").expect("kept"),
            "looks right"
        );
    }

    #[test]
    fn a_refusal_is_an_answer_and_a_database_error_is_an_error() {
        assert_eq!(
            answer(Err(ReviewError::Refused(ReviewRefusal::Moved))).expect("answer"),
            ReviewAnswer::Refused {
                refusal: ReviewRefusal::Moved
            }
        );
        assert!(matches!(
            answer(Err(ReviewError::Db(dbboard_core::DbError::Query(
                "x".into()
            )))),
            Err(ServiceError::Db(_))
        ));
    }

    #[test]
    fn a_refusal_serialises_with_its_kind_for_the_window() {
        let json = serde_json::to_value(ReviewAnswer::Refused {
            refusal: ReviewRefusal::Conflict {
                keys: vec!["player/9/gems".into()],
                more: 0,
            },
        })
        .expect("json");
        assert_eq!(json["outcome"], "refused");
        assert_eq!(json["refusal"]["kind"], "conflict");
        assert_eq!(json["refusal"]["keys"][0], "player/9/gems");
    }
}
