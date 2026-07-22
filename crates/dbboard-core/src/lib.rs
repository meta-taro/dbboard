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
mod read_only;
mod row;
mod schema;
mod sort;
mod value;
mod write_back;

pub use adapter::DatabaseAdapter;
pub use capabilities::{
    AuthAdmin, Capabilities, FunctionIntrospection, RealtimeChannels, StorageAdmin,
    ViewIntrospection,
};
pub use error::{DbError, DbResult};
pub use limits::{too_many_rows_error, MAX_RESULT_ROWS};
pub use read_only::{
    check_read_only, classify_read_only, is_single_read_only_statement, ReadOnlyStatement,
    ReadOnlyViolation,
};
pub use row::{Column, QueryResult, Row};
pub use schema::{ColumnInfo, TableInfo, TableSchema};
pub use sort::{compare_values, sorted_row_order, SortKey};
pub use value::Value;
pub use write_back::{
    build_update_sql, CellValue, RowIdentity, RowKey, SqlDialect, UpdatePlan, WriteBackError,
};
