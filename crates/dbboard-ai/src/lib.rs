//! AI provider trait layer for dbboard (ADR-0023).
//!
//! This crate defines the `AiProvider` trait and the value types its
//! Stage 1 methods exchange. It performs no I/O and depends only on
//! [`dbboard_core`] (for `TableInfo`, re-exported here so downstream
//! provider crates do not need a direct dependency on `dbboard-core`).
//!
//! Concrete providers — starting with `dbboard-anthropic` — live in
//! sibling crates that depend on this one only, exactly as the DB
//! adapter crates depend on `dbboard-core` only (ADR-0002 /
//! ADR-0023 Decision 1).

mod capabilities;
mod error;
mod provider;
mod request;

pub use capabilities::AiCapabilities;
pub use dbboard_core::TableInfo;
pub use error::{AiError, AiResult};
pub use provider::AiProvider;
pub use request::{AiResponse, ExplainRequest, SuggestRequest};
