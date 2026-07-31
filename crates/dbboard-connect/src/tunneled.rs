//! [`TunneledAdapter`] — a decorator that keeps an SSH tunnel alive for as
//! long as the adapter it fronts (ADR-0069).
//!
//! When a connection forwards through a bastion, [`crate::connect_adapter`]
//! opens the tunnel, connects the ordinary TCP adapter to the loopback
//! forward, and wraps the pair here. Every [`DatabaseAdapter`] call delegates
//! straight to the inner adapter; the tunnel guard rides along untouched and
//! its `Drop` tears the forward down when the last `Arc` to this adapter goes
//! away. The decorator adds no behaviour — it only ties two lifetimes
//! together so the forward outlives every query but not the connection.

use std::sync::Arc;

use async_trait::async_trait;
use dbboard_core::{
    AuthAdmin, FunctionIntrospection, RealtimeChannels, StorageAdmin, ViewIntrospection,
};
use dbboard_core::{
    Capabilities, DatabaseAdapter, DbResult, ForeignKey, QueryResult, TableInfo, TableSchema,
};
use dbboard_tunnel::SshTunnel;

/// An adapter that owns the SSH tunnel its inner adapter connects through.
/// Delegates the whole [`DatabaseAdapter`] surface; exists only to bind the
/// tunnel's lifetime to the adapter's.
pub struct TunneledAdapter {
    inner: Arc<dyn DatabaseAdapter>,
    // The forward stays up while this is held; dropping it aborts the accept
    // loop. Boxed as an opaque guard so the decorator does not care what keeps
    // the forward alive — a live `SshTunnel` in production, a no-op in tests.
    _guard: Box<dyn Send + Sync>,
}

impl TunneledAdapter {
    /// Wrap `inner` and take ownership of `tunnel`, keeping the forward open
    /// for the adapter's lifetime.
    #[must_use]
    pub fn new(inner: Arc<dyn DatabaseAdapter>, tunnel: SshTunnel) -> Self {
        Self {
            inner,
            _guard: Box::new(tunnel),
        }
    }
}

#[async_trait]
impl DatabaseAdapter for TunneledAdapter {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn ping(&self) -> DbResult<()> {
        self.inner.ping().await
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        self.inner.list_tables().await
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        self.inner.query(sql).await
    }

    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        self.inner.query_read_only(sql, max_rows).await
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        self.inner.describe_table(table).await
    }

    async fn foreign_keys(&self, table: &TableInfo) -> DbResult<Vec<ForeignKey>> {
        self.inner.foreign_keys(table).await
    }

    async fn table_ddl(&self, table: &TableInfo) -> DbResult<String> {
        self.inner.table_ddl(table).await
    }

    async fn execute(&self, sql: &str) -> DbResult<u64> {
        self.inner.execute(sql).await
    }

    async fn execute_in_transaction(&self, statements: &[String]) -> DbResult<()> {
        self.inner.execute_in_transaction(statements).await
    }

    fn views(&self) -> Option<&dyn ViewIntrospection> {
        self.inner.views()
    }

    fn functions(&self) -> Option<&dyn FunctionIntrospection> {
        self.inner.functions()
    }

    fn auth(&self) -> Option<&dyn AuthAdmin> {
        self.inner.auth()
    }

    fn storage(&self) -> Option<&dyn StorageAdmin> {
        self.inner.storage()
    }

    fn realtime(&self) -> Option<&dyn RealtimeChannels> {
        self.inner.realtime()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Inner adapter that records how often each delegated method was called
    /// and returns identifiable values, so the decorator's delegation is
    /// observable without a real database or a real tunnel.
    struct SpyAdapter {
        queries: AtomicUsize,
    }

    #[async_trait]
    impl DatabaseAdapter for SpyAdapter {
        fn id(&self) -> &'static str {
            "spy"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                has_execute: true,
                ..Capabilities::default()
            }
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(vec![TableInfo::unqualified("widgets")])
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            self.queries.fetch_add(1, Ordering::SeqCst);
            Ok(QueryResult::empty())
        }
        async fn execute(&self, _sql: &str) -> DbResult<u64> {
            Ok(7)
        }
    }

    /// Build a `TunneledAdapter` whose guard is a no-op, so delegation can be
    /// tested without opening a real SSH tunnel.
    fn tunneled(inner: Arc<dyn DatabaseAdapter>) -> TunneledAdapter {
        TunneledAdapter {
            inner,
            _guard: Box::new(()),
        }
    }

    #[test]
    fn id_and_capabilities_pass_through() {
        let inner = Arc::new(SpyAdapter {
            queries: AtomicUsize::new(0),
        });
        let adapter = tunneled(inner);
        assert_eq!(adapter.id(), "spy");
        assert!(adapter.capabilities().has_execute);
    }

    #[tokio::test]
    async fn query_delegates_to_the_inner_adapter() {
        let inner = Arc::new(SpyAdapter {
            queries: AtomicUsize::new(0),
        });
        let adapter = tunneled(inner.clone());
        adapter.query("SELECT 1").await.unwrap();
        adapter.query("SELECT 2").await.unwrap();
        assert_eq!(inner.queries.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn list_tables_and_execute_pass_through() {
        let inner = Arc::new(SpyAdapter {
            queries: AtomicUsize::new(0),
        });
        let adapter = tunneled(inner);
        let tables = adapter.list_tables().await.unwrap();
        assert_eq!(tables, vec![TableInfo::unqualified("widgets")]);
        assert_eq!(
            adapter
                .execute("INSERT INTO widgets VALUES (1)")
                .await
                .unwrap(),
            7
        );
    }

    #[test]
    fn dropping_the_guard_runs_on_drop() {
        // The guard's Drop must fire when the adapter is dropped; prove it with
        // a guard that flips a flag, standing in for SshTunnel's accept-loop
        // teardown.
        use std::sync::atomic::AtomicBool;
        static DROPPED: AtomicBool = AtomicBool::new(false);
        struct DropSpy;
        impl Drop for DropSpy {
            fn drop(&mut self) {
                DROPPED.store(true, Ordering::SeqCst);
            }
        }
        let inner = Arc::new(SpyAdapter {
            queries: AtomicUsize::new(0),
        });
        let adapter = TunneledAdapter {
            inner,
            _guard: Box::new(DropSpy),
        };
        assert!(!DROPPED.load(Ordering::SeqCst));
        drop(adapter);
        assert!(DROPPED.load(Ordering::SeqCst));
    }
}
