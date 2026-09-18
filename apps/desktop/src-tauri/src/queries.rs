//! Saved-query commands (ADR-0147).
//!
//! The write path for `saved-queries.toml`, mirroring the annotation
//! commands in [`crate::browse`]: [`SavedQueryAdmin`](dbboard_config::SavedQueryAdmin)
//! owns the file, every mutation writes it whole and atomically, and nothing
//! here touches a database or an adapter — a saved query is text on the
//! operator's own disk.
//!
//! Deliberately **not** MCP tools. An agent can already run any statement it
//! can compose, so exposing the list adds no capability it lacks; what it
//! would add is the operator's private working notes arriving in an agent's
//! context because a tool listed them (ADR-0087's rule: a verb is additive
//! only while it opens nothing that was not already open).

use dbboard_config::{SavedQuery, SavedQueryError};
use serde::Serialize;

use crate::AppState;

/// One saved query as the frontend sees it. A separate shape from
/// `dbboard_config::SavedQuery` so the on-disk format can change without the
/// IPC surface following it by accident.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SavedQueryView {
    pub name: String,
    pub sql: String,
    pub saved_at: i64,
}

impl From<&SavedQuery> for SavedQueryView {
    fn from(q: &SavedQuery) -> Self {
        Self {
            name: q.name.clone(),
            sql: q.sql.clone(),
            saved_at: q.saved_at,
        }
    }
}

/// Every saved query for `connection_id`, in the order they were first
/// saved. Sorting is the caller's business — the file keeps insertion order
/// so its diffs stay readable, and each entry carries `saved_at`.
#[tauri::command]
pub(crate) async fn list_saved_queries(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<SavedQueryView>, String> {
    let admin = state.saved_queries.lock().map_err(|_| lock_poisoned())?;
    Ok(admin
        .queries(&connection_id)
        .iter()
        .map(SavedQueryView::from)
        .collect())
}

/// Save `sql` under `name` for `connection_id`.
///
/// `overwrite` is the answer to a question the frontend must have asked
/// first: with `false` a name that is already taken is refused, and the
/// caller is expected to confirm with the operator before calling again with
/// `true`. Replacing silently is how a saved query is lost, and this layer
/// cannot know whether it was meant.
#[tauri::command]
pub(crate) async fn save_query(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    name: String,
    sql: String,
    overwrite: bool,
) -> Result<(), String> {
    let at = now_ms();
    let mut admin = state.saved_queries.lock().map_err(|_| lock_poisoned())?;
    let result = if overwrite {
        admin.replace(&connection_id, &name, &sql, at)
    } else {
        admin.add(&connection_id, &name, &sql, at)
    };
    result.map_err(describe)
}

/// Delete the saved query named `name`. Returns whether one was there, so a
/// second click on a stale list is a no-op rather than an error.
#[tauri::command]
pub(crate) async fn delete_saved_query(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    name: String,
) -> Result<bool, String> {
    let mut admin = state.saved_queries.lock().map_err(|_| lock_poisoned())?;
    admin.remove(&connection_id, &name).map_err(describe)
}

/// Epoch milliseconds, or 0 if the clock is before the epoch.
///
/// A stamp is a convenience for ordering, never a correctness input, so a
/// nonsensical clock costs the operator nothing and must not fail the save.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The message the frontend shows. `DuplicateName` keeps a distinct prefix so
/// the caller can tell "this name is taken" (ask, then retry with
/// `overwrite`) from every other failure (report and stop).
fn describe(err: SavedQueryError) -> String {
    match err {
        SavedQueryError::DuplicateName { .. } => "duplicate-name".to_owned(),
        other => other.to_string(),
    }
}

fn lock_poisoned() -> String {
    "the saved-query store is unavailable after an earlier panic".to_owned()
}
