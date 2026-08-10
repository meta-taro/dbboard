//! Cloud Firestore adapter for dbboard.
//!
//! Firestore is the first non-SQL backend (ADR-0091). Three things follow from
//! that, and they are the whole design:
//!
//! 1. **The query text is a Firestore `StructuredQuery`, as JSON.** The trait's
//!    query parameter carries the adapter's own native form; there is no
//!    translation layer and no invented query language.
//! 2. **A document is a tree**, so a cell that holds one is
//!    `dbboard_core::Value::Json` (issue 0018). Scalars still land in the flat
//!    variants, so a Firestore string sorts and exports like any other string.
//! 3. **Read-only is structural, not parsed.** The REST API splits reads
//!    (`:runQuery`, `:listCollectionIds`) from writes (`:commit`) at the
//!    *endpoint*, so this crate simply contains no code path that builds a
//!    write URL — see [`endpoint`]. Nothing classifies a query string, so
//!    there is no classifier to get wrong.

mod adapter;
mod auth;
mod credentials;
mod document;
mod endpoint;
mod sample;

pub use adapter::{FirestoreAdapter, FirestoreConfig, FirestoreCredentials, DEFAULT_BASE_URL};
