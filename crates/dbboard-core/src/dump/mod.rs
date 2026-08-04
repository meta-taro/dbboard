//! Logical dump serialization (ADR-0049).
//!
//! Pure, I/O-free rendering of a database's data into SQL text. This slice
//! (a) owns the two leaf serializers every engine's dump path shares:
//!
//! - [`value_literal`] — a *total*, dialect-aware `Value` → SQL-literal
//!   function (the value-side counterpart to write-back's cell encoding).
//! - [`build_insert`] — multi-row `INSERT` assembly for one table.
//!
//! Both reuse `write_back`'s identifier/string quoting so escaping has a
//! single implementation across the write-back and dump paths.
//!
//! Slice (d) adds the paging/progress/cancellation orchestration:
//! [`build_select_page`] renders one keyset page and [`run_dump`] drives a
//! whole-connection dump through the [`DatabaseAdapter`](crate::DatabaseAdapter)
//! trait and a caller-supplied [`DumpSink`]. Engine-specific DDL generation
//! lives in each adapter (via `table_ddl`); this module stays I/O-free and
//! unit-testable in isolation, mirroring `export.rs`.

mod insert;
mod literal;
mod plan;
mod run;
mod select;

pub use insert::build_insert;
pub use literal::value_literal;
pub use plan::{DumpPlan, TablePlan, DEFAULT_BACKUP_WARN_ROWS, INSERT_BATCH_ROWS, READ_PAGE_ROWS};
pub use run::{
    plan_dump, run_dump, DumpControl, DumpError, DumpOutcome, DumpProgress, DumpResult, DumpSink,
    TableFailure, TableTruncation,
};
pub use select::{build_count, build_select_page};
