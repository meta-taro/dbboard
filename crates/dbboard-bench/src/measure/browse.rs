//! Opening a database and reading enough of it to show a table.
//!
//! In-memory libSQL, so nothing here includes a network. That is the point:
//! a hosted database's numbers would describe somebody's link on the day, and
//! would not reproduce on the next run, let alone the next machine. What is
//! left is the adapter and the schema handling, which is the part this repo
//! can actually make faster.

use std::time::Instant;

use dbboard_core::{build_select_page, DatabaseAdapter, SqlDialect, TableInfo, Value};
use dbboard_turso::TursoAdapter;

use super::{BenchResult, Sampler};
use crate::harness::Reading;

/// Tables in the fixture schema, matching the `_20` in the point ids.
const TABLE_COUNT: usize = 20;
/// Rows in the table the first page is read from — more than one page, so
/// `LIMIT 100` is a real limit rather than "everything there is".
const SUBJECT_ROWS: usize = 500;
/// The table the per-table points describe. Twelve columns and two outgoing
/// references: a middling real table, not a two-column toy.
const SUBJECT: &str = "subject";

/// Build the fixture schema on a fresh in-memory database.
async fn seeded() -> BenchResult<TursoAdapter> {
    let adapter = TursoAdapter::connect_local(":memory:").await?;
    adapter
        .execute("CREATE TABLE ref_a (id INTEGER PRIMARY KEY, label TEXT)")
        .await?;
    adapter
        .execute("CREATE TABLE ref_b (id INTEGER PRIMARY KEY, label TEXT)")
        .await?;
    adapter
        .execute(&format!(
            "CREATE TABLE {SUBJECT} (
                 id        INTEGER PRIMARY KEY,
                 a_id      INTEGER,
                 b_id      INTEGER,
                 name      TEXT NOT NULL,
                 code      TEXT,
                 amount    REAL,
                 quantity  INTEGER,
                 note      TEXT,
                 flag      INTEGER,
                 created   TEXT,
                 updated   TEXT,
                 payload   BLOB,
                 FOREIGN KEY (a_id) REFERENCES ref_a (id),
                 FOREIGN KEY (b_id) REFERENCES ref_b (id)
             )"
        ))
        .await?;
    // Filler, so `list_tables` has TABLE_COUNT to enumerate rather than three.
    for i in 0..TABLE_COUNT - 3 {
        adapter
            .execute(&format!(
                "CREATE TABLE filler_{i:02} (id INTEGER PRIMARY KEY, label TEXT, value REAL)"
            ))
            .await?;
    }
    adapter
        .execute("INSERT INTO ref_a (id, label) VALUES (1, 'a')")
        .await?;
    adapter
        .execute("INSERT INTO ref_b (id, label) VALUES (1, 'b')")
        .await?;
    for i in 0..SUBJECT_ROWS {
        adapter
            .execute(&format!(
                "INSERT INTO {SUBJECT}
                   (id, a_id, b_id, name, code, amount, quantity, note, flag, created, updated, payload)
                 VALUES
                   ({i}, 1, 1, 'row {i}', 'code-{i:04}', {i}.5, {i}, 'note for row {i}', {},
                    '2026-09-02', '2026-09-02', NULL)",
                i % 2
            ))
            .await?;
    }
    Ok(adapter)
}

/// Time the connect-and-browse group into `out`.
///
/// # Errors
///
/// Returns any connection, DDL or query failure.
pub async fn measure(out: &mut Vec<Reading>) -> BenchResult<()> {
    // ---- browse/connect_memory ------------------------------------------
    //
    // A fresh database each iteration, so this is the cost of opening one
    // and running the read-only probe, not of reopening a warm handle.
    let mut s = Sampler::new("browse/connect_memory");
    while s.wants_more() {
        let t = Instant::now();
        let adapter = TursoAdapter::connect_local(":memory:").await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(adapter));
    }
    out.extend(s.finish());

    let adapter = seeded().await?;
    let subject = TableInfo::unqualified(SUBJECT);

    // ---- browse/list_tables_20 ------------------------------------------
    let mut s = Sampler::new("browse/list_tables_20");
    while s.wants_more() {
        let t = Instant::now();
        let tables = adapter.list_tables().await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(tables));
    }
    out.extend(s.finish());

    // ---- browse/describe_table_12col ------------------------------------
    let mut s = Sampler::new("browse/describe_table_12col");
    while s.wants_more() {
        let t = Instant::now();
        let schema = adapter.describe_table(&subject).await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(schema));
    }
    out.extend(s.finish());

    // ---- browse/foreign_keys --------------------------------------------
    let mut s = Sampler::new("browse/foreign_keys");
    while s.wants_more() {
        let t = Instant::now();
        let keys = adapter.foreign_keys(&subject).await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(keys));
    }
    out.extend(s.finish());

    // ---- browse/first_page_100 ------------------------------------------
    let page = format!("SELECT * FROM {SUBJECT} LIMIT 100");
    let mut s = Sampler::new("browse/first_page_100");
    while s.wants_more() {
        let t = Instant::now();
        let rows = adapter.query(&page).await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(rows));
    }
    out.extend(s.finish());

    // ---- browse/next_page_100 -------------------------------------------
    //
    // The *deepest* page of the 500-row table, not the second one. `OFFSET`
    // is cheap at page two and expensive at page five, so a measurement that
    // only ever read page two would not notice the difference the keyset
    // cursor was chosen for (ADR-0145).
    //
    // Run through `adapter.query` like `first_page_100` above, rather than
    // through `browse_page`, so the only difference between the two numbers
    // is the cursor — not the read-only path wrapped around it.
    let deep = build_select_page(
        &subject,
        &["id".to_owned()],
        SqlDialect::Sqlite,
        100,
        Some(&[Value::Integer(
            i64::try_from(SUBJECT_ROWS - 100).expect("fits"),
        )]),
    );
    let mut s = Sampler::new("browse/next_page_100");
    while s.wants_more() {
        let t = Instant::now();
        let rows = adapter.query(&deep).await?;
        s.record(t.elapsed());
        drop(std::hint::black_box(rows));
    }
    out.extend(s.finish());

    Ok(())
}
