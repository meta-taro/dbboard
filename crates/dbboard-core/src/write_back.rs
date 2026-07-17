//! Pure write-back SQL generation for inline cell editing (ADR-0042).
//!
//! This is **slice a** of issue 0013: the app's first mutation path,
//! contained entirely in a pure, no-I/O module next to the adapter
//! contract (CLAUDE.md forbids business logic in egui handlers). It turns
//! a set of staged cell edits plus a row-identity key into a single
//! `UPDATE … SET … WHERE …` string that the existing `query(sql)` path
//! executes — **no new adapter method and no HTTP contract change**, so
//! the first write path stays desktop-only / in-process.
//!
//! Safety is by construction, not by trust:
//!
//! - **Identifiers** are double-quoted with embedded `"` doubled
//!   (`"a""b"`) — identical for SQLite and Postgres.
//! - **Edited values** are emitted as single-quoted string literals (with
//!   `'` doubled) or the bare keyword `NULL`. The editor only produces
//!   text, so every value is written as a string literal and the engine
//!   coerces it by the target column's type (SQLite affinity; Postgres
//!   assignment cast from an `unknown` literal). `NULL` is the one value
//!   that is not text and gets its own variant.
//! - **Identity values** come typed from the row (`Value`), so the
//!   `WHERE` key encodes them by their real type (bare number vs quoted
//!   text vs `IS NULL`) rather than round-tripping through text.
//!
//! No user text is ever concatenated unescaped.

use crate::schema::{TableInfo, TableSchema};
use crate::value::Value;

/// SQL dialect family. Drives schema qualification and which implicit
/// row identity is allowed. Placeholder style is irrelevant here because
/// values are literal-encoded, not bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    /// SQLite family: Turso / libSQL, Cloudflare D1. No schema namespace;
    /// an implicit `rowid` identifies rows on ordinary tables.
    Sqlite,
    /// Postgres family: Supabase, Neon, Aurora DSQL. Tables live in a
    /// schema; there is no safe implicit row key (`ctid` is unstable).
    Postgres,
}

/// What can identify a row for a safe `WHERE` — the *capability*, decided
/// from the table schema and dialect by [`RowIdentity::resolve`]. A table
/// with no resolvable identity is not editable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowIdentity {
    /// Key on the declared primary-key columns (in key order).
    PrimaryKey(Vec<String>),
    /// SQLite-family implicit `rowid` (ordinary, non-`WITHOUT ROWID`
    /// tables that lack a declared primary key).
    SqliteRowid,
}

impl RowIdentity {
    /// Decide how — or whether — a row from `schema` can be identified.
    ///
    /// Returns `None` (= refuse editing) when there is no safe key:
    /// a Postgres table with no primary key, or a SQLite `WITHOUT ROWID`
    /// table with no primary key. A declared primary key always wins.
    ///
    /// `without_rowid` is the caller's knowledge that a SQLite table was
    /// declared `WITHOUT ROWID` (those have no usable `rowid`); it is
    /// ignored for Postgres.
    #[must_use]
    pub fn resolve(
        schema: &TableSchema,
        dialect: SqlDialect,
        without_rowid: bool,
    ) -> Option<RowIdentity> {
        if !schema.primary_key.is_empty() {
            return Some(RowIdentity::PrimaryKey(schema.primary_key.clone()));
        }
        match dialect {
            // SQLite gives ordinary tables a usable rowid even without a
            // declared PK; WITHOUT ROWID tables are the exception.
            SqlDialect::Sqlite if !without_rowid => Some(RowIdentity::SqliteRowid),
            // No declared key and no safe implicit one → not editable.
            SqlDialect::Sqlite | SqlDialect::Postgres => None,
        }
    }
}

/// A staged new value for one cell. The editor works on text, so a value
/// is either text (written as a coerced string literal) or an explicit
/// SQL `NULL` — never "empty string standing in for null".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellValue {
    /// Explicit SQL `NULL`.
    Null,
    /// Text from the editor, emitted as a `'…'` literal and coerced by the
    /// target column's type.
    Text(String),
}

/// The concrete `WHERE` key for one row: the identity columns' *original*
/// values, or a SQLite `rowid`.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKey {
    /// Named identity columns paired with the row's original values.
    Columns(Vec<(String, Value)>),
    /// SQLite implicit rowid.
    Rowid(i64),
}

/// A single-row `UPDATE`: the base table, the `WHERE` key, and the columns
/// that changed with their new values.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdatePlan {
    pub table: TableInfo,
    pub key: RowKey,
    /// `(column, new value)` pairs. Order is preserved in the emitted
    /// `SET` clause so output is deterministic (stable tests, readable
    /// history).
    pub edits: Vec<(String, CellValue)>,
}

/// Why an [`UpdatePlan`] could not be turned into SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteBackError {
    /// No columns were edited — there is nothing to write.
    NoEdits,
    /// The `WHERE` key has no columns — refusing to build an unkeyed
    /// `UPDATE` that could rewrite the whole table.
    EmptyKey,
    /// An identity column's value is a blob, which has no safe literal
    /// form for a `WHERE` comparison.
    UnsupportedKeyType(String),
}

impl std::fmt::Display for WriteBackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEdits => write!(f, "no columns were edited"),
            Self::EmptyKey => write!(f, "no row-identity columns to key the update on"),
            Self::UnsupportedKeyType(col) => {
                write!(f, "identity column {col:?} has an unsupported (blob) value")
            }
        }
    }
}

impl std::error::Error for WriteBackError {}

/// Build a single-row `UPDATE` for `plan` in `dialect`.
///
/// The output is a complete, fully-escaped SQL string:
/// `UPDATE <table> SET <col> = <lit>, … WHERE <key> [AND …]`.
///
/// # Errors
///
/// - [`WriteBackError::NoEdits`] when `plan.edits` is empty.
/// - [`WriteBackError::EmptyKey`] when the key has no columns.
/// - [`WriteBackError::UnsupportedKeyType`] when an identity value is a
///   blob.
pub fn build_update_sql(plan: &UpdatePlan, dialect: SqlDialect) -> Result<String, WriteBackError> {
    if plan.edits.is_empty() {
        return Err(WriteBackError::NoEdits);
    }

    let set = plan
        .edits
        .iter()
        .map(|(col, val)| format!("{} = {}", quote_ident(col), edit_literal(val)))
        .collect::<Vec<_>>()
        .join(", ");

    let where_clause = build_where(&plan.key)?;

    Ok(format!(
        "UPDATE {} SET {} WHERE {}",
        qualified_table(&plan.table, dialect),
        set,
        where_clause
    ))
}

fn build_where(key: &RowKey) -> Result<String, WriteBackError> {
    match key {
        RowKey::Rowid(id) => Ok(format!("rowid = {id}")),
        RowKey::Columns(cols) => {
            if cols.is_empty() {
                return Err(WriteBackError::EmptyKey);
            }
            let mut parts = Vec::with_capacity(cols.len());
            for (col, val) in cols {
                parts.push(key_predicate(col, val)?);
            }
            Ok(parts.join(" AND "))
        }
    }
}

/// One `WHERE` predicate for an identity column, encoding the original
/// value by its real type. `NULL` becomes `IS NULL` (a primary key never
/// is, but a unique-key fallback could be).
fn key_predicate(col: &str, val: &Value) -> Result<String, WriteBackError> {
    let ident = quote_ident(col);
    Ok(match val {
        Value::Null => format!("{ident} IS NULL"),
        Value::Integer(n) => format!("{ident} = {n}"),
        Value::Real(x) => format!("{ident} = {x}"),
        Value::Text(s) => format!("{ident} = {}", quote_str(s)),
        Value::Blob(_) => return Err(WriteBackError::UnsupportedKeyType(col.to_owned())),
    })
}

/// A staged edit value as a SQL literal. Text is always quoted and left
/// for the engine to coerce to the column type; `NULL` is a bare keyword.
fn edit_literal(val: &CellValue) -> String {
    match val {
        CellValue::Null => "NULL".to_owned(),
        CellValue::Text(s) => quote_str(s),
    }
}

/// The table name for the `UPDATE` target. Postgres qualifies with the
/// schema when present; SQLite has no schema namespace so the name stands
/// alone even if a `schema` slipped into `TableInfo`.
fn qualified_table(table: &TableInfo, dialect: SqlDialect) -> String {
    match (dialect, &table.schema) {
        (SqlDialect::Postgres, Some(schema)) => {
            format!("{}.{}", quote_ident(schema), quote_ident(&table.name))
        }
        _ => quote_ident(&table.name),
    }
}

/// Quote a SQL identifier, doubling any embedded double-quote. Valid for
/// both SQLite and Postgres.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Quote a SQL string literal, doubling any embedded single-quote.
fn quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ColumnInfo;

    fn col(name: &str, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_owned(),
            declared_type: Some("TEXT".to_owned()),
            nullable: !pk,
            primary_key: pk,
            ordinal: 0,
            default_value: None,
        }
    }

    fn schema_with(table: TableInfo, cols: Vec<ColumnInfo>, pk: Vec<&str>) -> TableSchema {
        TableSchema {
            table,
            columns: cols,
            primary_key: pk.into_iter().map(String::from).collect(),
        }
    }

    // ---- RowIdentity::resolve -------------------------------------------

    #[test]
    fn resolve_prefers_the_declared_primary_key_on_both_dialects() {
        let schema = schema_with(
            TableInfo::unqualified("t"),
            vec![col("id", true), col("name", false)],
            vec!["id"],
        );
        for d in [SqlDialect::Sqlite, SqlDialect::Postgres] {
            assert_eq!(
                RowIdentity::resolve(&schema, d, false),
                Some(RowIdentity::PrimaryKey(vec!["id".to_owned()]))
            );
        }
    }

    #[test]
    fn resolve_keeps_composite_primary_key_order() {
        let schema = schema_with(
            TableInfo::unqualified("lines"),
            vec![col("order_id", true), col("line_no", true)],
            vec!["order_id", "line_no"],
        );
        assert_eq!(
            RowIdentity::resolve(&schema, SqlDialect::Postgres, false),
            Some(RowIdentity::PrimaryKey(vec![
                "order_id".to_owned(),
                "line_no".to_owned()
            ]))
        );
    }

    #[test]
    fn resolve_falls_back_to_rowid_on_sqlite_without_a_pk() {
        let schema = schema_with(TableInfo::unqualified("t"), vec![col("a", false)], vec![]);
        assert_eq!(
            RowIdentity::resolve(&schema, SqlDialect::Sqlite, false),
            Some(RowIdentity::SqliteRowid)
        );
    }

    #[test]
    fn resolve_refuses_sqlite_without_rowid_and_no_pk() {
        let schema = schema_with(TableInfo::unqualified("t"), vec![col("a", false)], vec![]);
        assert_eq!(
            RowIdentity::resolve(&schema, SqlDialect::Sqlite, true),
            None
        );
    }

    #[test]
    fn resolve_refuses_postgres_without_a_pk() {
        // Postgres has no safe implicit key, so no PK ⇒ not editable
        // regardless of the rowid flag.
        let schema = schema_with(TableInfo::unqualified("t"), vec![col("a", false)], vec![]);
        assert_eq!(
            RowIdentity::resolve(&schema, SqlDialect::Postgres, false),
            None
        );
    }

    // ---- build_update_sql: happy paths ----------------------------------

    #[test]
    fn builds_a_single_column_update_keyed_on_an_integer_pk() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("widgets"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(7))]),
            edits: vec![("name".to_owned(), CellValue::Text("gadget".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(
            sql,
            r#"UPDATE "widgets" SET "name" = 'gadget' WHERE "id" = 7"#
        );
    }

    #[test]
    fn multiple_edits_keep_their_order_in_the_set_clause() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(1))]),
            edits: vec![
                ("b".to_owned(), CellValue::Text("2".to_owned())),
                ("a".to_owned(), CellValue::Text("1".to_owned())),
            ],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(sql, r#"UPDATE "t" SET "b" = '2', "a" = '1' WHERE "id" = 1"#);
    }

    #[test]
    fn null_edit_emits_the_bare_keyword_not_a_string() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(1))]),
            edits: vec![("note".to_owned(), CellValue::Null)],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(sql, r#"UPDATE "t" SET "note" = NULL WHERE "id" = 1"#);
    }

    #[test]
    fn postgres_qualifies_the_table_with_its_schema() {
        let plan = UpdatePlan {
            table: TableInfo::qualified("public", "users"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(1))]),
            edits: vec![("email".to_owned(), CellValue::Text("x@y.z".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Postgres).unwrap();
        assert_eq!(
            sql,
            r#"UPDATE "public"."users" SET "email" = 'x@y.z' WHERE "id" = 1"#
        );
    }

    #[test]
    fn sqlite_ignores_a_schema_that_slipped_into_table_info() {
        let plan = UpdatePlan {
            table: TableInfo::qualified("main", "t"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(1))]),
            edits: vec![("a".to_owned(), CellValue::Text("v".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(sql, r#"UPDATE "t" SET "a" = 'v' WHERE "id" = 1"#);
    }

    #[test]
    fn composite_key_joins_predicates_with_and() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("lines"),
            key: RowKey::Columns(vec![
                ("order_id".to_owned(), Value::Integer(10)),
                ("line_no".to_owned(), Value::Integer(2)),
            ]),
            edits: vec![("qty".to_owned(), CellValue::Text("5".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Postgres).unwrap();
        assert_eq!(
            sql,
            r#"UPDATE "lines" SET "qty" = '5' WHERE "order_id" = 10 AND "line_no" = 2"#
        );
    }

    #[test]
    fn rowid_key_emits_an_unquoted_rowid_predicate() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Rowid(42),
            edits: vec![("a".to_owned(), CellValue::Text("v".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(sql, r#"UPDATE "t" SET "a" = 'v' WHERE rowid = 42"#);
    }

    #[test]
    fn text_identity_value_is_quoted_and_null_becomes_is_null() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![
                ("code".to_owned(), Value::Text("A1".to_owned())),
                ("region".to_owned(), Value::Null),
            ]),
            edits: vec![("v".to_owned(), CellValue::Text("x".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Postgres).unwrap();
        assert_eq!(
            sql,
            r#"UPDATE "t" SET "v" = 'x' WHERE "code" = 'A1' AND "region" IS NULL"#
        );
    }

    // ---- build_update_sql: injection / escaping -------------------------

    #[test]
    fn single_quotes_in_edited_text_are_doubled() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(1))]),
            edits: vec![(
                "name".to_owned(),
                CellValue::Text("O'Brien'; DROP TABLE t;--".to_owned()),
            )],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        // The whole payload stays inside one escaped string literal; no
        // statement break escapes the quotes.
        assert_eq!(
            sql,
            r#"UPDATE "t" SET "name" = 'O''Brien''; DROP TABLE t;--' WHERE "id" = 1"#
        );
    }

    #[test]
    fn double_quotes_in_identifiers_are_doubled() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified(r#"we"ird"#),
            key: RowKey::Columns(vec![(r#"i"d"#.to_owned(), Value::Integer(1))]),
            edits: vec![(r#"c"l"#.to_owned(), CellValue::Text("v".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(sql, r#"UPDATE "we""ird" SET "c""l" = 'v' WHERE "i""d" = 1"#);
    }

    #[test]
    fn single_quotes_in_a_text_identity_value_are_doubled() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![("code".to_owned(), Value::Text("a'b".to_owned()))]),
            edits: vec![("v".to_owned(), CellValue::Text("x".to_owned()))],
        };
        let sql = build_update_sql(&plan, SqlDialect::Sqlite).unwrap();
        assert_eq!(sql, r#"UPDATE "t" SET "v" = 'x' WHERE "code" = 'a''b'"#);
    }

    // ---- build_update_sql: refusals -------------------------------------

    #[test]
    fn no_edits_is_refused() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![("id".to_owned(), Value::Integer(1))]),
            edits: vec![],
        };
        assert_eq!(
            build_update_sql(&plan, SqlDialect::Sqlite),
            Err(WriteBackError::NoEdits)
        );
    }

    #[test]
    fn an_empty_column_key_is_refused() {
        // Guards against an unkeyed UPDATE that would rewrite every row.
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![]),
            edits: vec![("a".to_owned(), CellValue::Text("v".to_owned()))],
        };
        assert_eq!(
            build_update_sql(&plan, SqlDialect::Sqlite),
            Err(WriteBackError::EmptyKey)
        );
    }

    #[test]
    fn a_blob_identity_value_is_refused() {
        let plan = UpdatePlan {
            table: TableInfo::unqualified("t"),
            key: RowKey::Columns(vec![("k".to_owned(), Value::Blob(vec![1, 2, 3]))]),
            edits: vec![("a".to_owned(), CellValue::Text("v".to_owned()))],
        };
        assert_eq!(
            build_update_sql(&plan, SqlDialect::Sqlite),
            Err(WriteBackError::UnsupportedKeyType("k".to_owned()))
        );
    }
}
