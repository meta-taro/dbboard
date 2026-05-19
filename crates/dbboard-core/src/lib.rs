//! Domain layer for dbboard.
//!
//! This crate holds the adapter trait, value types, and errors shared
//! by every database adapter and the UI. It performs no I/O.
//!
//! The concrete trait and types land in Phase 2 (see
//! `docs/roadmap.md`). Phase 1 ships Turso-shaped types directly to
//! avoid premature abstraction.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        // Placeholder until Phase 2 introduces the adapter trait.
    }
}
