//! Prove a SQL string is a single, read-only statement (ADR-0046).
//!
//! This is the pure classifier behind the MCP server's read-only tool
//! surface. It parses the SQL into an AST and walks it — it never does
//! `starts_with("SELECT")`-style string matching, which is unsound: the
//! Postgres simple query protocol runs `SELECT 1; DROP TABLE t` as two
//! statements, and `WITH x AS (DELETE ... RETURNING *) SELECT * FROM x`
//! is a write that starts with `WITH`.
//!
//! Two layers rely on this:
//!
//! - **Cloudflare D1** has no server-side read-only mode, so this
//!   classifier is its *primary* enforcement — fail closed on anything
//!   it cannot prove read-only, including unparseable input.
//! - The Postgres family (`BEGIN READ ONLY`) and libSQL (`PRAGMA
//!   query_only`) enforce read-only at the engine; this classifier is
//!   their defense-in-depth and single-statement guard.
//!
//! Being pure and I/O-free, it is unit-testable against adversarial
//! input and shareable with the `dbboard-web` sibling.

use sqlparser::ast::{Query, SetExpr, Statement};
use sqlparser::dialect::{Dialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::{DbError, SqlDialect};

/// Why a statement was rejected as not-a-single-read-only-query.
///
/// The reason names the *category* of the problem (multiple statements,
/// a data-modifying statement, a locking clause, …). It deliberately
/// does not echo the offending SQL back, so a rejection never risks
/// reflecting attacker-controlled text into a log or error surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnlyViolation {
    reason: String,
}

impl ReadOnlyViolation {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// The category-level explanation, without any leading prefix.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for ReadOnlyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a single read-only statement: {}", self.reason)
    }
}

impl std::error::Error for ReadOnlyViolation {}

/// The kind of read-only statement a SQL string was proven to be.
///
/// Callers that enforce read-only *at the engine* need this to pick the
/// right mechanism: a plain query can be wrapped in a server-side cursor
/// for row capping, whereas an `EXPLAIN` is a utility statement that
/// cannot be a cursor source and must run directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyStatement {
    /// A `SELECT` / `VALUES` / `TABLE` / `WITH … SELECT` query.
    Query,
    /// An `EXPLAIN` / `DESCRIBE` of a read-only statement.
    Explain,
}

impl From<ReadOnlyViolation> for DbError {
    /// A read-only rejection surfaces to callers as a query error: the
    /// statement was refused before execution. The category-level reason
    /// travels in the message; the original SQL is never echoed.
    fn from(violation: ReadOnlyViolation) -> Self {
        DbError::Query(violation.to_string())
    }
}

/// Reject anything that is not exactly one read-only statement.
///
/// Returns `Ok(())` only when `sql` parses (under `dialect`) to a single
/// statement that reads without writing: a `SELECT` / `VALUES` / `TABLE`
/// query, a `WITH … SELECT` whose every CTE is itself read-only, or an
/// `EXPLAIN`/`DESCRIBE` of such a statement. Everything else — a write,
/// DDL, a `CALL`, a `PRAGMA`, a locking `FOR UPDATE`, data-modifying CTEs,
/// multiple statements, or unparseable text — is a [`ReadOnlyViolation`].
///
/// Fails closed: unparseable input is rejected, not passed through.
///
/// # Errors
///
/// Returns [`ReadOnlyViolation`] describing the category of the problem.
pub fn check_read_only(sql: &str, dialect: SqlDialect) -> Result<(), ReadOnlyViolation> {
    classify_read_only(sql, dialect).map(|_| ())
}

/// Like [`check_read_only`], but on success reports which kind of
/// read-only statement `sql` is — so an engine-enforcing caller can pick
/// a cursor (for a [`ReadOnlyStatement::Query`]) or a direct run (for a
/// non-cursorable [`ReadOnlyStatement::Explain`]).
///
/// # Errors
///
/// Returns [`ReadOnlyViolation`] describing the category of the problem.
pub fn classify_read_only(
    sql: &str,
    dialect: SqlDialect,
) -> Result<ReadOnlyStatement, ReadOnlyViolation> {
    let statements = parse(sql, dialect)?;
    match statements.as_slice() {
        [] => Err(ReadOnlyViolation::new("no statement found")),
        [single] => check_statement(single),
        many => Err(ReadOnlyViolation::new(format!(
            "expected a single statement, found {}",
            many.len()
        ))),
    }
}

/// Boolean form of [`check_read_only`] — `true` iff `sql` is a single
/// read-only statement under `dialect`.
#[must_use]
pub fn is_single_read_only_statement(sql: &str, dialect: SqlDialect) -> bool {
    check_read_only(sql, dialect).is_ok()
}

fn parse(sql: &str, dialect: SqlDialect) -> Result<Vec<Statement>, ReadOnlyViolation> {
    // Own the dialect behind a trait object so the two arms share the
    // single `parse_sql` call rather than duplicating it per dialect.
    let parser_dialect: Box<dyn Dialect> = match dialect {
        SqlDialect::Postgres => Box::new(PostgreSqlDialect {}),
        SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
    };
    Parser::parse_sql(parser_dialect.as_ref(), sql)
        .map_err(|e| ReadOnlyViolation::new(format!("could not parse SQL ({e})")))
}

fn check_statement(stmt: &Statement) -> Result<ReadOnlyStatement, ReadOnlyViolation> {
    match stmt {
        Statement::Query(query) => {
            check_query(query)?;
            Ok(ReadOnlyStatement::Query)
        }
        // EXPLAIN/DESC/DESCRIBE of a statement. `EXPLAIN ANALYZE <dml>`
        // actually *executes* the inner statement, so require the inner
        // one to be read-only rather than trusting the EXPLAIN wrapper.
        Statement::Explain { statement, .. } => {
            check_statement(statement)?;
            Ok(ReadOnlyStatement::Explain)
        }
        _ => Err(ReadOnlyViolation::new(
            "only read-only SELECT / WITH / EXPLAIN statements are allowed",
        )),
    }
}

fn check_query(query: &Query) -> Result<(), ReadOnlyViolation> {
    // `SELECT … FOR UPDATE`/`FOR SHARE` takes row locks and signals write
    // intent — it is not read-only even though it starts with SELECT.
    if !query.locks.is_empty() {
        return Err(ReadOnlyViolation::new(
            "a locking clause (FOR UPDATE / FOR SHARE) is not read-only",
        ));
    }

    // Postgres allows data-modifying statements inside a CTE
    // (`WITH x AS (DELETE …) …`); each CTE body is itself a query, so
    // recurse into every one.
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            check_query(&cte.query)?;
        }
    }

    check_set_expr(&query.body)
}

fn check_set_expr(body: &SetExpr) -> Result<(), ReadOnlyViolation> {
    match body {
        // A plain SELECT body. Any subqueries it contains are themselves
        // queries — SQL does not permit a data-modifying statement in a
        // scalar/derived subquery, only in a top-level CTE (handled in
        // `check_query`), so a SELECT body is read-only.
        SetExpr::Select(_) | SetExpr::Values(_) | SetExpr::Table(_) => Ok(()),
        SetExpr::Query(inner) => check_query(inner),
        SetExpr::SetOperation { left, right, .. } => {
            check_set_expr(left)?;
            check_set_expr(right)
        }
        SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_) => {
            Err(ReadOnlyViolation::new(
                "a data-modifying statement (in a CTE or set expression) is not read-only",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Statements that must be accepted, under both dialects unless the
    /// syntax is dialect-specific.
    const READ_ONLY_BOTH: &[&str] = &[
        "SELECT 1",
        "SELECT 1;",
        "SELECT * FROM users",
        "SELECT id, name FROM users WHERE active = TRUE ORDER BY id LIMIT 10",
        "SELECT count(*) FROM orders",
        "WITH recent AS (SELECT * FROM orders WHERE ts > 0) SELECT * FROM recent",
        "SELECT * FROM a UNION SELECT * FROM b",
        "SELECT * FROM (SELECT id FROM t WHERE id IN (SELECT id FROM u)) s",
        "VALUES (1), (2)",
        "  \n SELECT 1 \n ",
        "-- leading comment\nSELECT 1",
        "EXPLAIN SELECT * FROM users",
    ];

    /// Statements that must be rejected under both dialects.
    const WRITES_BOTH: &[&str] = &[
        "INSERT INTO t (a) VALUES (1)",
        "UPDATE t SET a = 1",
        "DELETE FROM t",
        "DROP TABLE t",
        "CREATE TABLE t (id INT)",
        "ALTER TABLE t ADD COLUMN c INT",
        "TRUNCATE t",
        // Multi-statement: the classic simple-query-protocol hazard.
        "SELECT 1; DROP TABLE t",
        "SELECT 1; SELECT 2",
        "DROP TABLE t; SELECT 1",
        // Data-modifying CTE that *starts with* WITH/SELECT.
        "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x",
        "WITH x AS (INSERT INTO t VALUES (1) RETURNING *) SELECT * FROM x",
        "WITH x AS (UPDATE t SET a = 1 RETURNING *) SELECT * FROM x",
        // Locking read.
        "SELECT * FROM t FOR UPDATE",
        "SELECT * FROM t FOR SHARE",
        // EXPLAIN ANALYZE executes its inner statement.
        "EXPLAIN ANALYZE DELETE FROM t",
        // Not a query at all.
        "",
        "   ",
        "-- just a comment",
        // Unparseable → fail closed.
        "SELEC 1",
        "NOT SQL AT ALL !!!",
    ];

    #[test]
    fn accepts_read_only_statements_in_both_dialects() {
        for sql in READ_ONLY_BOTH {
            for dialect in [SqlDialect::Postgres, SqlDialect::Sqlite] {
                assert!(
                    is_single_read_only_statement(sql, dialect),
                    "should accept under {dialect:?}: {sql:?} -> {:?}",
                    check_read_only(sql, dialect)
                );
            }
        }
    }

    #[test]
    fn rejects_writes_and_multi_statements_in_both_dialects() {
        for sql in WRITES_BOTH {
            for dialect in [SqlDialect::Postgres, SqlDialect::Sqlite] {
                assert!(
                    !is_single_read_only_statement(sql, dialect),
                    "should reject under {dialect:?}: {sql:?}"
                );
            }
        }
    }

    #[test]
    fn classify_distinguishes_queries_from_explains() {
        assert_eq!(
            classify_read_only("SELECT 1", SqlDialect::Postgres).unwrap(),
            ReadOnlyStatement::Query
        );
        assert_eq!(
            classify_read_only("WITH x AS (SELECT 1) SELECT * FROM x", SqlDialect::Postgres)
                .unwrap(),
            ReadOnlyStatement::Query
        );
        assert_eq!(
            classify_read_only("EXPLAIN SELECT * FROM t", SqlDialect::Postgres).unwrap(),
            ReadOnlyStatement::Explain
        );
    }

    #[test]
    fn classify_rejects_a_write_like_check_does() {
        assert!(classify_read_only("DELETE FROM t", SqlDialect::Postgres).is_err());
    }

    #[test]
    fn multi_statement_reason_names_the_count() {
        let err = check_read_only("SELECT 1; SELECT 2", SqlDialect::Postgres).unwrap_err();
        assert!(
            err.reason().contains("single statement"),
            "reason was: {}",
            err.reason()
        );
    }

    #[test]
    fn locking_clause_reason_is_specific() {
        let err = check_read_only("SELECT * FROM t FOR UPDATE", SqlDialect::Postgres).unwrap_err();
        assert!(err.reason().contains("locking"), "reason: {}", err.reason());
    }

    #[test]
    fn empty_input_is_rejected_not_passed_through() {
        let err = check_read_only("", SqlDialect::Sqlite).unwrap_err();
        assert!(
            err.reason().contains("no statement"),
            "reason: {}",
            err.reason()
        );
    }

    #[test]
    fn unparseable_input_fails_closed() {
        assert!(!is_single_read_only_statement(
            "DEFINITELY NOT SQL",
            SqlDialect::Sqlite
        ));
        assert!(!is_single_read_only_statement(
            "DEFINITELY NOT SQL",
            SqlDialect::Postgres
        ));
    }

    #[test]
    fn violation_display_carries_a_stable_prefix() {
        let err = check_read_only("DELETE FROM t", SqlDialect::Postgres).unwrap_err();
        assert!(err
            .to_string()
            .starts_with("not a single read-only statement:"));
    }

    #[test]
    fn violation_converts_to_a_query_error() {
        let violation = check_read_only("DELETE FROM t", SqlDialect::Postgres).unwrap_err();
        let err: DbError = violation.into();
        assert_eq!(err.category(), "query");
        assert!(err.message().contains("read-only"));
    }

    #[test]
    fn violation_reason_never_echoes_the_input_sql() {
        // A rejection must not reflect attacker-controlled text back.
        let sql = "DROP TABLE super_secret_marker_table";
        let err = check_read_only(sql, SqlDialect::Postgres).unwrap_err();
        assert!(
            !err.to_string().contains("super_secret_marker_table"),
            "violation leaked the SQL: {err}"
        );
    }
}
