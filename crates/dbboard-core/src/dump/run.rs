//! The logical-dump orchestrator (ADR-0049, slice d).
//!
//! [`run_dump`] drives a whole-connection dump: for each planned table it
//! emits the table's DDL (when the adapter can produce it), then pages
//! through its rows keyset-style, emitting batched `INSERT`s. It reports
//! progress after every page and checks for cancellation between pages, so
//! a large dump stays responsive and interruptible.
//!
//! Two failure policies, matching ADR-0049 Decisions 9 and 10:
//! - A per-table adapter error (DDL, describe, or a data page) is *non-
//!   fatal*: it is recorded in [`DumpOutcome::failures`] and the dump moves
//!   on to the next table. One unreadable table never aborts the run.
//! - A failure of the output [`DumpSink`] *is* fatal — if the dump cannot
//!   be written there is nothing to continue for — and returns [`DumpError`].
//!
//! The orchestrator holds no I/O of its own: it reads through the
//! [`DatabaseAdapter`] trait and writes through the caller's [`DumpSink`],
//! so it lives in the domain layer and is testable with a fake adapter.

use crate::adapter::DatabaseAdapter;
use crate::capabilities::Capabilities;
use crate::dump::{build_insert, build_select_page, INSERT_BATCH_ROWS, READ_PAGE_ROWS};
use crate::row::Row;
use crate::schema::TableInfo;
use crate::value::Value;
use crate::write_back::SqlDialect;

use super::plan::DumpPlan;

/// Sink the dump's SQL text is written to. The app supplies a file-backed
/// implementation; tests use an in-memory buffer.
pub trait DumpSink {
    /// Append a chunk of dump text.
    ///
    /// # Errors
    ///
    /// Returns [`DumpError::Sink`] when the underlying writer fails (e.g. a
    /// full disk); this aborts the whole dump.
    fn write_str(&mut self, chunk: &str) -> DumpResult<()>;
}

/// The caller's progress + cancellation channel. On the desktop app this is
/// backed by a message channel to the UI and a shared cancel flag; both
/// methods take `&self` so an implementation can use interior mutability.
pub trait DumpControl: Send + Sync {
    /// Called at the start of each table, after each data page, and once at
    /// the end. Never called with stale totals.
    fn report(&self, progress: &DumpProgress);

    /// Polled between pages and tables. Returning `true` stops the dump at
    /// the next boundary, leaving a partial but valid output.
    fn is_cancelled(&self) -> bool;
}

/// A progress snapshot. `rows_done`/`rows_total` drive a percentage bar;
/// `tables_done`/`tables_total` a coarser step count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpProgress {
    pub tables_total: usize,
    pub tables_done: usize,
    pub rows_total: u64,
    pub rows_done: u64,
    /// The table currently being read, or `None` at the final report.
    pub current_table: Option<String>,
}

/// A table the dump could not read. Recorded rather than propagated so one
/// bad table does not abort the run (ADR-0049 Decision 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableFailure {
    pub table: String,
    pub message: String,
}

/// A table whose data was cut short. Only keyless tables larger than one
/// page truncate — they cannot be keyset-paged — and the shortfall is
/// surfaced rather than hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableTruncation {
    pub table: String,
    pub rows_written: u64,
}

/// The result of a completed (or cancelled) dump. Always returned on
/// success even when some tables failed — the caller decides how to present
/// partial results.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DumpOutcome {
    pub tables_dumped: usize,
    pub rows_written: u64,
    pub failures: Vec<TableFailure>,
    pub truncations: Vec<TableTruncation>,
    pub cancelled: bool,
}

/// A fatal dump error — currently only an output-sink failure. Adapter
/// errors are non-fatal and land in [`DumpOutcome::failures`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DumpError {
    Sink(String),
}

impl std::fmt::Display for DumpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DumpError::Sink(m) => write!(f, "dump output failed: {m}"),
        }
    }
}

impl std::error::Error for DumpError {}

pub type DumpResult<T> = Result<T, DumpError>;

/// How processing one table ended.
enum TableFlow {
    /// The table was dumped (possibly with a recorded truncation).
    Completed,
    /// The table was skipped after a recorded adapter failure.
    Failed,
    /// Cancellation was observed mid-table.
    Cancelled,
}

/// Run a whole-connection dump described by `plan`, writing SQL to `sink`
/// and reporting through `control`.
///
/// Returns the [`DumpOutcome`] — including any per-table failures and
/// truncations — unless the sink itself fails, which is fatal.
///
/// # Errors
///
/// Returns [`DumpError::Sink`] if writing to `sink` fails at any point.
pub async fn run_dump(
    adapter: &dyn DatabaseAdapter,
    dialect: SqlDialect,
    plan: &DumpPlan,
    sink: &mut dyn DumpSink,
    control: &dyn DumpControl,
) -> DumpResult<DumpOutcome> {
    let caps = adapter.capabilities();
    let tables_total = plan.tables.len();
    let rows_total = plan.total_rows();
    let mut outcome = DumpOutcome::default();

    sink.write_str(&dump_header(dialect))?;

    for (index, table_plan) in plan.tables.iter().enumerate() {
        if control.is_cancelled() {
            outcome.cancelled = true;
            break;
        }
        report(
            control,
            tables_total,
            index,
            rows_total,
            &outcome,
            Some(&table_plan.table),
        );

        match dump_one_table(
            adapter,
            &caps,
            dialect,
            &table_plan.table,
            sink,
            control,
            &mut outcome,
        )
        .await?
        {
            TableFlow::Completed => outcome.tables_dumped += 1,
            TableFlow::Failed => {}
            TableFlow::Cancelled => {
                outcome.cancelled = true;
                break;
            }
        }
    }

    report(
        control,
        tables_total,
        tables_total,
        rows_total,
        &outcome,
        None,
    );
    Ok(outcome)
}

/// Emit one table: its DDL, then its data. An adapter error at any step is
/// recorded and the table skipped; only a sink error propagates.
async fn dump_one_table(
    adapter: &dyn DatabaseAdapter,
    caps: &Capabilities,
    dialect: SqlDialect,
    table: &TableInfo,
    sink: &mut dyn DumpSink,
    control: &dyn DumpControl,
    outcome: &mut DumpOutcome,
) -> DumpResult<TableFlow> {
    let name = display_name(table);

    if caps.has_table_ddl {
        match adapter.table_ddl(table).await {
            Ok(ddl) => {
                sink.write_str(&format!("\n-- {name}\n"))?;
                sink.write_str(&ddl)?;
                if !ddl.ends_with('\n') {
                    sink.write_str("\n")?;
                }
            }
            Err(e) => {
                outcome.failures.push(TableFailure {
                    table: name,
                    message: e.message().to_owned(),
                });
                return Ok(TableFlow::Failed);
            }
        }
    }

    // The primary key drives keyset paging. Absent describe support, or a
    // keyless table, falls back to a single capped page.
    let key_columns = if caps.has_describe_table {
        match adapter.describe_table(table).await {
            Ok(schema) => schema.primary_key,
            Err(e) => {
                outcome.failures.push(TableFailure {
                    table: name,
                    message: e.message().to_owned(),
                });
                return Ok(TableFlow::Failed);
            }
        }
    } else {
        Vec::new()
    };

    dump_table_data(
        adapter,
        dialect,
        table,
        &key_columns,
        sink,
        control,
        outcome,
    )
    .await
}

/// Page through a table's rows, emitting batched `INSERT`s. Keyset paging
/// when `key_columns` is non-empty, otherwise a single capped page.
async fn dump_table_data(
    adapter: &dyn DatabaseAdapter,
    dialect: SqlDialect,
    table: &TableInfo,
    key_columns: &[String],
    sink: &mut dyn DumpSink,
    control: &dyn DumpControl,
    outcome: &mut DumpOutcome,
) -> DumpResult<TableFlow> {
    let name = display_name(table);
    let keyed = !key_columns.is_empty();
    let mut cursor: Option<Vec<Value>> = None;

    loop {
        if control.is_cancelled() {
            return Ok(TableFlow::Cancelled);
        }

        let sql = build_select_page(
            table,
            key_columns,
            dialect,
            READ_PAGE_ROWS,
            cursor.as_deref(),
        );
        let result = match adapter.query(&sql).await {
            Ok(result) => result,
            Err(e) => {
                outcome.failures.push(TableFailure {
                    table: name,
                    message: e.message().to_owned(),
                });
                return Ok(TableFlow::Failed);
            }
        };

        if result.rows.is_empty() {
            break;
        }

        let columns: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
        for chunk in result.rows.chunks(INSERT_BATCH_ROWS) {
            if let Some(statement) = build_insert(table, &columns, chunk, dialect) {
                sink.write_str(&statement)?;
                sink.write_str("\n")?;
            }
        }

        let page_len = result.rows.len();
        outcome.rows_written = outcome.rows_written.saturating_add(page_len as u64);

        if !keyed {
            // No key means no stable cursor: a filled page implies the
            // table is larger than we can page, so record the shortfall.
            if page_len >= READ_PAGE_ROWS {
                outcome.truncations.push(TableTruncation {
                    table: name,
                    rows_written: page_len as u64,
                });
            }
            break;
        }

        if page_len < READ_PAGE_ROWS {
            break;
        }

        // Advance the cursor to the last (largest-key) row of this page.
        let Some(next) = cursor_from_last_row(&result.rows, &columns, key_columns) else {
            outcome.failures.push(TableFailure {
                table: name,
                message: "primary-key column missing from result set".to_owned(),
            });
            return Ok(TableFlow::Failed);
        };
        cursor = Some(next);
    }

    Ok(TableFlow::Completed)
}

/// Extract the key values of the last row of a page, positionally matching
/// `key_columns`. `None` if any key column is absent from `columns`.
fn cursor_from_last_row(
    rows: &[Row],
    columns: &[String],
    key_columns: &[String],
) -> Option<Vec<Value>> {
    let last = rows.last()?;
    let mut values = Vec::with_capacity(key_columns.len());
    for key in key_columns {
        let at = columns.iter().position(|c| c == key)?;
        values.push(last.get(at)?.clone());
    }
    Some(values)
}

fn report(
    control: &dyn DumpControl,
    tables_total: usize,
    tables_done: usize,
    rows_total: u64,
    outcome: &DumpOutcome,
    current_table: Option<&TableInfo>,
) {
    control.report(&DumpProgress {
        tables_total,
        tables_done,
        rows_total,
        rows_done: outcome.rows_written,
        current_table: current_table.map(display_name),
    });
}

fn dump_header(dialect: SqlDialect) -> String {
    let engine = match dialect {
        SqlDialect::Sqlite => "sqlite",
        SqlDialect::Postgres => "postgres",
    };
    format!("-- dbboard logical dump ({engine})\n")
}

fn display_name(table: &TableInfo) -> String {
    match &table.schema {
        Some(schema) => format!("{schema}.{}", table.name),
        None => table.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capabilities;
    use crate::dump::{TablePlan, READ_PAGE_ROWS};
    use crate::error::{DbError, DbResult};
    use crate::row::{Column, QueryResult};
    use crate::schema::TableSchema;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// An adapter whose `query` replays a scripted queue of results, and
    /// whose DDL/describe responses are configurable. It ignores the SQL
    /// text — the SELECT correctness is covered by `select.rs` — so it can
    /// exercise the orchestration loop deterministically.
    struct FakeAdapter {
        caps: Capabilities,
        ddl: DbResult<String>,
        pk: Vec<String>,
        pages: Mutex<Vec<DbResult<QueryResult>>>,
    }

    impl FakeAdapter {
        fn new(caps: Capabilities, pk: Vec<&str>, pages: Vec<DbResult<QueryResult>>) -> Self {
            Self {
                caps,
                ddl: Ok("CREATE TABLE t (id INTEGER);\n".to_owned()),
                pk: pk.into_iter().map(String::from).collect(),
                // Reversed so `pop` dispenses in call order.
                pages: Mutex::new(pages.into_iter().rev().collect()),
            }
        }
    }

    #[async_trait]
    impl DatabaseAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn capabilities(&self) -> Capabilities {
            self.caps
        }
        async fn ping(&self) -> DbResult<()> {
            Ok(())
        }
        async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
            Ok(Vec::new())
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            self.pages
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| Ok(QueryResult::empty()))
        }
        async fn table_ddl(&self, _table: &TableInfo) -> DbResult<String> {
            self.ddl.clone()
        }
        async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
            Ok(TableSchema {
                table: table.clone(),
                columns: Vec::new(),
                primary_key: self.pk.clone(),
            })
        }
    }

    struct VecSink(String);
    impl DumpSink for VecSink {
        fn write_str(&mut self, chunk: &str) -> DumpResult<()> {
            self.0.push_str(chunk);
            Ok(())
        }
    }

    struct FailingSink;
    impl DumpSink for FailingSink {
        fn write_str(&mut self, _chunk: &str) -> DumpResult<()> {
            Err(DumpError::Sink("disk full".to_owned()))
        }
    }

    #[derive(Default)]
    struct RecordingControl {
        cancel_after_checks: Option<usize>,
        checks: AtomicUsize,
        reports: Mutex<Vec<DumpProgress>>,
    }

    impl DumpControl for RecordingControl {
        fn report(&self, progress: &DumpProgress) {
            self.reports.lock().unwrap().push(progress.clone());
        }
        fn is_cancelled(&self) -> bool {
            match self.cancel_after_checks {
                Some(limit) => self.checks.fetch_add(1, Ordering::SeqCst) >= limit,
                None => false,
            }
        }
    }

    fn caps_full() -> Capabilities {
        Capabilities {
            has_describe_table: true,
            has_table_ddl: true,
            ..Capabilities::default()
        }
    }

    fn one_col_page(ids: &[i64]) -> QueryResult {
        QueryResult {
            columns: vec![Column {
                name: "id".into(),
                declared_type: None,
            }],
            rows: ids
                .iter()
                .map(|&i| Row::new(vec![Value::Integer(i)]))
                .collect(),
            rows_affected: 0,
        }
    }

    fn plan_of(names: &[&str], rows: u64) -> DumpPlan {
        DumpPlan::new(
            names
                .iter()
                .map(|n| TablePlan::new(TableInfo::unqualified(*n), rows))
                .collect(),
        )
    }

    #[tokio::test]
    async fn dumps_ddl_and_data_for_a_small_table() {
        // One short page ends paging; DDL precedes the INSERT.
        let adapter = FakeAdapter::new(caps_full(), vec!["id"], vec![Ok(one_col_page(&[1, 2, 3]))]);
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], 3),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert_eq!(outcome.tables_dumped, 1);
        assert_eq!(outcome.rows_written, 3);
        assert!(outcome.failures.is_empty());
        assert!(!outcome.cancelled);
        assert!(sink.0.contains("-- dbboard logical dump (sqlite)"));
        let ddl_at = sink.0.find("CREATE TABLE").unwrap();
        let insert_at = sink.0.find("INSERT INTO").unwrap();
        assert!(ddl_at < insert_at, "DDL must precede data:\n{}", sink.0);
        assert!(sink.0.contains("(1),\n(2),\n(3)") || sink.0.contains("(1), (2), (3)"));
    }

    #[tokio::test]
    async fn keyset_paging_continues_until_a_short_page() {
        // A full page (READ_PAGE_ROWS rows) forces a second query; the
        // second page is short and ends paging. Total rows accumulate.
        let full: Vec<i64> = (0..READ_PAGE_ROWS)
            .map(|i| i64::try_from(i).unwrap())
            .collect();
        let adapter = FakeAdapter::new(
            caps_full(),
            vec!["id"],
            vec![Ok(one_col_page(&full)), Ok(one_col_page(&[9_999]))],
        );
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], READ_PAGE_ROWS as u64 + 1),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert_eq!(outcome.rows_written, READ_PAGE_ROWS as u64 + 1);
        assert_eq!(outcome.tables_dumped, 1);
        assert!(
            outcome.truncations.is_empty(),
            "keyed tables never truncate"
        );
    }

    #[tokio::test]
    async fn a_keyless_full_page_records_a_truncation() {
        let full: Vec<i64> = (0..READ_PAGE_ROWS)
            .map(|i| i64::try_from(i).unwrap())
            .collect();
        // No PK → single capped page → truncation recorded.
        let adapter = FakeAdapter::new(caps_full(), vec![], vec![Ok(one_col_page(&full))]);
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], READ_PAGE_ROWS as u64),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert_eq!(outcome.truncations.len(), 1);
        assert_eq!(outcome.truncations[0].table, "t");
        assert_eq!(outcome.rows_written, READ_PAGE_ROWS as u64);
    }

    #[tokio::test]
    async fn a_table_ddl_error_is_recorded_and_skips_the_table() {
        let mut adapter = FakeAdapter::new(caps_full(), vec!["id"], vec![Ok(one_col_page(&[1]))]);
        adapter.ddl = Err(DbError::Query("no such table: t".to_owned()));
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], 1),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert_eq!(outcome.tables_dumped, 0);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].table, "t");
        assert_eq!(outcome.rows_written, 0);
        // No INSERT emitted for the skipped table.
        assert!(!sink.0.contains("INSERT INTO"));
    }

    #[tokio::test]
    async fn a_query_error_on_one_table_does_not_abort_the_others() {
        // First table's data query errors; second table dumps fine.
        let adapter = FakeAdapter::new(
            caps_full(),
            vec!["id"],
            vec![
                Err(DbError::Query("boom".to_owned())),
                Ok(one_col_page(&[1, 2])),
            ],
        );
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["bad", "good"], 2),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].table, "bad");
        assert_eq!(outcome.tables_dumped, 1);
        assert_eq!(outcome.rows_written, 2);
    }

    #[tokio::test]
    async fn cancellation_stops_the_dump_with_a_partial_outcome() {
        // Cancel is observed at the first between-table check, before any
        // table is processed.
        let adapter = FakeAdapter::new(caps_full(), vec!["id"], vec![Ok(one_col_page(&[1]))]);
        let mut sink = VecSink(String::new());
        let control = RecordingControl {
            cancel_after_checks: Some(0),
            ..RecordingControl::default()
        };
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["a", "b"], 1),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert!(outcome.cancelled);
        assert_eq!(outcome.tables_dumped, 0);
    }

    #[tokio::test]
    async fn a_sink_failure_is_fatal() {
        let adapter = FakeAdapter::new(caps_full(), vec!["id"], vec![Ok(one_col_page(&[1]))]);
        let mut sink = FailingSink;
        let control = RecordingControl::default();
        let err = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], 1),
            &mut sink,
            &control,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DumpError::Sink(_)));
    }

    #[tokio::test]
    async fn a_table_without_ddl_capability_still_dumps_data() {
        let caps = Capabilities {
            has_describe_table: true,
            has_table_ddl: false,
            ..Capabilities::default()
        };
        let adapter = FakeAdapter::new(caps, vec!["id"], vec![Ok(one_col_page(&[7]))]);
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        let outcome = run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], 1),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        assert_eq!(outcome.rows_written, 1);
        assert!(!sink.0.contains("CREATE TABLE"));
        assert!(sink.0.contains("INSERT INTO"));
    }

    #[tokio::test]
    async fn the_final_report_clears_the_current_table() {
        let adapter = FakeAdapter::new(caps_full(), vec!["id"], vec![Ok(one_col_page(&[1]))]);
        let mut sink = VecSink(String::new());
        let control = RecordingControl::default();
        run_dump(
            &adapter,
            SqlDialect::Sqlite,
            &plan_of(&["t"], 1),
            &mut sink,
            &control,
        )
        .await
        .unwrap();

        let reports = control.reports.lock().unwrap();
        let last = reports.last().unwrap();
        assert_eq!(last.current_table, None);
        assert_eq!(last.tables_done, last.tables_total);
    }
}
