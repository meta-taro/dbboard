//! Domain layer for dbboard.
//!
//! This crate holds the value types, schema metadata, and error
//! taxonomy shared by every database adapter and the UI. It performs
//! no I/O and does not depend on any other workspace crate.
//!
//! The adapter trait itself is introduced in Phase 2 once the Turso
//! slice has revealed the concrete API surface (see
//! `docs/roadmap.md`). Phase 1 keeps the types Turso-shaped to avoid
//! premature abstraction.

mod error;
mod limits;
mod row;
mod schema;
mod value;

pub use error::{DbError, DbResult};
pub use limits::{too_many_rows_error, MAX_RESULT_ROWS};
pub use row::{Column, QueryResult, Row};
pub use schema::{ColumnInfo, TableInfo};
pub use value::Value;
