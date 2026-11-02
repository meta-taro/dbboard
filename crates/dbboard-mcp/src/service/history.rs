//! The history a versioned database keeps, read through the service
//! (ADR-0165).
//!
//! A child of `service` so it shares the adapter cache without widening its
//! visibility. Every call here reads; none of them commits, checks out or
//! merges, because this slot shows history and never changes it.

use dbboard_core::{BlameEntry, Commit, CommitDiff, RowHistory, TableInfo, Value};
use serde::Serialize;

use super::{McpService, ServiceError};
use crate::target::Target;

/// Commits listed when the caller does not say how many.
pub const DEFAULT_COMMIT_LIMIT: usize = 100;
/// The most commits one call returns. A long-lived Dolt database has tens of
/// thousands; the log is something a person scrolls, not an export.
pub const MAX_COMMIT_LIMIT: usize = 1000;
/// Versions of one row listed when the caller does not say how many.
pub const DEFAULT_ROW_VERSIONS: usize = 50;

/// Result of [`McpService::commit_log`].
///
/// `available` is false for a connection that keeps no history, which is
/// most of them. That is an answer rather than an error: the client asks
/// this once per connection to decide whether to show the history views at
/// all (ADR-0165, "the client shows the views only when it does").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitLogView {
    pub connection_id: String,
    pub available: bool,
    /// Newest first.
    pub commits: Vec<Commit>,
}

impl McpService {
    /// The newest `limit` commits (default [`DEFAULT_COMMIT_LIMIT`], clamped
    /// to [`MAX_COMMIT_LIMIT`]).
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConnectionNotFound`] for an unknown id, or
    /// [`ServiceError::Db`] when the database refuses the read.
    pub async fn commit_log<'t>(
        &self,
        target: impl Into<Target<'t>>,
        limit: Option<usize>,
    ) -> Result<CommitLogView, ServiceError> {
        let target = target.into();
        let connection_id = target.connection_id.to_string();
        let adapter = self.adapter_for_target(target).await?;
        let Some(history) = adapter.history() else {
            return Ok(CommitLogView {
                connection_id,
                available: false,
                commits: Vec::new(),
            });
        };
        let limit = clamp(limit, DEFAULT_COMMIT_LIMIT, MAX_COMMIT_LIMIT);
        Ok(CommitLogView {
            connection_id,
            available: true,
            commits: history.commit_log(limit).await?,
        })
    }

    /// One row's value at each commit that changed it, newest first.
    ///
    /// # Errors
    ///
    /// [`ServiceError::InvalidRequest`] for an empty key: without one every
    /// row of the table would match. [`ServiceError::NoHistory`] when the
    /// connection keeps none.
    pub async fn row_history<'t>(
        &self,
        target: impl Into<Target<'t>>,
        table: &TableInfo,
        key: &[(String, Value)],
        limit: Option<usize>,
    ) -> Result<RowHistory, ServiceError> {
        if key.is_empty() {
            return Err(ServiceError::InvalidRequest(
                "a row's history needs its primary key".into(),
            ));
        }
        let adapter = self.adapter_for_target(target.into()).await?;
        let history = adapter
            .history()
            .ok_or_else(|| ServiceError::NoHistory(adapter.id().to_string()))?;
        let limit = clamp(limit, DEFAULT_ROW_VERSIONS, MAX_COMMIT_LIMIT);
        Ok(history.row_history(table, key, limit).await?)
    }

    /// What changed in `table` between two commits. `from` and `to` are any
    /// ref the database resolves: a hash, a branch, `HEAD`, `HEAD~1`.
    ///
    /// # Errors
    ///
    /// [`ServiceError::InvalidRequest`] for a blank ref, which would otherwise
    /// reach the database as an empty name. [`ServiceError::NoHistory`] when
    /// the connection keeps none.
    pub async fn commit_diff<'t>(
        &self,
        target: impl Into<Target<'t>>,
        table: &TableInfo,
        from: &str,
        to: &str,
    ) -> Result<CommitDiff, ServiceError> {
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty() || to.is_empty() {
            return Err(ServiceError::InvalidRequest(
                "a diff needs two commits to compare".into(),
            ));
        }
        let adapter = self.adapter_for_target(target.into()).await?;
        let history = adapter
            .history()
            .ok_or_else(|| ServiceError::NoHistory(adapter.id().to_string()))?;
        Ok(history.diff(table, from, to).await?)
    }

    /// The commit that last changed each row of `table`.
    ///
    /// # Errors
    ///
    /// [`ServiceError::NoHistory`] when the connection keeps none, or
    /// [`ServiceError::Db`] — Dolt refuses blame on a table without a
    /// primary key.
    pub async fn blame<'t>(
        &self,
        target: impl Into<Target<'t>>,
        table: &TableInfo,
    ) -> Result<Vec<BlameEntry>, ServiceError> {
        let adapter = self.adapter_for_target(target.into()).await?;
        let history = adapter
            .history()
            .ok_or_else(|| ServiceError::NoHistory(adapter.id().to_string()))?;
        Ok(history.blame(table).await?)
    }
}

fn clamp(asked: Option<usize>, default: usize, max: usize) -> usize {
    asked.unwrap_or(default).clamp(1, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::CachedAdapter;
    use async_trait::async_trait;
    use dbboard_config::InMemorySecretStore;
    use dbboard_core::{
        Capabilities, ChangeKind, Column, DatabaseAdapter, DbResult, HistoryReader, QueryResult,
        Row, RowChange,
    };
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    /// Records what the service asked the reader for, and answers with one
    /// commit, so the tests can see both directions.
    #[derive(Default)]
    struct Versioned {
        asked: Mutex<Vec<String>>,
    }

    fn commit(hash: &str) -> Commit {
        Commit {
            hash: hash.into(),
            author: "Sample Author".into(),
            email: "author@example.com".into(),
            date: "2026-01-02 03:04:05".into(),
            message: "seed".into(),
        }
    }

    fn id_column() -> Column {
        Column {
            name: "id".into(),
            declared_type: None,
        }
    }

    #[async_trait]
    impl HistoryReader for Versioned {
        async fn commit_log(&self, limit: usize) -> DbResult<Vec<Commit>> {
            self.asked.lock().unwrap().push(format!("log {limit}"));
            Ok(vec![commit("abc")])
        }
        async fn row_history(
            &self,
            table: &TableInfo,
            key: &[(String, Value)],
            limit: usize,
        ) -> DbResult<RowHistory> {
            self.asked
                .lock()
                .unwrap()
                .push(format!("row {} {} {limit}", table.name, key[0].0));
            Ok(RowHistory {
                columns: vec![id_column()],
                versions: Vec::new(),
            })
        }
        async fn diff(&self, table: &TableInfo, from: &str, to: &str) -> DbResult<CommitDiff> {
            self.asked
                .lock()
                .unwrap()
                .push(format!("diff {} {from} {to}", table.name));
            Ok(CommitDiff {
                columns: vec![id_column()],
                changes: RowChange::between(None, Some(Row::new(vec![Value::Integer(1)])))
                    .into_iter()
                    .collect(),
            })
        }
        async fn blame(&self, table: &TableInfo) -> DbResult<Vec<BlameEntry>> {
            self.asked
                .lock()
                .unwrap()
                .push(format!("blame {}", table.name));
            Ok(vec![BlameEntry {
                key: Row::new(vec![Value::Integer(1)]),
                commit: commit("abc"),
            }])
        }
    }

    #[async_trait]
    impl DatabaseAdapter for Versioned {
        fn id(&self) -> &'static str {
            "mysql"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                has_history: true,
                ..Capabilities::default()
            }
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(Vec::new())
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            Ok(QueryResult::empty())
        }
        fn history(&self) -> Option<&dyn HistoryReader> {
            Some(self)
        }
    }

    /// A plain MySQL server: no history.
    struct Plain;

    #[async_trait]
    impl DatabaseAdapter for Plain {
        fn id(&self) -> &'static str {
            "mysql"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(Vec::new())
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            Ok(QueryResult::empty())
        }
    }

    struct Fixture {
        _dir: TempDir,
        service: McpService,
        versioned: Arc<Versioned>,
    }

    async fn fixture() -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let config_path = dir.path().join("connections.toml");
        std::fs::write(&config_path, "version = 1\n").expect("write config");
        let service = McpService::new(
            config_path,
            dir.path().join("annotations.toml"),
            dir.path().join("ui-settings.toml"),
            Arc::new(InMemorySecretStore::default()),
        );
        let versioned = Arc::new(Versioned::default());
        {
            let mut cache = service.cache.lock().await;
            cache.insert(
                "dolt".to_string(),
                CachedAdapter::new(versioned.clone() as Arc<dyn DatabaseAdapter>),
            );
            cache.insert("plain".to_string(), CachedAdapter::new(Arc::new(Plain)));
        }
        Fixture {
            _dir: dir,
            service,
            versioned,
        }
    }

    fn orders() -> TableInfo {
        TableInfo::unqualified("orders")
    }

    fn asked(fx: &Fixture) -> Vec<String> {
        fx.versioned.asked.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn a_connection_without_history_answers_unavailable_not_an_error() {
        let fx = fixture().await;
        let view = fx.service.commit_log("plain", None).await.expect("log");
        assert!(!view.available);
        assert!(view.commits.is_empty());
        assert_eq!(view.connection_id, "plain");
    }

    #[tokio::test]
    async fn the_commit_log_uses_the_default_and_clamps_the_limit() {
        let fx = fixture().await;
        let view = fx.service.commit_log("dolt", None).await.expect("log");
        assert!(view.available);
        assert_eq!(view.commits, vec![commit("abc")]);
        fx.service.commit_log("dolt", Some(0)).await.expect("log");
        fx.service
            .commit_log("dolt", Some(1_000_000))
            .await
            .expect("log");
        assert_eq!(asked(&fx), ["log 100", "log 1", "log 1000"]);
    }

    #[tokio::test]
    async fn the_other_views_are_refused_where_there_is_no_history() {
        let fx = fixture().await;
        let key = [("id".to_string(), Value::Integer(1))];
        let errors = [
            fx.service
                .row_history("plain", &orders(), &key, None)
                .await
                .unwrap_err(),
            fx.service
                .commit_diff("plain", &orders(), "HEAD~1", "HEAD")
                .await
                .unwrap_err(),
            fx.service.blame("plain", &orders()).await.unwrap_err(),
        ];
        for e in errors {
            assert!(matches!(e, ServiceError::NoHistory(_)), "{e}");
        }
    }

    #[tokio::test]
    async fn row_history_needs_a_key() {
        let fx = fixture().await;
        let e = fx
            .service
            .row_history("dolt", &orders(), &[], None)
            .await
            .unwrap_err();
        assert!(matches!(e, ServiceError::InvalidRequest(_)), "{e}");
        assert!(asked(&fx).is_empty(), "nothing may reach the database");
    }

    #[tokio::test]
    async fn row_history_passes_the_key_and_default_limit_through() {
        let fx = fixture().await;
        let key = [("id".to_string(), Value::Integer(1))];
        fx.service
            .row_history("dolt", &orders(), &key, None)
            .await
            .expect("row history");
        assert_eq!(asked(&fx), ["row orders id 50"]);
    }

    #[tokio::test]
    async fn a_diff_needs_two_refs_and_trims_them() {
        let fx = fixture().await;
        let e = fx
            .service
            .commit_diff("dolt", &orders(), "  ", "HEAD")
            .await
            .unwrap_err();
        assert!(matches!(e, ServiceError::InvalidRequest(_)), "{e}");

        let diff = fx
            .service
            .commit_diff("dolt", &orders(), " abc ", "HEAD")
            .await
            .expect("diff");
        assert_eq!(diff.changes[0].kind, ChangeKind::Added);
        assert_eq!(asked(&fx), ["diff orders abc HEAD"]);
    }

    #[tokio::test]
    async fn blame_reads_the_table_it_was_given() {
        let fx = fixture().await;
        let blame = fx.service.blame("dolt", &orders()).await.expect("blame");
        assert_eq!(blame.len(), 1);
        assert_eq!(asked(&fx), ["blame orders"]);
    }

    #[test]
    fn the_log_view_keeps_its_frontend_json_shape() {
        let view = CommitLogView {
            connection_id: "dolt".into(),
            available: true,
            commits: vec![commit("abc")],
        };
        let json = serde_json::to_value(&view).expect("serialize");
        assert_eq!(json["available"], true);
        assert_eq!(json["commits"][0]["hash"], "abc");
        assert_eq!(json["commits"][0]["author"], "Sample Author");
    }
}
