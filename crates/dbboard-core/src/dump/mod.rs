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
//! single implementation across the write-back and dump paths. Engine-
//! specific DDL generation and the paging/progress/cancellation orchestration
//! land in later slices; keeping this rendering here — with no adapter or UI
//! dependency — mirrors `export.rs` and keeps it unit-testable in isolation.

mod insert;
mod literal;

pub use insert::build_insert;
pub use literal::value_literal;
