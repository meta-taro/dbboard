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
mod dump;
mod error;
mod limits;
mod read_only;
mod restore;
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
pub use dump::{
    build_count, build_insert, build_select_page, plan_dump, run_dump, value_literal, DumpControl,
    DumpError, DumpOutcome, DumpPlan, DumpProgress, DumpResult, DumpSink, TableFailure, TablePlan,
    TableTruncation, DEFAULT_BACKUP_WARN_ROWS, INSERT_BATCH_ROWS, READ_PAGE_ROWS,
};
pub use error::{DbError, DbResult};
pub use limits::{too_many_rows_error, MAX_RESULT_ROWS};
pub use read_only::{
    check_read_only, classify_read_only, is_single_read_only_statement, ReadOnlyStatement,
    ReadOnlyViolation,
};
pub use restore::{
    classify_script, plan_restore, run_restore, split_statements, OnError, RestoreControl,
    RestoreError, RestoreOptions, RestoreOutcome, RestorePlan, RestoreProgress, RestoreResult,
    RestoreStatement, StatementFailure, StatementKind,
};
pub use row::{Column, QueryResult, Row};
pub use schema::{resolve_referenced_columns, ColumnInfo, ForeignKey, TableInfo, TableSchema};
pub use sort::{compare_values, sorted_row_order, SortKey};
pub use value::Value;
pub use write_back::{
    build_update_sql, CellValue, RowIdentity, RowKey, SqlDialect, UpdatePlan, WriteBackError,
};
