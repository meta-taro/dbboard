//! In-memory query history (ADR-0014).
//!
//! A bounded, newest-first ring buffer of recently-run SQL statements,
//! owned by the egui app. The store is intentionally trivial: it tracks
//! only the SQL text — no result metadata, no timing. Persistence is
//! deferred to a Stage 2 ADR (see ADR-0014).

use std::collections::VecDeque;

/// Default cap used by [`HistoryStore::default`]. Chosen for UI
/// ergonomics, not correctness — 100 short SQL strings is plenty for a
/// session and stays well below any meaningful memory budget.
pub const DEFAULT_CAPACITY: usize = 100;

/// One remembered SQL statement. Stage 1 stores only the text; later
/// stages may add result metadata or timing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub sql: String,
}

/// Bounded, newest-first ring buffer of [`HistoryEntry`].
#[derive(Debug)]
pub struct HistoryStore {
    entries: VecDeque<HistoryEntry>,
    capacity: usize,
}

impl HistoryStore {
    /// Build a store with the given cap. A `capacity` of 0 is clamped to
    /// 1 — a zero-capacity history is a footgun, not a feature.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a SQL statement onto the front of the history.
    ///
    /// - Empty (after trimming) inputs are ignored.
    /// - If `sql` matches the most recent entry exactly, the call is a
    ///   no-op (adjacent dedup — see ADR-0014). Non-adjacent repeats
    ///   are kept.
    /// - When the buffer is full, the oldest entry is dropped.
    pub fn push(&mut self, sql: impl Into<String>) {
        let sql = sql.into();
        if sql.trim().is_empty() {
            return;
        }
        if self.entries.front().is_some_and(|head| head.sql == sql) {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_back();
        }
        self.entries.push_front(HistoryEntry { sql });
    }

    /// Iterate over entries newest-first.
    pub fn iter(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryStore, DEFAULT_CAPACITY};

    #[test]
    fn empty_by_default() {
        let h = HistoryStore::default();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.capacity(), DEFAULT_CAPACITY);
    }

    #[test]
    fn push_adds_one_entry() {
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        assert_eq!(h.len(), 1);
        assert_eq!(h.iter().next().unwrap().sql, "SELECT 1");
    }

    #[test]
    fn iter_is_newest_first() {
        let mut h = HistoryStore::new(10);
        h.push("first");
        h.push("second");
        h.push("third");

        let collected: Vec<&str> = h.iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(collected, vec!["third", "second", "first"]);
    }

    #[test]
    fn adjacent_duplicate_is_collapsed() {
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        h.push("SELECT 1");
        h.push("SELECT 1");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn non_adjacent_duplicate_is_kept() {
        let mut h = HistoryStore::new(10);
        h.push("SELECT 1");
        h.push("SELECT 2");
        h.push("SELECT 1");
        assert_eq!(h.len(), 3);

        let collected: Vec<&str> = h.iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(collected, vec!["SELECT 1", "SELECT 2", "SELECT 1"]);
    }

    #[test]
    fn capacity_drops_oldest_entry() {
        let mut h = HistoryStore::new(3);
        h.push("one");
        h.push("two");
        h.push("three");
        h.push("four");

        assert_eq!(h.len(), 3);
        let collected: Vec<&str> = h.iter().map(|e| e.sql.as_str()).collect();
        assert_eq!(collected, vec!["four", "three", "two"]);
    }

    #[test]
    fn empty_or_whitespace_input_is_ignored() {
        let mut h = HistoryStore::new(10);
        h.push("");
        h.push("   ");
        h.push("\t\n");
        assert!(h.is_empty());
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut h = HistoryStore::new(0);
        assert_eq!(h.capacity(), 1);
        h.push("first");
        h.push("second");
        assert_eq!(h.len(), 1);
        assert_eq!(h.iter().next().unwrap().sql, "second");
    }
}
