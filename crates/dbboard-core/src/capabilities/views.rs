//! View introspection capability.
//!
//! Phase 2 ships the marker only; methods (list views, describe view,
//! materialised-view refresh, …) land with the first adapter that
//! implements the capability (likely `dbboard-postgres` in Phase 3).

pub trait ViewIntrospection: Send + Sync {}
