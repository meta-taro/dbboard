//! Realtime change-feed capability (e.g. Supabase Realtime, Postgres
//! logical replication, libSQL replication streams).
//!
//! Phase 2 ships the marker only; methods land with the first adapter
//! that implements them.

pub trait RealtimeChannels: Send + Sync {}
