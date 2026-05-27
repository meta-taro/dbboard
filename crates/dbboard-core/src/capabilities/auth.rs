//! User and role administration capability (e.g. Supabase auth).
//!
//! Phase 2 ships the marker only; methods land with `dbboard-supabase`
//! in Phase 3.

pub trait AuthAdmin: Send + Sync {}
