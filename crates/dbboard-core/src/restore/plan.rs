//! Classify a `.sql` script into labelled statements (ADR-0051, slice 3).
//!
//! Layer 2 of the restore pipeline. It takes the raw statements
//! [`split_statements`](super::split_statements) produced and parses each
//! one with the dialect's grammar to attach a [`StatementKind`]. The label
//! drives two things the runner needs: which statements to *strip* (a
//! dump's own `BEGIN`/`COMMIT` must go, because the runner wraps the whole
//! restore in one transaction of its own) and a rough DDL/data breakdown for
//! the completion summary.
//!
//! The defining property is **downgrade-on-parse-failure**: a statement the
//! grammar cannot parse is not dropped and not rejected — it is labelled
//! [`StatementKind::Unparsed`] and passed through verbatim for best-effort
//! execution. This is the deliberate inverse of ADR-0046's `read_only`
//! classifier, which *fails closed* on anything it cannot prove safe.
//! Restore trusts the operator and a hand-written or exotic statement still
//! runs; the engine, not this classifier, has the final say.

use sqlparser::ast::Statement;
use sqlparser::dialect::{Dialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::restore::split_statements;
use crate::write_back::SqlDialect;

/// The category a restore statement was classified into.
///
/// Only two categories change behaviour: [`TransactionControl`] is stripped
/// before the runner applies the script (it manages its own transaction),
/// and the rest run in file order. [`Ddl`] / [`Data`] / [`Other`] differ
/// only for the summary; [`Unparsed`] runs verbatim, best-effort.
///
/// [`TransactionControl`]: StatementKind::TransactionControl
/// [`Ddl`]: StatementKind::Ddl
/// [`Data`]: StatementKind::Data
/// [`Other`]: StatementKind::Other
/// [`Unparsed`]: StatementKind::Unparsed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// Schema construction: `CREATE …`, `ALTER TABLE`, `DROP`, `TRUNCATE`,
    /// `COMMENT`. Runs.
    Ddl,
    /// Data change: `INSERT`, `UPDATE`, `DELETE`, `COPY`. Runs.
    Data,
    /// `BEGIN` / `COMMIT` / `ROLLBACK` / `SAVEPOINT` / `RELEASE`. Stripped —
    /// the runner supplies the transaction boundary.
    TransactionControl,
    /// Parsed to something else (a `SELECT`, `SET`, `PRAGMA`, `USE`, …).
    /// Runs as-is.
    Other,
    /// Did not parse under the dialect (or was not exactly one statement).
    /// Runs verbatim, best-effort — never dropped.
    Unparsed,
}

/// One classified statement: its verbatim source text and its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreStatement {
    pub sql: String,
    pub kind: StatementKind,
}

/// Split `sql` into statements and classify each under `dialect`.
///
/// Statement order is preserved verbatim — restore never reorders, because a
/// dump is already emitted in dependency-safe order. A statement that will
/// not parse is kept as [`StatementKind::Unparsed`] rather than discarded.
#[must_use]
pub fn classify_script(sql: &str, dialect: SqlDialect) -> Vec<RestoreStatement> {
    split_statements(sql)
        .into_iter()
        .map(|sql| {
            let kind = classify_one(&sql, dialect);
            RestoreStatement { sql, kind }
        })
        .collect()
}

fn classify_one(sql: &str, dialect: SqlDialect) -> StatementKind {
    match parse_single(sql, dialect) {
        Some(stmt) => kind_of(&stmt),
        None => StatementKind::Unparsed,
    }
}

/// Parse `sql` as exactly one statement, or `None` if it fails to parse or
/// resolves to zero/multiple statements (either way the runner treats it as
/// opaque). Mirrors `read_only::parse`'s dialect dispatch.
fn parse_single(sql: &str, dialect: SqlDialect) -> Option<Statement> {
    let parser_dialect: Box<dyn Dialect> = match dialect {
        SqlDialect::Postgres => Box::new(PostgreSqlDialect {}),
        SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
    };
    let mut statements = Parser::parse_sql(parser_dialect.as_ref(), sql).ok()?;
    if statements.len() == 1 {
        statements.pop()
    } else {
        None
    }
}

fn kind_of(stmt: &Statement) -> StatementKind {
    match stmt {
        Statement::StartTransaction { .. }
        | Statement::Commit { .. }
        | Statement::Rollback { .. }
        | Statement::Savepoint { .. }
        | Statement::ReleaseSavepoint { .. } => StatementKind::TransactionControl,

        Statement::Insert(_)
        | Statement::Update(_)
        | Statement::Delete(_)
        | Statement::Copy { .. } => StatementKind::Data,

        // Best-effort DDL detection for the summary — the common shapes a
        // dump emits. An un-enumerated DDL variant falls through to `Other`
        // and still runs; only the count is approximate.
        Statement::CreateTable(_)
        | Statement::CreateView(_)
        | Statement::CreateIndex(_)
        | Statement::CreateVirtualTable { .. }
        | Statement::CreateSchema { .. }
        | Statement::CreateSequence { .. }
        | Statement::CreateFunction(_)
        | Statement::CreateTrigger(_)
        | Statement::CreateExtension(_)
        | Statement::CreateType { .. }
        | Statement::CreateDomain(_)
        | Statement::AlterTable(_)
        | Statement::Drop { .. }
        | Statement::Truncate(_)
        | Statement::Comment { .. } => StatementKind::Ddl,

        _ => StatementKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_script, StatementKind};
    use crate::write_back::SqlDialect;

    fn kinds(sql: &str, dialect: SqlDialect) -> Vec<StatementKind> {
        classify_script(sql, dialect)
            .into_iter()
            .map(|s| s.kind)
            .collect()
    }

    #[test]
    fn an_empty_or_comment_only_script_classifies_to_nothing() {
        assert!(classify_script("", SqlDialect::Sqlite).is_empty());
        assert!(classify_script("-- just a header\n", SqlDialect::Sqlite).is_empty());
    }

    #[test]
    fn create_table_is_ddl() {
        assert_eq!(
            kinds(
                "CREATE TABLE t (id INTEGER PRIMARY KEY)",
                SqlDialect::Sqlite
            ),
            vec![StatementKind::Ddl]
        );
    }

    #[test]
    fn create_index_and_view_are_ddl() {
        let sql = "CREATE INDEX i ON t (a); CREATE VIEW v AS SELECT 1";
        assert_eq!(
            kinds(sql, SqlDialect::Postgres),
            vec![StatementKind::Ddl, StatementKind::Ddl]
        );
    }

    #[test]
    fn drop_and_alter_are_ddl() {
        let sql = "DROP TABLE t; ALTER TABLE t ADD COLUMN c INT";
        assert_eq!(
            kinds(sql, SqlDialect::Postgres),
            vec![StatementKind::Ddl, StatementKind::Ddl]
        );
    }

    #[test]
    fn insert_update_delete_are_data() {
        let sql = "INSERT INTO t VALUES (1); UPDATE t SET a = 2; DELETE FROM t";
        assert_eq!(
            kinds(sql, SqlDialect::Sqlite),
            vec![
                StatementKind::Data,
                StatementKind::Data,
                StatementKind::Data
            ]
        );
    }

    #[test]
    fn begin_and_commit_are_transaction_control() {
        // A dump's own transaction wrapper — the runner strips these.
        let sql = "BEGIN; INSERT INTO t VALUES (1); COMMIT";
        assert_eq!(
            kinds(sql, SqlDialect::Postgres),
            vec![
                StatementKind::TransactionControl,
                StatementKind::Data,
                StatementKind::TransactionControl
            ]
        );
    }

    #[test]
    fn select_and_set_are_other() {
        let sql = "SELECT 1; SET search_path TO public";
        assert_eq!(
            kinds(sql, SqlDialect::Postgres),
            vec![StatementKind::Other, StatementKind::Other]
        );
    }

    #[test]
    fn a_pragma_downgrades_to_unparsed_but_is_preserved_and_still_runs() {
        // sqlparser does not model the `PRAGMA x = OFF` form under either
        // dialect, so it lands as Unparsed. The point of downgrade-open is
        // that it is kept verbatim rather than dropped — the engine, not the
        // classifier, decides whether a PRAGMA is valid.
        for dialect in [SqlDialect::Sqlite, SqlDialect::Postgres] {
            let out = classify_script("PRAGMA foreign_keys = OFF", dialect);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].kind, StatementKind::Unparsed);
            assert_eq!(out[0].sql, "PRAGMA foreign_keys = OFF");
        }
    }

    #[test]
    fn unparseable_text_downgrades_rather_than_being_dropped() {
        // Not valid SQL under any dialect: kept as Unparsed, still present.
        let out = classify_script("this is not sql at all", SqlDialect::Sqlite);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, StatementKind::Unparsed);
        assert_eq!(out[0].sql, "this is not sql at all");
    }

    #[test]
    fn a_pg_dollar_quoted_function_parses_under_postgres() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql";
        assert_eq!(kinds(sql, SqlDialect::Postgres), vec![StatementKind::Ddl]);
    }

    #[test]
    fn the_verbatim_sql_is_preserved_alongside_the_kind() {
        let out = classify_script("INSERT INTO t VALUES ('a; b')", SqlDialect::Sqlite);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, StatementKind::Data);
        assert_eq!(out[0].sql, "INSERT INTO t VALUES ('a; b')");
    }
}
