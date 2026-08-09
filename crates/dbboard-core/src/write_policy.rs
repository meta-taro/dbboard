//! Decide whether a SQL string is a write dbboard will run on an agent's
//! behalf (ADR-0087).
//!
//! [`crate::check_read_only`] answers "may this be read?". This answers the
//! next question: "the operator has opted this connection in to writes — is
//! *this* statement one of them?". Two categories are deliberately not:
//!
//! - **Privilege and principal changes** (`GRANT`, `REVOKE`, `DENY`,
//!   `CREATE`/`ALTER`/`DROP USER`/`ROLE`). An agent that can grant is an
//!   agent that can widen its own reach beyond the connection it was given.
//! - **Wholesale destruction** (`TRUNCATE`, `DROP`). The line against
//!   `DELETE`, which *is* allowed, is that a `DELETE` is row-logged and can
//!   be rolled back inside a transaction, while these are DDL that commit
//!   implicitly on `MySQL` and leave nothing to undo.
//!
//! Neither category is reachable by any configuration — there is no flag
//! that turns them on. They stay in the desktop app's SQL editor, where a
//! human types them.
//!
//! Like [`crate::check_read_only`], what is *permitted* is decided by an AST
//! walk rather than string matching: `DELETE` and `DROP` are not
//! distinguishable by prefix once a statement carries comments or a CTE.
//! A leading-keyword prefilter runs first, but it can only ever refuse — see
//! [`classify_write`]. The whole thing **fails closed**: anything it cannot
//! parse, or can parse but does not recognise, is refused.

use sqlparser::ast::Statement;
use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

use crate::{DbError, SqlDialect};

/// What kind of write a statement was proven to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStatement {
    /// `INSERT` / `UPDATE` / `DELETE` / `MERGE` — changes rows.
    Data,
    /// `CREATE TABLE` / `CREATE VIEW` / `CREATE INDEX` / `CREATE SCHEMA` /
    /// `ALTER TABLE` — changes shape.
    Schema,
}

/// Why a statement was refused.
///
/// Carries whether the refusal is *permanent*, because that changes what an
/// agent should do next: a permanently closed statement will not start
/// working after a config change, and retrying it — or asking the operator
/// to enable something — is wasted effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePolicyViolation {
    reason: String,
    permanent: bool,
}

impl WritePolicyViolation {
    fn refused(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            permanent: false,
        }
    }

    fn closed(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            permanent: true,
        }
    }

    /// The category-level explanation, without any leading prefix. Never
    /// echoes the offending SQL.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Whether no configuration change can make this statement run.
    #[must_use]
    pub fn is_permanent(&self) -> bool {
        self.permanent
    }
}

impl std::fmt::Display for WritePolicyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.permanent {
            write!(
                f,
                "refused permanently: {} — dbboard never runs this through an agent; \
                 use the desktop app's SQL editor",
                self.reason
            )
        } else {
            write!(f, "not a supported write statement: {}", self.reason)
        }
    }
}

impl std::error::Error for WritePolicyViolation {}

impl From<WritePolicyViolation> for DbError {
    fn from(violation: WritePolicyViolation) -> Self {
        DbError::Query(violation.to_string())
    }
}

/// Prove `sql` is exactly one write statement dbboard is willing to run.
///
/// Reads are refused here rather than passed through: the read tools cap
/// their result sets, and letting a `SELECT` in through the write path
/// would be an uncapped read wearing the wrong name.
///
/// # Errors
///
/// Returns [`WritePolicyViolation`]. Check [`WritePolicyViolation::is_permanent`]
/// to tell "not supported" from "never".
pub fn classify_write(
    sql: &str,
    dialect: SqlDialect,
) -> Result<WriteStatement, WritePolicyViolation> {
    // Refuse-only prefilter. The AST below is the sole authority on what is
    // *permitted*; this can only ever refuse, so a wrong guess costs a
    // false refusal and never a false permit. It exists because the parser
    // does not accept every vendor spelling of `CREATE USER`, and
    // "could not be parsed" is the wrong thing to tell an agent that just
    // tried to create a login — it invites a retry with different syntax.
    if let Some(reason) = permanently_closed_prefix(sql) {
        return Err(WritePolicyViolation::closed(reason));
    }

    let parser_dialect: Box<dyn Dialect> = match dialect {
        SqlDialect::Postgres => Box::new(PostgreSqlDialect {}),
        SqlDialect::Sqlite => Box::new(SQLiteDialect {}),
        SqlDialect::MySql => Box::new(MySqlDialect {}),
    };
    let statements = Parser::parse_sql(parser_dialect.as_ref(), sql)
        .map_err(|err| WritePolicyViolation::refused(format!("could not be parsed: {err}")))?;

    match statements.as_slice() {
        [] => Err(WritePolicyViolation::refused("no statement found")),
        [single] => check_statement(single),
        many => Err(WritePolicyViolation::refused(format!(
            "expected a single statement, found {}",
            many.len()
        ))),
    }
}

/// Boolean form of [`classify_write`].
#[must_use]
pub fn is_permitted_write(sql: &str, dialect: SqlDialect) -> bool {
    classify_write(sql, dialect).is_ok()
}

/// The leading keywords of statements that are closed no matter how they
/// parse, or whether they parse at all.
///
/// Only the first two words matter, so this reads them off the front rather
/// than tokenising the whole statement: leading whitespace and comments are
/// skipped, and the scan stops as soon as it has what it needs. A string
/// literal cannot appear before the first keyword, so quoting cannot hide
/// anything from it.
fn permanently_closed_prefix(sql: &str) -> Option<&'static str> {
    let words = leading_words(sql, 2);
    let first = words.first()?.as_str();
    let second = words.get(1).map(String::as_str).unwrap_or_default();

    match (first, second) {
        ("GRANT" | "REVOKE" | "DENY", _) => Some("a privilege change (GRANT / REVOKE / DENY)"),
        (_, "USER" | "ROLE" | "GROUP")
            if matches!(first, "CREATE" | "ALTER" | "DROP" | "RENAME") =>
        {
            Some("a change to a database user or role")
        }
        ("SET", "PASSWORD") => Some("a password change"),
        ("TRUNCATE", _) => Some("TRUNCATE — it cannot be rolled back the way a DELETE can"),
        ("DROP", _) => Some("DROP — it destroys an object rather than its contents"),
        _ => None,
    }
}

/// The first `limit` bare words of `sql`, uppercased, with SQL comments and
/// whitespace skipped.
fn leading_words(sql: &str, limit: usize) -> Vec<String> {
    let bytes: Vec<char> = sql.chars().collect();
    let mut index = 0;
    let mut words = Vec::with_capacity(limit);

    while index < bytes.len() && words.len() < limit {
        let ch = bytes[index];
        if ch.is_whitespace() {
            index += 1;
        } else if ch == '-' && bytes.get(index + 1) == Some(&'-') {
            while index < bytes.len() && bytes[index] != '\n' {
                index += 1;
            }
        } else if ch == '/' && bytes.get(index + 1) == Some(&'*') {
            index += 2;
            while index < bytes.len()
                && !(bytes[index] == '*' && bytes.get(index + 1) == Some(&'/'))
            {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
        } else if ch.is_alphabetic() {
            let start = index;
            while index < bytes.len() && bytes[index].is_alphanumeric() {
                index += 1;
            }
            words.push(
                bytes[start..index]
                    .iter()
                    .collect::<String>()
                    .to_uppercase(),
            );
        } else {
            // A non-word character before the keywords we care about means
            // this is not one of the closed forms; leave it to the parser.
            break;
        }
    }

    words
}

fn check_statement(statement: &Statement) -> Result<WriteStatement, WritePolicyViolation> {
    match statement {
        Statement::Insert(_)
        | Statement::Update(_)
        | Statement::Delete(_)
        | Statement::Merge(_) => Ok(WriteStatement::Data),

        Statement::CreateTable(_)
        | Statement::CreateView(_)
        | Statement::CreateIndex(_)
        | Statement::CreateSchema { .. }
        | Statement::AlterTable(_) => Ok(WriteStatement::Schema),

        Statement::Grant(_) | Statement::Revoke(_) | Statement::Deny(_) => Err(
            WritePolicyViolation::closed("a privilege change (GRANT / REVOKE / DENY)"),
        ),

        Statement::CreateUser(_)
        | Statement::AlterUser(_)
        | Statement::CreateRole(_)
        | Statement::AlterRole { .. } => Err(WritePolicyViolation::closed(
            "a change to a database user or role",
        )),

        Statement::Truncate(_) => Err(WritePolicyViolation::closed(
            "TRUNCATE — it cannot be rolled back the way a DELETE can",
        )),

        Statement::Drop { .. }
        | Statement::DropFunction { .. }
        | Statement::DropProcedure { .. } => Err(WritePolicyViolation::closed(
            "DROP — it destroys an object rather than its contents",
        )),

        // Fail closed. A statement kind nobody listed is not thereby safe:
        // sqlparser grows variants faster than this policy is revisited, and
        // the cost of refusing a benign one is a follow-up, not a lost table.
        other => Err(WritePolicyViolation::refused(format!(
            "{} is not on the permitted list",
            statement_label(other)
        ))),
    }
}

/// A short category name for a refused statement — enough for an agent to
/// see *what* it tried, without echoing the SQL itself back into a log.
fn statement_label(statement: &Statement) -> &'static str {
    match statement {
        Statement::Query(_) => "a read query (use the read tool, which caps its rows)",
        Statement::Copy { .. } => "COPY",
        Statement::Call(_) => "CALL",
        Statement::Set(_) => "SET",
        Statement::Analyze(_) => "ANALYZE",
        Statement::CreateDatabase { .. } => "CREATE DATABASE",
        Statement::CreateFunction(_) => "CREATE FUNCTION",
        Statement::CreateProcedure { .. } => "CREATE PROCEDURE",
        Statement::CreateTrigger(_) => "CREATE TRIGGER",
        Statement::StartTransaction { .. }
        | Statement::Commit { .. }
        | Statement::Rollback { .. } => "explicit transaction control",
        _ => "that statement",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: SqlDialect = SqlDialect::Postgres;
    const MY: SqlDialect = SqlDialect::MySql;

    fn violation(sql: &str, dialect: SqlDialect) -> WritePolicyViolation {
        classify_write(sql, dialect).expect_err("must be refused")
    }

    #[test]
    fn dml_is_permitted() {
        for sql in [
            "INSERT INTO t (a) VALUES (1)",
            "UPDATE t SET a = 1 WHERE id = 2",
            "DELETE FROM t WHERE id = 2",
        ] {
            assert_eq!(
                classify_write(sql, PG).expect("permitted"),
                WriteStatement::Data,
                "{sql}"
            );
        }
    }

    #[test]
    fn the_ddl_the_maintainer_asked_for_is_permitted() {
        for sql in [
            "CREATE TABLE t (id INT)",
            "ALTER TABLE t ADD COLUMN b TEXT",
            "CREATE INDEX idx_t_a ON t (a)",
            "CREATE VIEW v AS SELECT 1",
        ] {
            assert_eq!(
                classify_write(sql, PG).expect("permitted"),
                WriteStatement::Schema,
                "{sql}"
            );
        }
    }

    #[test]
    fn privilege_changes_are_permanently_closed() {
        for sql in [
            "GRANT SELECT ON t TO alice",
            "REVOKE SELECT ON t FROM alice",
        ] {
            let refused = violation(sql, PG);
            assert!(refused.is_permanent(), "{sql} must be permanently closed");
        }
    }

    #[test]
    fn principal_changes_are_permanently_closed() {
        let refused = violation("CREATE USER alice PASSWORD 'x'", PG);
        assert!(refused.is_permanent());
        assert!(
            refused.reason().contains("user or role"),
            "reason should name the category: {}",
            refused.reason()
        );
    }

    /// The vendor spellings of `CREATE USER` that sqlparser 0.62 cannot parse
    /// must still refuse *permanently*. Without the prefilter they land on
    /// "could not be parsed", which reads as a syntax complaint and invites a
    /// retry with different syntax.
    #[test]
    fn an_unparseable_principal_change_still_refuses_permanently() {
        for sql in [
            "CREATE USER alice WITH PASSWORD 'x'",
            "CREATE USER 'a'@'%' IDENTIFIED BY 'x'",
            "SET PASSWORD FOR 'a'@'%' = 'x'",
            "DROP USER alice",
        ] {
            let refused = violation(sql, PG);
            assert!(refused.is_permanent(), "should be permanent: {sql}");
            assert!(
                !refused.reason().contains("could not be parsed"),
                "reason should name the category, not the parser: {}",
                refused.reason()
            );
        }
    }

    /// The prefilter reads two words, so the DDL the maintainer asked for must
    /// not be caught by the `CREATE` / `ALTER` arms meant for principals.
    #[test]
    fn the_prefilter_does_not_catch_table_ddl() {
        assert!(permanently_closed_prefix("CREATE TABLE users (id int)").is_none());
        assert!(permanently_closed_prefix("ALTER TABLE users ADD COLUMN a int").is_none());
        assert!(permanently_closed_prefix("  -- note\n  UPDATE t SET a = 1").is_none());
    }

    /// A comment cannot smuggle a closed statement past the prefilter either.
    #[test]
    fn a_comment_cannot_disguise_a_grant() {
        let refused = violation("/* harmless */ GRANT ALL ON t TO alice", PG);
        assert!(refused.is_permanent());
        assert!(refused.reason().contains("privilege"));
    }

    #[test]
    fn truncate_is_permanently_closed_even_though_delete_is_not() {
        assert_eq!(
            classify_write("DELETE FROM t", PG).expect("delete is permitted"),
            WriteStatement::Data,
        );
        let refused = violation("TRUNCATE TABLE t", PG);
        assert!(refused.is_permanent());
    }

    #[test]
    fn drop_is_permanently_closed_even_though_alter_is_not() {
        assert_eq!(
            classify_write("ALTER TABLE t DROP COLUMN b", PG).expect("alter is permitted"),
            WriteStatement::Schema,
        );
        let refused = violation("DROP TABLE t", PG);
        assert!(refused.is_permanent());
    }

    /// `CREATE INDEX` is permitted, so "drop" reads like its undo — but a
    /// dropped index takes a rebuild to get back, and on a large table that
    /// rebuild is an outage. `DROP` is closed for every object, with no
    /// exception carved out here.
    #[test]
    fn dropping_an_index_is_closed_even_though_creating_one_is_not() {
        assert_eq!(
            classify_write("CREATE INDEX idx_users_email ON users (email)", PG)
                .expect("create index is permitted"),
            WriteStatement::Schema,
        );
        let refused = violation("DROP INDEX idx_users_email", PG);
        assert!(refused.is_permanent());
    }

    /// `COMMENT ON` changes no data and no shape, but it is not on the list,
    /// and "not listed" means refused rather than waved through.
    #[test]
    fn commenting_is_refused_because_nothing_listed_it() {
        let refused = violation("COMMENT ON TABLE t IS 'note'", PG);
        assert!(
            !refused.is_permanent(),
            "nothing about a comment is dangerous; it is simply unlisted"
        );
    }

    #[test]
    fn a_read_is_refused_so_it_cannot_dodge_the_row_cap() {
        let refused = violation("SELECT * FROM t", PG);
        assert!(
            !refused.is_permanent(),
            "a read is not forbidden, just misrouted"
        );
        assert!(
            refused.reason().contains("read tool"),
            "the refusal should point at the right tool: {}",
            refused.reason()
        );
    }

    #[test]
    fn a_batch_is_refused_however_it_ends() {
        // The Postgres simple query protocol would run both halves. Whether
        // the tail is benign or a DROP, one call means one statement.
        let refused = violation("UPDATE t SET a = 1; DROP TABLE t", PG);
        assert!(refused.reason().contains("single statement"));
    }

    #[test]
    fn unparseable_input_fails_closed() {
        let refused = violation("UPDATE ((( ", PG);
        assert!(refused.reason().contains("could not be parsed"));
    }

    #[test]
    fn a_comment_cannot_disguise_a_drop() {
        let refused = violation("/* UPDATE t SET a = 1 */ DROP TABLE t", PG);
        assert!(refused.is_permanent(), "leading text is not the statement");
    }

    #[test]
    fn transaction_control_is_refused_so_a_write_cannot_be_left_open() {
        let refused = violation("BEGIN", PG);
        assert!(!refused.is_permanent());
    }

    #[test]
    fn the_mysql_dialect_classifies_the_same_way() {
        assert_eq!(
            classify_write("UPDATE `t` SET `a` = 1 WHERE `id` = 2", MY).expect("permitted"),
            WriteStatement::Data,
        );
        assert!(violation("TRUNCATE `t`", MY).is_permanent());
        assert!(violation("GRANT ALL ON `d`.* TO 'a'@'%'", MY).is_permanent());
    }

    #[test]
    fn a_refusal_never_echoes_the_statement() {
        let sql = "DROP TABLE super_secret_table_name";
        let refused = violation(sql, PG);
        assert!(
            !refused.to_string().contains("super_secret_table_name"),
            "refusals must not reflect input: {refused}"
        );
    }

    #[test]
    fn a_permanent_refusal_says_where_to_go_instead() {
        let refused = violation("DROP TABLE t", PG);
        assert!(refused.to_string().contains("SQL editor"));
    }

    #[test]
    fn is_permitted_write_agrees_with_classify() {
        assert!(is_permitted_write("INSERT INTO t (a) VALUES (1)", PG));
        assert!(!is_permitted_write("DROP TABLE t", PG));
    }
}
