//! Dump planning: per-table row counts, the huge-DB threshold, and the batch
//! sizes the orchestrator pages with (ADR-0049, slice b).
//!
//! A dump's preflight counts every table so the run has a progress
//! denominator and can warn (but not block) when the total is large. The
//! paging constants here are the read/emit granularities the later
//! orchestrator slice uses; they live next to the plan so the size policy is
//! in one place.

use crate::limits::MAX_RESULT_ROWS;
use crate::schema::TableInfo;

/// Row-count total above which the UI warns before running a dump
/// (ADR-0049 Decision 8, warn-and-allow). A constant for now; promoting it to
/// a persisted setting is left as a later, non-blocking change.
pub const DEFAULT_BACKUP_WARN_ROWS: u64 = 500_000;

/// Rows read per `SELECT` page during a dump. Kept below [`MAX_RESULT_ROWS`]
/// so the adapter's per-query cap never trips, while still amortizing the
/// round-trip cost over many rows (matters most for the HTTP-based D1
/// adapter, where one request per 500 rows would be needlessly chatty).
pub const READ_PAGE_ROWS: usize = MAX_RESULT_ROWS / 2;

/// Rows per emitted `INSERT` statement. Bounded under SQLite's default
/// compound-statement limit (500) so a dumped multi-row `INSERT` re-parses on
/// SQLite; Postgres tolerates far larger literal batches, so SQLite is the
/// binding constraint.
pub const INSERT_BATCH_ROWS: usize = 500;

// The paging invariants, enforced at compile time: a read page must stay
// under the adapter's per-query cap, and an INSERT batch must fit both within
// a read page and under SQLite's compound-statement limit.
const _: () = assert!(READ_PAGE_ROWS < MAX_RESULT_ROWS);
const _: () = assert!(INSERT_BATCH_ROWS <= 500);
const _: () = assert!(INSERT_BATCH_ROWS <= READ_PAGE_ROWS);

/// One table's contribution to a dump: which table, and how many rows a
/// preflight `COUNT(*)` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePlan {
    pub table: TableInfo,
    pub row_count: u64,
}

impl TablePlan {
    #[must_use]
    pub fn new(table: TableInfo, row_count: u64) -> Self {
        Self { table, row_count }
    }
}

/// The preflight plan for a whole connection: every table to dump with its
/// counted size. Drives the progress total and the huge-DB warning.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DumpPlan {
    pub tables: Vec<TablePlan>,
}

impl DumpPlan {
    #[must_use]
    pub fn new(tables: Vec<TablePlan>) -> Self {
        Self { tables }
    }

    /// Total rows across all tables — the denominator for progress.
    ///
    /// Saturating so an implausibly huge sum can never wrap (a wrapped total
    /// would silently disable the huge-DB warning).
    #[must_use]
    pub fn total_rows(&self) -> u64 {
        self.tables
            .iter()
            .fold(0_u64, |acc, t| acc.saturating_add(t.row_count))
    }

    /// `Some(total)` when the plan's total row count *exceeds* `threshold`
    /// (the warn-and-allow gate), otherwise `None`. A total exactly equal to
    /// the threshold does not exceed it.
    #[must_use]
    pub fn exceeds_threshold(&self, threshold: u64) -> Option<u64> {
        let total = self.total_rows();
        (total > threshold).then_some(total)
    }

    /// Whether the plan has no rows to dump at all (every table empty, or no
    /// tables) — the orchestrator can still emit each table's DDL.
    #[must_use]
    pub fn is_empty_data(&self) -> bool {
        self.total_rows() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_of(counts: &[u64]) -> DumpPlan {
        DumpPlan::new(
            counts
                .iter()
                .enumerate()
                .map(|(i, &n)| TablePlan::new(TableInfo::unqualified(format!("t{i}")), n))
                .collect(),
        )
    }

    #[test]
    fn total_rows_sums_the_per_table_counts() {
        assert_eq!(plan_of(&[10, 20, 5]).total_rows(), 35);
    }

    #[test]
    fn an_empty_plan_has_zero_total_and_no_data() {
        let plan = DumpPlan::default();
        assert_eq!(plan.total_rows(), 0);
        assert!(plan.is_empty_data());
        assert_eq!(plan.exceeds_threshold(DEFAULT_BACKUP_WARN_ROWS), None);
    }

    #[test]
    fn a_plan_of_only_empty_tables_reports_no_data() {
        let plan = plan_of(&[0, 0]);
        assert!(plan.is_empty_data());
        assert_eq!(plan.total_rows(), 0);
    }

    #[test]
    fn threshold_boundary_uses_strictly_greater_than() {
        let threshold = DEFAULT_BACKUP_WARN_ROWS; // 500_000
        assert_eq!(plan_of(&[threshold - 1]).exceeds_threshold(threshold), None);
        // Exactly at the threshold does not warn.
        assert_eq!(plan_of(&[threshold]).exceeds_threshold(threshold), None);
        // One over warns, and reports the actual total.
        assert_eq!(
            plan_of(&[threshold + 1]).exceeds_threshold(threshold),
            Some(threshold + 1)
        );
    }

    #[test]
    fn exceeds_threshold_reports_the_summed_total_not_a_single_table() {
        // Split across tables so the sum, not any one table, crosses.
        let plan = plan_of(&[300_000, 300_000]);
        assert_eq!(
            plan.exceeds_threshold(DEFAULT_BACKUP_WARN_ROWS),
            Some(600_000)
        );
    }

    #[test]
    fn total_rows_saturates_instead_of_wrapping() {
        let plan = DumpPlan::new(vec![
            TablePlan::new(TableInfo::unqualified("a"), u64::MAX),
            TablePlan::new(TableInfo::unqualified("b"), 1),
        ]);
        assert_eq!(plan.total_rows(), u64::MAX);
    }

    // The paging-constant invariants are asserted at compile time above
    // (`const _: () = assert!(...)`), so there is no runtime test for them.
}
