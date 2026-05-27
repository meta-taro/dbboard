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
use crate::{DbResult, QueryResult, TableInfo};

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
    use crate::{DbResult, QueryResult, TableInfo};

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

    #[test]
    fn adapter_is_object_safe_behind_arc_dyn() {
        // Compile-time check: if `DatabaseAdapter` were not object-safe,
        // this line would not type-check.
        let adapter: Arc<dyn DatabaseAdapter> = Arc::new(NoopAdapter);
        assert_eq!(adapter.id(), "noop");
    }
}
