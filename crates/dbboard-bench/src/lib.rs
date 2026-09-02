//! The measurement harness behind the v0.14 "Speed, measured" slot.
//!
//! `docs/roadmap.md` reserves that slot for startup, connect-and-browse and
//! large result sets, with one condition attached: *measurement lands before
//! any optimisation, so the numbers are comparable afterwards*. The order is
//! the point. Optimising first leaves nothing to compare against, and the
//! next regression arrives unannounced.
//!
//! This crate is the "before". It is a tool, not a library the app links:
//! nothing in `apps/` or the other `crates/` depends on it.

pub mod harness;
pub mod measure;
pub mod points;
