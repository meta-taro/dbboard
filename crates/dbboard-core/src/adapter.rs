//! The `DatabaseAdapter` trait — the contract every backend implements.
//!
//! Designed per ADR-0012: a small required surface (id, capabilities,
//! ping, `list_tables`, query) plus `Option<&dyn Capability>` accessors
//! for per-DB features. The required surface mirrors the methods every
//! Phase 1 adapter already exposes, so trait extraction is a shape
//! change rather than a behaviour change.
//!
//! The trait is object-safe (`async-trait` desugars to
//! `Pin<Box<dyn Future>>`) so the server can hold adapters as
//! `Arc<dyn DatabaseAdapter>` and grow the adapter set without
//! touching a closed enum.

use async_trait::async_trait;

use crate::capabilities::{
    AuthAdmin, Capabilities, FunctionIntrospection, RealtimeChannels, StorageAdmin,
    ViewIntrospection,
};
use crate::{check_read_only, DbError, DbResult, QueryResult, SqlDialect, TableInfo, TableSchema};

#[async_trait]
pub trait DatabaseAdapter: Send + Sync {
    /// Stable identifier for this adapter kind, used by the UI and
    /// `/capabilities` discovery (e.g. `"turso"`, `"d1"`, `"postgres"`).
    /// The value is constant per adapter, so the bound is `'static`.
    fn id(&self) -> &'static str;

    /// Capability flags advertised over `GET /capabilities`. Must agree
    /// with the `Option<&dyn ...>` accessors below — the per-adapter
    /// unit test in [`crate::adapter::tests`] checks the invariant.
    fn capabilities(&self) -> Capabilities;

    async fn ping(&self) -> DbResult<()>;

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>>;

    async fn query(&self, sql: &str) -> DbResult<QueryResult>;

    /// Execute `sql` under a read-only guarantee, returning at most
    /// `max_rows` rows (ADR-0046).
    ///
    /// This is the only execution path an untrusted agent (the MCP
    /// server) is given. Unlike [`query`](Self::query) it must never let
    /// a write, DDL, multi-statement batch, or locking read through, and
    /// it caps the result set by *truncating* rather than erroring.
    ///
    /// The default implementation is a portable, engine-agnostic guard:
    /// it rejects anything [`check_read_only`] cannot prove is a single
    /// read-only statement (parsed with the Postgres grammar, the
    /// richest of the supported dialects), runs the ordinary
    /// [`query`](Self::query) path, and truncates to `max_rows`.
    /// Adapters SHOULD override it to enforce read-only *at the engine* —
    /// a Postgres `BEGIN READ ONLY`, a libSQL `PRAGMA query_only`, or
    /// (for engines with no read-only mode, like D1) an explicit AST
    /// classification with the correct dialect.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Query`] if `sql` is not a single read-only
    /// statement, plus any error [`query`](Self::query) surfaces.
    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        check_read_only(sql, SqlDialect::Postgres)?;
        let mut result = self.query(sql).await?;
        result.truncate_rows(max_rows);
        Ok(result)
    }

    /// Full column + primary-key description for one table (ADR-0028).
    ///
    /// The default returns [`DbError::Capability`] so adapters that
    /// pre-date ADR-0028 compile unchanged and miss at runtime rather
    /// than at build time. Implementors must also flip
    /// [`Capabilities::has_describe_table`] — the UI will grey out
    /// schema-dependent features (ADR-0028 slice c) on adapters that
    /// only ship names.
    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        let _ = table;
        Err(DbError::Capability(
            "describe_table not supported by this adapter".into(),
        ))
    }

    fn views(&self) -> Option<&dyn ViewIntrospection> {
        None
    }

    fn functions(&self) -> Option<&dyn FunctionIntrospection> {
        None
    }

    fn auth(&self) -> Option<&dyn AuthAdmin> {
        None
    }

    fn storage(&self) -> Option<&dyn StorageAdmin> {
        None
    }

    fn realtime(&self) -> Option<&dyn RealtimeChannels> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::DatabaseAdapter;
    use crate::capabilities::{
        AuthAdmin, Capabilities, FunctionIntrospection, RealtimeChannels, StorageAdmin,
        ViewIntrospection,
    };
    use crate::{
        Column, ColumnInfo, DbError, DbResult, QueryResult, Row, TableInfo, TableSchema, Value,
    };

    struct NoopAdapter;

    #[async_trait]
    impl DatabaseAdapter for NoopAdapter {
        fn id(&self) -> &'static str {
            "noop"
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

    struct FullAdapter;

    impl ViewIntrospection for FullAdapter {}
    impl FunctionIntrospection for FullAdapter {}
    impl AuthAdmin for FullAdapter {}
    impl StorageAdmin for FullAdapter {}
    impl RealtimeChannels for FullAdapter {}

    #[async_trait]
    impl DatabaseAdapter for FullAdapter {
        fn id(&self) -> &'static str {
            "full"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                has_views: true,
                has_functions: true,
                has_auth: true,
                has_storage: true,
                has_realtime: true,
                has_describe_table: true,
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
        async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
            Ok(TableSchema {
                table: table.clone(),
                columns: vec![ColumnInfo {
                    name: "id".into(),
                    declared_type: Some("INTEGER".into()),
                    nullable: false,
                    primary_key: true,
                    ordinal: 1,
                    default_value: None,
                }],
                primary_key: vec!["id".into()],
            })
        }
        fn views(&self) -> Option<&dyn ViewIntrospection> {
            Some(self)
        }
        fn functions(&self) -> Option<&dyn FunctionIntrospection> {
            Some(self)
        }
        fn auth(&self) -> Option<&dyn AuthAdmin> {
            Some(self)
        }
        fn storage(&self) -> Option<&dyn StorageAdmin> {
            Some(self)
        }
        fn realtime(&self) -> Option<&dyn RealtimeChannels> {
            Some(self)
        }
    }

    #[test]
    fn id_is_exposed_for_discovery() {
        assert_eq!(NoopAdapter.id(), "noop");
        assert_eq!(FullAdapter.id(), "full");
    }

    #[test]
    fn no_capabilities_means_every_accessor_returns_none() {
        let adapter = NoopAdapter;
        let caps = adapter.capabilities();

        assert_eq!(caps.has_views, adapter.views().is_some());
        assert_eq!(caps.has_functions, adapter.functions().is_some());
        assert_eq!(caps.has_auth, adapter.auth().is_some());
        assert_eq!(caps.has_storage, adapter.storage().is_some());
        assert_eq!(caps.has_realtime, adapter.realtime().is_some());

        assert!(adapter.views().is_none());
        assert!(adapter.functions().is_none());
        assert!(adapter.auth().is_none());
        assert!(adapter.storage().is_none());
        assert!(adapter.realtime().is_none());
    }

    #[test]
    fn full_capabilities_means_every_accessor_returns_some() {
        let adapter = FullAdapter;
        let caps = adapter.capabilities();

        assert_eq!(caps.has_views, adapter.views().is_some());
        assert_eq!(caps.has_functions, adapter.functions().is_some());
        assert_eq!(caps.has_auth, adapter.auth().is_some());
        assert_eq!(caps.has_storage, adapter.storage().is_some());
        assert_eq!(caps.has_realtime, adapter.realtime().is_some());

        assert!(adapter.views().is_some());
        assert!(adapter.functions().is_some());
        assert!(adapter.auth().is_some());
        assert!(adapter.storage().is_some());
        assert!(adapter.realtime().is_some());
    }

    #[tokio::test]
    async fn default_describe_table_surfaces_capability_error() {
        let err = NoopAdapter
            .describe_table(&TableInfo::unqualified("users"))
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Capability(_)));
        assert_eq!(
            err.message(),
            "describe_table not supported by this adapter"
        );
    }

    #[tokio::test]
    async fn overridden_describe_table_echoes_the_requested_table() {
        let table = TableInfo::qualified("public", "users");
        let schema = FullAdapter.describe_table(&table).await.unwrap();
        assert_eq!(schema.table, table);
        assert_eq!(schema.columns.len(), 1);
        assert_eq!(schema.primary_key, vec!["id".to_owned()]);
    }

    #[test]
    fn describe_table_capability_flag_matches_support() {
        assert!(!NoopAdapter.capabilities().has_describe_table);
        assert!(FullAdapter.capabilities().has_describe_table);
    }

    /// An adapter whose `query` always returns `row_count` single-cell
    /// rows, used to exercise the default `query_read_only` truncation
    /// without a real database.
    struct CountingAdapter {
        row_count: usize,
    }

    #[async_trait]
    impl DatabaseAdapter for CountingAdapter {
        fn id(&self) -> &'static str {
            "counting"
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
            Ok(QueryResult {
                columns: vec![Column {
                    name: "n".into(),
                    declared_type: None,
                }],
                rows: (0..self.row_count)
                    .map(|i| Row::new(vec![Value::Integer(i64::try_from(i).unwrap())]))
                    .collect(),
                rows_affected: 0,
            })
        }
    }

    #[tokio::test]
    async fn default_query_read_only_rejects_a_write() {
        let err = NoopAdapter
            .query_read_only("DELETE FROM t", 100)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        assert!(err.message().contains("read-only"), "message: {err}");
    }

    #[tokio::test]
    async fn default_query_read_only_rejects_multi_statement_batch() {
        // The simple-query-protocol hazard: both statements would run.
        let err = NoopAdapter
            .query_read_only("SELECT 1; DROP TABLE t", 100)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
    }

    #[tokio::test]
    async fn default_query_read_only_allows_a_select() {
        let result = NoopAdapter.query_read_only("SELECT 1", 100).await.unwrap();
        assert!(result.rows.is_empty());
    }

    #[tokio::test]
    async fn default_query_read_only_truncates_to_max_rows() {
        let adapter = CountingAdapter { row_count: 50 };
        let result = adapter
            .query_read_only("SELECT n FROM t", 10)
            .await
            .unwrap();
        assert_eq!(result.rows.len(), 10);
        assert_eq!(result.columns.len(), 1);
    }

    #[test]
    fn adapter_is_object_safe_behind_arc_dyn() {
        // Compile-time check: if `DatabaseAdapter` were not object-safe,
        // this line would not type-check.
        let adapter: Arc<dyn DatabaseAdapter> = Arc::new(NoopAdapter);
        assert_eq!(adapter.id(), "noop");
    }
}
