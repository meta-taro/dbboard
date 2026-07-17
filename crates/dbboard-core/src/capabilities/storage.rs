//! Object storage administration capability (e.g. Supabase Storage,
//! Cloudflare R2 via D1 bindings).
//!
//! Phase 2 ships the marker only; methods land with the first adapter
//! that implements them.

pub trait StorageAdmin: Send + Sync {}
