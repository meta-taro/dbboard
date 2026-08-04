//! The logical-restore orchestrator (ADR-0051, slice 4).
//!
//! The read-side counterpart to [`run_dump`](crate::run_dump). It takes a
//! `.sql` script, classifies it (Layer 2), and applies the runnable
//! statements to a target connection through the [`DatabaseAdapter`] trait —
//! reporting progress and honouring cancellation, exactly like the dump
//! orchestrator, and staying I/O-free so it is testable with a fake adapter.
//!
//! Two safety and correctness rules shape the run:
//!
//! - **Empty-target gate.** [`plan_restore`] records the target's existing
//!   tables; [`run_restore`] refuses to touch a non-empty target unless the
//!   caller passes `confirmed: true` (the typed-confirmation the UI collects).
//!   This is the ADR-0051 "empty / new targets only" safety model.
//! - **Per-engine transaction strategy.** An adapter that advertises
//!   [`Capabilities::has_atomic_restore`] runs the whole script as one atomic
//!   batch — all statements commit or none do. An adapter with only
//!   [`Capabilities::has_execute`] (Cloudflare D1, whose HTTP API has no
//!   multi-statement transaction) runs statements one at a time; its
//!   [`OnError`] policy decides whether a failed statement stops the run
//!   (leaving a partial restore) or is recorded and skipped.
//!
//! Either way, a dump's own `BEGIN`/`COMMIT` is stripped first — the runner
//! owns the transaction boundary, so leaving them in would nest transactions.

use crate::adapter::DatabaseAdapter;
use crate::error::{DbError, DbResult};
use crate::restore::plan::{classify_script, RestoreStatement, StatementKind};
use crate::schema::TableInfo;
use crate::write_back::SqlDialect;

/// The caller's progress + cancellation channel, mirroring
/// [`DumpControl`](crate::DumpControl). Both methods take `&self` so an
/// implementation can use interior mutability.
pub trait RestoreControl: Send + Sync {
    /// Called before each statement (per-statement path) or once around the
    /// atomic batch, and once at the end with `statements_done ==
    /// statements_total`.
    fn report(&self, progress: &RestoreProgress);

    /// Polled between statements on the per-statement path, and once before
    /// the atomic batch. Returning `true` stops at the next boundary.
    fn is_cancelled(&self) -> bool;
}

/// A progress snapshot. `Default` is the all-zero starting state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreProgress {
    pub statements_total: usize,
    pub statements_done: usize,
    /// The 0-based index of the statement about to run, or `None` at the
    /// final report.
    pub current_index: Option<usize>,
}

/// What to do when a statement fails on the per-statement (non-atomic) path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnError {
    /// Stop at the first failure, leaving a partial restore. The default and
    /// safest: without a transaction, later statements likely depend on the
    /// one that failed (an `INSERT` into a table whose `CREATE` errored).
    Stop,
    /// Record the failure and keep going. Best-effort, matching how the dump
    /// path degrades for Aurora DSQL.
    Continue,
}

/// Options for a restore run.
#[derive(Debug, Clone, Copy)]
pub struct RestoreOptions {
    /// The caller has confirmed applying to a non-empty target. Ignored when
    /// the target is already empty.
    pub confirmed: bool,
    /// Per-statement failure policy. Ignored on the atomic path (a batch is
    /// all-or-nothing by construction).
    pub on_error: OnError,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            confirmed: false,
            on_error: OnError::Stop,
        }
    }
}

/// A restore preflight: the classified script plus the target's current
/// tables (empty ⇒ the restore may run without confirmation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    pub statements: Vec<RestoreStatement>,
    pub existing_tables: Vec<String>,
}

impl RestorePlan {
    /// True when the target has no user tables — the unconfirmed-safe case.
    #[must_use]
    pub fn is_target_empty(&self) -> bool {
        self.existing_tables.is_empty()
    }

    /// The statements that will actually be executed — everything except the
    /// stripped transaction-control statements.
    fn runnable(&self) -> impl Iterator<Item = &RestoreStatement> {
        self.statements
            .iter()
            .filter(|s| s.kind != StatementKind::TransactionControl)
    }

    /// How many statements [`run_restore`] will execute.
    #[must_use]
    pub fn runnable_count(&self) -> usize {
        self.runnable().count()
    }
}

/// A statement that failed to apply on the per-statement path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementFailure {
    /// 0-based position among the runnable statements.
    pub index: usize,
    pub message: String,
}

/// The result of a completed (or cancelled) restore.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreOutcome {
    pub statements_run: usize,
    pub ddl_run: usize,
    pub data_run: usize,
    pub failures: Vec<StatementFailure>,
    pub cancelled: bool,
    /// True if the script ran as one atomic batch.
    pub atomic: bool,
}

/// A fatal restore error — one that prevents (or unwinds) the whole run.
/// Per-statement failures on the non-atomic path are non-fatal and land in
/// [`RestoreOutcome::failures`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The target already has tables and the caller did not confirm.
    TargetNotEmpty { existing: Vec<String> },
    /// The adapter cannot execute writes at all (no `has_execute`).
    Unsupported(String),
    /// The atomic batch failed as a unit — nothing was applied.
    Transaction(String),
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestoreError::TargetNotEmpty { existing } => write!(
                f,
                "restore target is not empty ({} existing table(s))",
                existing.len()
            ),
            RestoreError::Unsupported(m) => write!(f, "restore is unavailable: {m}"),
            RestoreError::Transaction(m) => write!(f, "restore transaction failed: {m}"),
        }
    }
}

impl std::error::Error for RestoreError {}

pub type RestoreResult<T> = Result<T, RestoreError>;

/// Preflight a restore: list the target's tables and classify `script`.
///
/// # Errors
///
/// Returns whatever [`DatabaseAdapter::list_tables`] surfaces.
pub async fn plan_restore(
    adapter: &dyn DatabaseAdapter,
    dialect: SqlDialect,
    script: &str,
) -> DbResult<RestorePlan> {
    let existing_tables = adapter
        .list_tables()
        .await?
        .iter()
        .map(display_name)
        .collect();
    Ok(RestorePlan {
        statements: classify_script(script, dialect),
        existing_tables,
    })
}

/// Apply `plan` to `adapter`, reporting through `control`.
///
/// # Errors
///
/// - [`RestoreError::TargetNotEmpty`] if the target has tables and
///   `options.confirmed` is false — nothing is applied.
/// - [`RestoreError::Unsupported`] if the adapter cannot execute writes.
/// - [`RestoreError::Transaction`] if the atomic batch fails — nothing is
///   applied.
pub async fn run_restore(
    adapter: &dyn DatabaseAdapter,
    plan: &RestorePlan,
    options: RestoreOptions,
    control: &dyn RestoreControl,
) -> RestoreResult<RestoreOutcome> {
    if !plan.is_target_empty() && !options.confirmed {
        return Err(RestoreError::TargetNotEmpty {
            existing: plan.existing_tables.clone(),
        });
    }

    let caps = adapter.capabilities();
    if !caps.has_execute {
        return Err(RestoreError::Unsupported(
            "this connection cannot execute statements".to_owned(),
        ));
    }

    let runnable: Vec<&RestoreStatement> = plan.runnable().collect();
    let total = runnable.len();

    if caps.has_atomic_restore {
        run_atomic(adapter, &runnable, total, control).await
    } else {
        Ok(run_per_statement(adapter, &runnable, total, options.on_error, control).await)
    }
}

/// Run every statement as one atomic batch. Cancellation can only be observed
/// before the batch starts — the adapter call is indivisible.
async fn run_atomic(
    adapter: &dyn DatabaseAdapter,
    runnable: &[&RestoreStatement],
    total: usize,
    control: &dyn RestoreControl,
) -> RestoreResult<RestoreOutcome> {
    if control.is_cancelled() {
        report(control, total, 0, None);
        return Ok(RestoreOutcome {
            cancelled: true,
            atomic: true,
            ..RestoreOutcome::default()
        });
    }

    report(control, total, 0, Some(0));
    let sqls: Vec<String> = runnable.iter().map(|s| s.sql.clone()).collect();
    adapter
        .execute_in_transaction(&sqls)
        .await
        .map_err(|e| RestoreError::Transaction(e.message().to_owned()))?;

    let (ddl_run, data_run) = count_kinds(runnable);
    report(control, total, total, None);
    Ok(RestoreOutcome {
        statements_run: total,
        ddl_run,
        data_run,
        failures: Vec::new(),
        cancelled: false,
        atomic: true,
    })
}

/// Run statements one at a time, honouring the [`OnError`] policy and
/// checking for cancellation between statements.
async fn run_per_statement(
    adapter: &dyn DatabaseAdapter,
    runnable: &[&RestoreStatement],
    total: usize,
    on_error: OnError,
    control: &dyn RestoreControl,
) -> RestoreOutcome {
    let mut outcome = RestoreOutcome {
        atomic: false,
        ..RestoreOutcome::default()
    };

    for (index, statement) in runnable.iter().enumerate() {
        if control.is_cancelled() {
            outcome.cancelled = true;
            break;
        }
        report(control, total, outcome.statements_run, Some(index));

        match adapter.execute(&statement.sql).await {
            Ok(_) => {
                outcome.statements_run += 1;
                match statement.kind {
                    StatementKind::Ddl => outcome.ddl_run += 1,
                    StatementKind::Data => outcome.data_run += 1,
                    _ => {}
                }
            }
            Err(e) => {
                outcome.failures.push(StatementFailure {
                    index,
                    message: e.message().to_owned(),
                });
                if on_error == OnError::Stop {
                    break;
                }
            }
        }
    }

    report(control, total, outcome.statements_run, None);
    outcome
}

fn count_kinds(runnable: &[&RestoreStatement]) -> (usize, usize) {
    let ddl = runnable
        .iter()
        .filter(|s| s.kind == StatementKind::Ddl)
        .count();
    let data = runnable
        .iter()
        .filter(|s| s.kind == StatementKind::Data)
        .count();
    (ddl, data)
}

fn report(control: &dyn RestoreControl, total: usize, done: usize, current: Option<usize>) {
    control.report(&RestoreProgress {
        statements_total: total,
        statements_done: done,
        current_index: current,
    });
}

fn display_name(table: &TableInfo) -> String {
    match &table.schema {
        Some(schema) => format!("{schema}.{}", table.name),
        None => table.name.clone(),
    }
}

/// A `DbError` conversion so a caller that funnels everything through
/// `DbResult` can carry a restore refusal. Not used inside this module.
impl From<RestoreError> for DbError {
    fn from(e: RestoreError) -> Self {
        DbError::Query(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capabilities;
    use crate::error::{DbError, DbResult};
    use crate::row::QueryResult;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Records every `execute` / `execute_in_transaction` call and replays a
    /// scripted queue of per-statement results. `caps` selects the path.
    struct FakeAdapter {
        caps: Capabilities,
        tables: Vec<TableInfo>,
        executed: Mutex<Vec<String>>,
        batches: Mutex<Vec<Vec<String>>>,
        results: Mutex<Vec<DbResult<u64>>>,
        tx_result: Mutex<Option<DbResult<()>>>,
    }

    impl FakeAdapter {
        fn new(caps: Capabilities) -> Self {
            Self {
                caps,
                tables: Vec::new(),
                executed: Mutex::new(Vec::new()),
                batches: Mutex::new(Vec::new()),
                results: Mutex::new(Vec::new()),
                tx_result: Mutex::new(Some(Ok(()))),
            }
        }

        fn with_tables(mut self, names: &[&str]) -> Self {
            self.tables = names.iter().map(|n| TableInfo::unqualified(*n)).collect();
            self
        }

        /// Script per-statement `execute` results, dispensed in call order.
        fn with_results(self, results: Vec<DbResult<u64>>) -> Self {
            *self.results.lock().unwrap() = results.into_iter().rev().collect();
            self
        }

        fn with_tx_result(self, r: DbResult<()>) -> Self {
            *self.tx_result.lock().unwrap() = Some(r);
            self
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
            Ok(self.tables.clone())
        }
        async fn query(&self, _sql: &str) -> DbResult<QueryResult> {
            Ok(QueryResult::empty())
        }
        async fn execute(&self, sql: &str) -> DbResult<u64> {
            self.executed.lock().unwrap().push(sql.to_owned());
            self.results.lock().unwrap().pop().unwrap_or(Ok(0))
        }
        async fn execute_in_transaction(&self, statements: &[String]) -> DbResult<()> {
            self.batches.lock().unwrap().push(statements.to_vec());
            self.tx_result.lock().unwrap().take().unwrap_or(Ok(()))
        }
    }

    #[derive(Default)]
    struct RecordingControl {
        cancel_after_checks: Option<usize>,
        checks: AtomicUsize,
        reports: Mutex<Vec<RestoreProgress>>,
    }

    impl RestoreControl for RecordingControl {
        fn report(&self, progress: &RestoreProgress) {
            self.reports.lock().unwrap().push(progress.clone());
        }
        fn is_cancelled(&self) -> bool {
            match self.cancel_after_checks {
                Some(limit) => self.checks.fetch_add(1, Ordering::SeqCst) >= limit,
                None => false,
            }
        }
    }

    fn caps_atomic() -> Capabilities {
        Capabilities {
            has_execute: true,
            has_atomic_restore: true,
            ..Capabilities::default()
        }
    }

    fn caps_per_statement() -> Capabilities {
        Capabilities {
            has_execute: true,
            has_atomic_restore: false,
            ..Capabilities::default()
        }
    }

    const SCRIPT: &str =
        "CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1); INSERT INTO t VALUES (2)";

    #[tokio::test]
    async fn plan_restore_records_existing_tables_and_classifies() {
        let adapter = FakeAdapter::new(caps_atomic()).with_tables(&["a", "b"]);
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        assert_eq!(plan.existing_tables, vec!["a", "b"]);
        assert!(!plan.is_target_empty());
        assert_eq!(plan.runnable_count(), 3);
    }

    #[tokio::test]
    async fn a_non_empty_target_is_refused_without_confirmation() {
        let adapter = FakeAdapter::new(caps_atomic()).with_tables(&["existing"]);
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let err = run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(),
            &RecordingControl::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RestoreError::TargetNotEmpty { .. }));
        // Nothing was executed.
        assert!(adapter.batches.lock().unwrap().is_empty());
        assert!(adapter.executed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_non_empty_target_runs_when_confirmed() {
        let adapter = FakeAdapter::new(caps_atomic()).with_tables(&["existing"]);
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let options = RestoreOptions {
            confirmed: true,
            ..RestoreOptions::default()
        };
        let outcome = run_restore(&adapter, &plan, options, &RecordingControl::default())
            .await
            .unwrap();
        assert!(outcome.atomic);
        assert_eq!(outcome.statements_run, 3);
    }

    #[tokio::test]
    async fn an_adapter_without_execute_is_unsupported() {
        let adapter = FakeAdapter::new(Capabilities::default());
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let err = run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(),
            &RecordingControl::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RestoreError::Unsupported(_)));
    }

    #[tokio::test]
    async fn the_atomic_path_sends_one_batch_of_all_runnable_statements() {
        let adapter = FakeAdapter::new(caps_atomic());
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let outcome = run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(),
            &RecordingControl::default(),
        )
        .await
        .unwrap();

        assert!(outcome.atomic);
        assert_eq!(outcome.statements_run, 3);
        assert_eq!(outcome.ddl_run, 1);
        assert_eq!(outcome.data_run, 2);
        let batches = adapter.batches.lock().unwrap();
        assert_eq!(batches.len(), 1, "exactly one atomic batch");
        assert_eq!(batches[0].len(), 3);
        assert!(
            adapter.executed.lock().unwrap().is_empty(),
            "no per-statement calls"
        );
    }

    #[tokio::test]
    async fn the_atomic_path_strips_transaction_control_statements() {
        let adapter = FakeAdapter::new(caps_atomic());
        let script = "BEGIN; CREATE TABLE t (id INT); INSERT INTO t VALUES (1); COMMIT";
        let plan = plan_restore(&adapter, SqlDialect::Postgres, script)
            .await
            .unwrap();
        run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(),
            &RecordingControl::default(),
        )
        .await
        .unwrap();
        let batches = adapter.batches.lock().unwrap();
        // BEGIN and COMMIT are dropped; only the two real statements remain.
        assert_eq!(batches[0].len(), 2);
        assert!(batches[0][0].contains("CREATE TABLE"));
        assert!(batches[0][1].contains("INSERT INTO"));
    }

    #[tokio::test]
    async fn an_atomic_batch_failure_is_fatal_and_applies_nothing() {
        let adapter = FakeAdapter::new(caps_atomic())
            .with_tx_result(Err(DbError::Query("constraint violation".to_owned())));
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let err = run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(),
            &RecordingControl::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RestoreError::Transaction(_)));
    }

    #[tokio::test]
    async fn the_per_statement_path_runs_each_statement_in_order() {
        let adapter = FakeAdapter::new(caps_per_statement());
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let outcome = run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(),
            &RecordingControl::default(),
        )
        .await
        .unwrap();

        assert!(!outcome.atomic);
        assert_eq!(outcome.statements_run, 3);
        assert_eq!(outcome.ddl_run, 1);
        assert_eq!(outcome.data_run, 2);
        let executed = adapter.executed.lock().unwrap();
        assert_eq!(executed.len(), 3);
        assert!(executed[0].contains("CREATE TABLE"));
        assert!(
            adapter.batches.lock().unwrap().is_empty(),
            "no atomic batch"
        );
    }

    #[tokio::test]
    async fn per_statement_stop_on_error_halts_at_the_first_failure() {
        let adapter = FakeAdapter::new(caps_per_statement()).with_results(vec![
            Ok(0),
            Err(DbError::Query("boom".to_owned())),
            Ok(0),
        ]);
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let outcome = run_restore(
            &adapter,
            &plan,
            RestoreOptions::default(), // OnError::Stop
            &RecordingControl::default(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.statements_run, 1);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].index, 1);
        // Stopped: the third statement was never attempted.
        assert_eq!(adapter.executed.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn per_statement_continue_on_error_records_and_keeps_going() {
        let adapter = FakeAdapter::new(caps_per_statement()).with_results(vec![
            Ok(0),
            Err(DbError::Query("boom".to_owned())),
            Ok(0),
        ]);
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let options = RestoreOptions {
            on_error: OnError::Continue,
            ..RestoreOptions::default()
        };
        let outcome = run_restore(&adapter, &plan, options, &RecordingControl::default())
            .await
            .unwrap();

        assert_eq!(outcome.statements_run, 2);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].index, 1);
        // All three were attempted.
        assert_eq!(adapter.executed.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn cancellation_stops_the_per_statement_run() {
        let adapter = FakeAdapter::new(caps_per_statement());
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let control = RecordingControl {
            cancel_after_checks: Some(1), // allow one statement, then cancel
            ..RecordingControl::default()
        };
        let outcome = run_restore(&adapter, &plan, RestoreOptions::default(), &control)
            .await
            .unwrap();

        assert!(outcome.cancelled);
        assert_eq!(outcome.statements_run, 1);
    }

    #[tokio::test]
    async fn the_final_report_clears_the_current_index() {
        let adapter = FakeAdapter::new(caps_per_statement());
        let plan = plan_restore(&adapter, SqlDialect::Sqlite, SCRIPT)
            .await
            .unwrap();
        let control = RecordingControl::default();
        run_restore(&adapter, &plan, RestoreOptions::default(), &control)
            .await
            .unwrap();
        let reports = control.reports.lock().unwrap();
        let last = reports.last().unwrap();
        assert_eq!(last.current_index, None);
        assert_eq!(last.statements_done, last.statements_total);
    }
}
