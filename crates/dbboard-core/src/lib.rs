//! Domain layer for dbboard.
//!
//! This crate holds the value types, schema metadata, error taxonomy,
//! and adapter contract shared by every database adapter and the UI.
//! It performs no I/O and does not depend on any other workspace crate.
//!
//! Phase 2 introduces the adapter trait and the [`Capabilities`]
//! discovery struct per ADR-0012. Optional per-DB features attach as
//! marker traits in [`capabilities`].

mod adapter;
mod capabilities;
mod error;
mod limits;
mod row;
mod schema;
mod value;

pub use adapter::DatabaseAdapter;
pub use capabilities::{
    AuthAdmin, Capabilities, FunctionIntrospection, RealtimeChannels, StorageAdmin,
    ViewIntrospection,
};
pub use error::{DbError, DbResult};
pub use limits::{too_many_rows_error, MAX_RESULT_ROWS};
pub use row::{Column, QueryResult, Row};
pub use schema::{ColumnInfo, TableInfo};
pub use value::Value;
