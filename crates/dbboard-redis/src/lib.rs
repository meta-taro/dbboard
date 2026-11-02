//! Read-only adapter for Redis-compatible stores (ADR-0166).
//!
//! Redis, Valkey, Amazon `MemoryDB`, Amazon `ElastiCache` and Upstash all speak
//! RESP, so one adapter serves them; the connection list's label changes the
//! defaults a form fills in, not the code path.
//!
//! Like `MongoDB`, nothing about the wire separates a read from a write, so
//! the guarantee is a classifier: [`read_only`] is an allowlist of commands
//! and of their arguments, pure and I/O-free, and every path from caller text
//! to the server runs through it.

mod adapter;
mod command;
mod read_only;
mod reply;

pub use adapter::{RedisAdapter, RedisConfig};
pub use command::{split, SplitError};
pub use read_only::{classify, ReadPlan, Shape, SizeGuard, Violation, MAX_SCAN_COUNT};
