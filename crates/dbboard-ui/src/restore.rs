//! Worker-side restore (logical import) plumbing (ADR-0051, slice 6).
//!
//! The pure restore orchestrator and its preflight live in `dbboard-core`
//! ([`run_restore`](dbboard_core::run_restore),
//! [`plan_restore`](dbboard_core::plan_restore)). This module supplies the
//! single worker-side piece they need — the one that touches the UI channel
//! and so cannot live in the domain layer:
//!
//! - [`ChannelControl`] — a [`RestoreControl`] that forwards each
//!   [`RestoreProgress`] to the UI as [`Reply::RestoreProgress`] (waking the
//!   egui frame) and reads cancellation off a [`CancellationToken`], the same
//!   token a [`Command::CancelRestore`](crate::Command) cancels.
//!
//! Unlike the dump side there is no sink here: the restore writes into the
//! target database through the adapter, not to a file. The `.sql` file is read
//! and classified at preflight time (`PlanRestore`), so the plan the worker
//! runs already holds the parsed statements — [`run_restore`] just applies
//! them and answers with a single terminal [`Reply`].

use std::sync::mpsc::Sender;
use std::sync::Arc;

use dbboard_core::{
    run_restore as core_run_restore, DatabaseAdapter, RestoreControl, RestoreError, RestoreOptions,
    RestorePlan, RestoreProgress,
};
use eframe::egui;
use tokio_util::sync::CancellationToken;

use crate::Reply;

/// A [`RestoreControl`] that bridges the orchestrator to the UI: every
/// progress snapshot becomes a [`Reply::RestoreProgress`] and wakes the frame;
/// cancellation is read off the shared token.
struct ChannelControl {
    reply_tx: Sender<Reply>,
    ctx: egui::Context,
    cancel: CancellationToken,
}

impl RestoreControl for ChannelControl {
    fn report(&self, progress: &RestoreProgress) {
        // A closed channel means the UI is gone; the terminal reply send will
        // no-op too, and the restore finishes into the void harmlessly.
        let _ = self.reply_tx.send(Reply::RestoreProgress {
            progress: progress.clone(),
        });
        self.ctx.request_repaint();
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// Run one restore and answer with a single terminal [`Reply`].
///
/// Emits [`Reply::RestoreProgress`] throughout via [`ChannelControl`]. A
/// cancellation observed mid-run is not an error: [`run_restore`] returns an
/// outcome with `cancelled == true`, surfaced as [`Reply::RestoreComplete`] so
/// the UI can report the partial restore honestly. Only a fatal
/// [`RestoreError`] (a non-empty target the caller did not confirm, an
/// adapter that cannot execute writes, or an atomic batch that unwound)
/// becomes a [`Reply::RestoreFailed`].
pub(crate) async fn run_restore(
    adapter: Arc<dyn DatabaseAdapter>,
    plan: RestorePlan,
    options: RestoreOptions,
    cancel: CancellationToken,
    reply_tx: Sender<Reply>,
    ctx: egui::Context,
) {
    let control = ChannelControl {
        reply_tx: reply_tx.clone(),
        ctx: ctx.clone(),
        cancel,
    };

    let reply = match core_run_restore(adapter.as_ref(), &plan, options, &control).await {
        Ok(outcome) => Reply::RestoreComplete { outcome },
        Err(e) => Reply::RestoreFailed {
            message: restore_error_message(&e),
        },
    };
    send(&reply_tx, &ctx, reply);
}

/// Render a fatal [`RestoreError`] as a UI-ready message. `TargetNotEmpty`
/// should not reach here (the UI gates on the empty-target confirmation before
/// sending `StartRestore`), but it is mapped defensively rather than dropped.
fn restore_error_message(error: &RestoreError) -> String {
    error.to_string()
}

fn send(reply_tx: &Sender<Reply>, ctx: &egui::Context, reply: Reply) {
    let _ = reply_tx.send(reply);
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbboard_core::RestoreControl;
    use std::sync::mpsc;

    #[test]
    fn channel_control_forwards_progress_and_reads_cancellation() {
        let (tx, rx) = mpsc::channel();
        let cancel = CancellationToken::new();
        let control = ChannelControl {
            reply_tx: tx,
            ctx: egui::Context::default(),
            cancel: cancel.clone(),
        };

        assert!(!control.is_cancelled());
        control.report(&RestoreProgress {
            statements_total: 10,
            statements_done: 4,
            current_index: Some(4),
        });
        cancel.cancel();
        assert!(control.is_cancelled());

        match rx.try_recv().unwrap() {
            Reply::RestoreProgress { progress } => {
                assert_eq!(progress.statements_done, 4);
                assert_eq!(progress.current_index, Some(4));
            }
            other => panic!("expected RestoreProgress, got {other:?}"),
        }
    }

    #[test]
    fn target_not_empty_renders_a_message() {
        let msg = restore_error_message(&RestoreError::TargetNotEmpty {
            existing: vec!["users".into(), "orders".into()],
        });
        assert!(msg.contains("not empty"), "unexpected message: {msg}");
    }
}
