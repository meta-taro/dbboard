//! Logical restore / import (ADR-0051).
//!
//! The read-side counterpart to [`dump`](crate::dump): it takes a `.sql`
//! script and applies it to a target connection. The design mirrors the
//! dump pipeline — a pure, I/O-free core in this crate driven by the
//! `DatabaseAdapter` trait, with the app supplying the file source and the
//! progress/cancellation channel — and it accepts *any* `.sql`, not only
//! dbboard's own dumps.
//!
//! Restore is a two-layer pipeline:
//!
//! - **Layer 1 — [`split_statements`]**: a lexical, dialect-agnostic
//!   splitter that carves a script into individual statements, correctly
//!   ignoring `;` inside strings, quoted identifiers, dollar-quoted bodies,
//!   and comments. It classifies nothing and rejects nothing.
//! - **Layer 2 (a later slice)**: an sqlparser-based classifier that labels
//!   each statement and downgrades gracefully when a statement will not
//!   parse, so a best-effort restore of hand-written SQL still runs.
//!
//! This slice lands Layer 1 only.

mod split;

pub use split::split_statements;
