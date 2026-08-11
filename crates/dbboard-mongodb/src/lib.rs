//! `MongoDB` adapter for dbboard.
//!
//! `MongoDB` is the second non-SQL backend (ADR-0091), and it is the expensive
//! one. Firestore could lean on its transport — reads and writes are different
//! REST endpoints, so an adapter that never builds a write URL is read-only by
//! construction. `MongoDB` has no such split: every command travels the same
//! `runCommand` path, and the wire carries no hint about which of them mutate.
//!
//! So the read-only guarantee here is a *classifier*, and the classifier is
//! this crate's safety-critical piece. It lives in [`read_only`], is pure and
//! I/O-free, and is written to the same bar as `dbboard_core::read_only`:
//! fail closed on anything it cannot prove read-only, and never decide by
//! looking at the shape of a string.

mod adapter;
mod command;
mod document;
mod read_only;
mod sample;

pub use adapter::{MongoAdapter, MongoConfig};
pub use read_only::{
    check_read_only, classify_read_only, CommandDoc, CommandViolation, ReadCommand,
};
