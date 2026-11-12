//! The changes agents leave for a person to approve (ADR-0176), for the
//! Review tab.
//!
//! Thin wrappers over `McpService`, like `history.rs`. Approve and reject are
//! the only writes dbboard makes to Umbrachyx, and they live here, in the
//! window, because approval is a person's act. The MCP server has no tool
//! for either.

use dbboard_mcp::service::{PendingView, ReviewAnswer};
use dbboard_mcp::target::Target;

use crate::AppState;

/// The waiting changes, or `available: false` for a connection without a
/// queue. The frontend asks this to decide whether to show the tab.
#[tauri::command]
pub(crate) async fn pending_changes(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<PendingView, String> {
    state
        .service
        .pending_changes(Target::new(&connection_id, None))
        .await
        .map_err(|e| e.to_string())
}

/// Approve the change on `branch` that the person looked at (`head`).
#[tauri::command]
pub(crate) async fn approve_change(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    branch: String,
    head: String,
    reason: String,
) -> Result<ReviewAnswer, String> {
    state
        .service
        .approve_change(Target::new(&connection_id, None), &branch, &head, &reason)
        .await
        .map_err(|e| e.to_string())
}

/// Reject the change on `branch` that the person looked at (`head`).
#[tauri::command]
pub(crate) async fn reject_change(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    branch: String,
    head: String,
    reason: String,
) -> Result<ReviewAnswer, String> {
    state
        .service
        .reject_change(Target::new(&connection_id, None), &branch, &head, &reason)
        .await
        .map_err(|e| e.to_string())
}
