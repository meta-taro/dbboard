//! The history a versioned database keeps (ADR-0165), read for the History
//! tab.
//!
//! Thin wrappers over `McpService`, like `browse.rs`: the logic and its tests
//! live in `dbboard-mcp`. Every command here reads. None of them commits,
//! checks out or merges, and none is offered to an MCP agent — that is a verb
//! for a later slot, with its own permission design (ADR-0087).

use dbboard_core::{BlameEntry, CommitDiff, RowHistory, TableInfo, Value};
use dbboard_mcp::service::CommitLogView;
use dbboard_mcp::target::Target;

use crate::AppState;

/// One primary-key column of the row whose history is asked for. `value` is
/// a bare JSON scalar decoded by core's [`Value`], exactly as `update_row`
/// takes it, so a key read from a diff or a blame row goes back unchanged.
#[derive(serde::Deserialize)]
pub(crate) struct HistoryKeyInput {
    column: String,
    value: Value,
}

/// The newest commits, or `available: false` for a connection that keeps
/// no history. The frontend asks this once per connection to decide whether
/// to show the History tab at all.
#[tauri::command]
pub(crate) async fn commit_log(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    database: Option<String>,
    limit: Option<usize>,
) -> Result<CommitLogView, String> {
    state
        .service
        .commit_log(Target::new(&connection_id, database.as_deref()), limit)
        .await
        .map_err(|e| e.to_string())
}

/// One row's value at each commit that changed it, newest first.
#[tauri::command]
pub(crate) async fn row_history(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    database: Option<String>,
    table: TableInfo,
    key: Vec<HistoryKeyInput>,
    limit: Option<usize>,
) -> Result<RowHistory, String> {
    let key: Vec<(String, Value)> = key.into_iter().map(|k| (k.column, k.value)).collect();
    state
        .service
        .row_history(
            Target::new(&connection_id, database.as_deref()),
            &table,
            &key,
            limit,
        )
        .await
        .map_err(|e| e.to_string())
}

/// What changed in one table between two commits.
#[tauri::command]
pub(crate) async fn commit_diff(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    database: Option<String>,
    table: TableInfo,
    from: String,
    to: String,
) -> Result<CommitDiff, String> {
    state
        .service
        .commit_diff(
            Target::new(&connection_id, database.as_deref()),
            &table,
            &from,
            &to,
        )
        .await
        .map_err(|e| e.to_string())
}

/// The commit that last changed each row of one table.
#[tauri::command]
pub(crate) async fn blame(
    state: tauri::State<'_, AppState>,
    connection_id: String,
    database: Option<String>,
    table: TableInfo,
) -> Result<Vec<BlameEntry>, String> {
    state
        .service
        .blame(Target::new(&connection_id, database.as_deref()), &table)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend sends a key the way `update_row` takes one; a cell copied
    /// out of a diff must decode to the same value it was read as.
    #[test]
    fn a_key_decodes_from_the_shape_the_frontend_sends() {
        let key: Vec<HistoryKeyInput> =
            serde_json::from_str(r#"[{"column":"id","value":7},{"column":"code","value":"a"}]"#)
                .expect("decode");
        assert_eq!(key[0].column, "id");
        assert_eq!(key[0].value, Value::Integer(7));
        assert_eq!(key[1].value, Value::Text("a".into()));
    }
}
