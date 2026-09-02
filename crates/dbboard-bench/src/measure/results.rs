//! Carrying a full result set to the frontend.
//!
//! `MAX_RESULT_ROWS` is 10,000 and every adapter loads a result fully into
//! memory (there is no streaming yet), so 10,000 rows is the largest thing
//! the UI can ever be handed. Each point here sits between a query finishing
//! and rows appearing in the grid.
//!
//! `result/serialize_10k` is the one to watch. `QueryResult` reaches the
//! `SvelteKit` frontend as JSON over Tauri IPC, so that number *is* the IPC
//! cost — and it is paid on every query, not just the big ones.

use std::time::Instant;

use dbboard_core::{sorted_row_order, DatabaseAdapter, QueryResult, SortKey, MAX_RESULT_ROWS};
use dbboard_turso::TursoAdapter;

use super::{BenchResult, Sampler};
use crate::harness::Reading;

/// Eight columns of mixed type, so serialisation is not measured against a
/// column of identical short integers.
const COLUMNS: usize = 8;
/// The soft cap the read-only path truncates to (ADR-0046).
const READ_ONLY_CAP: usize = 100;

/// Rows per multi-row INSERT while seeding. 10,000 round trips through
/// `execute` would make setting the fixture up dominate the run.
const SEED_BATCH: usize = 500;

/// A table holding [`MAX_RESULT_ROWS`] rows of [`COLUMNS`] columns.
async fn seeded() -> BenchResult<TursoAdapter> {
    let adapter = TursoAdapter::connect_local(":memory:").await?;
    adapter
        .execute(
            "CREATE TABLE wide (
                 id       INTEGER PRIMARY KEY,
                 name     TEXT,
                 code     TEXT,
                 amount   REAL,
                 quantity INTEGER,
                 note     TEXT,
                 flag     INTEGER,
                 absent   TEXT
             )",
        )
        .await?;
    for start in (0..MAX_RESULT_ROWS).step_by(SEED_BATCH) {
        let values: Vec<String> = (start..start + SEED_BATCH)
            .map(|i| {
                format!(
                    "({i}, 'name {i}', 'code-{i:05}', {i}.25, {i}, 'a note of some length for row {i}', {}, NULL)",
                    i % 2
                )
            })
            .collect();
        adapter
            .execute(&format!("INSERT INTO wide VALUES {}", values.join(", ")))
            .await?;
    }
    Ok(adapter)
}

/// Time the large-result-set group into `out`.
///
/// # Errors
///
/// Returns any connection, DDL or query failure.
pub async fn measure(out: &mut Vec<Reading>) -> BenchResult<()> {
    let adapter = seeded().await?;
    let sql = format!("SELECT * FROM wide LIMIT {MAX_RESULT_ROWS}");

    // ---- result/query_10k -----------------------------------------------
    let mut s = Sampler::new("result/query_10k");
    while s.wants_more() {
        let t = Instant::now();
        let result = adapter.query(&sql).await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(result));
    }
    out.extend(s.finish());

    // The other three all operate on a result that is already in memory, so
    // they share one — building it inside the timed region would measure the
    // query three more times.
    let base: QueryResult = adapter.query(&sql).await?;
    debug_assert_eq!(base.rows.len(), MAX_RESULT_ROWS);
    debug_assert_eq!(base.columns.len(), COLUMNS);

    // ---- result/serialize_10k -------------------------------------------
    let mut s = Sampler::new("result/serialize_10k");
    while s.wants_more() {
        let t = Instant::now();
        let json = serde_json::to_string(&base)?;
        s.record(t.elapsed());
        drop(std::hint::black_box(json));
    }
    out.extend(s.finish());

    // ---- result/sort_10k ------------------------------------------------
    //
    // On the text column rather than the integer key: comparing strings is
    // what the grid actually asks for when someone clicks a name header, and
    // it is the more expensive of the two.
    let keys = [SortKey {
        column: 1,
        ascending: true,
    }];
    let mut s = Sampler::new("result/sort_10k");
    while s.wants_more() {
        let t = Instant::now();
        let order = sorted_row_order(&base.rows, &keys);
        s.record(t.elapsed());
        drop(std::hint::black_box(order));
    }
    out.extend(s.finish());

    // ---- result/truncate_10k_to_100 -------------------------------------
    //
    // The clone is outside the timed region on purpose: cloning 10,000 rows
    // costs far more than dropping 9,900 of them, and timing both together
    // would report the clone.
    let mut s = Sampler::new("result/truncate_10k_to_100");
    while s.wants_more() {
        let mut copy = base.clone();
        let t = Instant::now();
        copy.truncate_rows(READ_ONLY_CAP);
        s.record(t.elapsed());
        drop(std::hint::black_box(copy));
    }
    out.extend(s.finish());

    Ok(())
}
