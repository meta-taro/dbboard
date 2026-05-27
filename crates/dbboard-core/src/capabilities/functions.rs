//! Stored function and procedure introspection capability.
//!
//! Phase 2 ships the marker only; methods land with the first adapter
//! that implements them.

pub trait FunctionIntrospection: Send + Sync {}
