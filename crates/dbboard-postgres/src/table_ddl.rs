//! `CREATE` DDL reconstruction for a Postgres-wire table (ADR-0049 slice
//! c2).
//!
//! The [`assemble_table_ddl`] function is a *pure* assembler: it takes the
//! decoded rows of four `pg_catalog` introspection queries — columns,
//! constraints, standalone indexes, and owned sequences — and renders the
//! `CREATE` statements that reconstruct the table. Keeping it free of any
//! `sqlx`/pool dependency makes the interesting logic (identifier quoting,
//! statement ordering, sequence parameters, the DSQL degradation) unit-
//! testable without a live database; the adapter method in `lib.rs` only
//! runs the queries and decodes each row into the structs below.
//!
//! Fidelity notes:
//! - Constraint and index text comes verbatim from `pg_get_constraintdef`
//!   / `pg_get_indexdef`, so Postgres itself owns the hard escaping.
//! - Constraints are emitted *inline* in the `CREATE TABLE`, names
//!   preserved. In a multi-table dump this means a forward-referencing
//!   `FOREIGN KEY` can precede its target — acceptable because ADR-0049 is
//!   dump-only and makes no re-import promise (Decision 0).
//! - Aurora DSQL has no foreign keys and no sequences (ADR-0021), so those
//!   catalog queries return nothing and the corresponding sections are
//!   simply omitted — the "degrades by construction" of Decision 6.

/// One column of the table, decoded from the `pg_attribute` /
/// `format_type` / `pg_get_expr` introspection query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnDef {
    pub name: String,
    /// `format_type(atttypid, atttypmod)` output, e.g. `character
    /// varying(255)`, `integer`, `numeric(10,2)`. Already canonical.
    pub type_name: String,
    pub not_null: bool,
    /// `pg_get_expr(adbin, adrelid)` of the column default, verbatim
    /// (e.g. `nextval('s'::regclass)`, `now()`, `'x'::text`). `None` when
    /// the column has no default.
    pub default_expr: Option<String>,
}

/// One table constraint, decoded from `pg_constraint`. `def` is the
/// verbatim `pg_get_constraintdef` body (e.g. `PRIMARY KEY (id)`,
/// `FOREIGN KEY (a) REFERENCES other(b)`, `CHECK ((n > 0))`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstraintDef {
    pub name: String,
    pub def: String,
}

/// One owned sequence (a `SERIAL`/`GENERATED` column's backing sequence),
/// decoded from `pg_sequence`. Emitted ahead of the table so the column
/// default that references it resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequenceDef {
    pub schema: String,
    pub name: String,
    /// `format_type` of the sequence element type: `bigint`, `integer`,
    /// or `smallint`.
    pub type_name: String,
    pub start: i64,
    pub increment: i64,
    pub min_value: i64,
    pub max_value: i64,
    pub cache: i64,
    pub cycle: bool,
}

/// The decoded inputs for one table's DDL. `constraints`, `indexes`, and
/// `sequences` may each be empty (a table with no constraints, or DSQL's
/// missing FK/sequence catalogs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableDdlParts {
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnDef>,
    pub constraints: Vec<ConstraintDef>,
    /// Verbatim `pg_get_indexdef` statements for indexes *not* backing a
    /// constraint (constraint-backed indexes are recreated by the
    /// constraint itself).
    pub indexes: Vec<String>,
    pub sequences: Vec<SequenceDef>,
}

/// Quote a Postgres identifier by wrapping it in double quotes and
/// doubling any embedded double quote. Applied to names dbboard controls
/// (column, table, schema, sequence); constraint/index text is already
/// quoted by `pg_get_*def`.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// `"schema"."name"` — a schema-qualified identifier.
fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(name))
}

fn render_column(col: &ColumnDef) -> String {
    let mut line = format!("{} {}", quote_ident(&col.name), col.type_name);
    if col.not_null {
        line.push_str(" NOT NULL");
    }
    if let Some(default) = &col.default_expr {
        line.push_str(" DEFAULT ");
        line.push_str(default);
    }
    line
}

fn render_sequence(seq: &SequenceDef) -> String {
    let mut stmt = format!(
        "CREATE SEQUENCE {} AS {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} CACHE {}",
        qualified(&seq.schema, &seq.name),
        seq.type_name,
        seq.start,
        seq.increment,
        seq.min_value,
        seq.max_value,
        seq.cache,
    );
    if seq.cycle {
        stmt.push_str(" CYCLE");
    }
    stmt.push(';');
    stmt
}

/// Assemble the full DDL for one table: owned sequences first, then the
/// `CREATE TABLE` (columns then inline constraints), then any standalone
/// indexes. Every statement is `;`-terminated and newline-separated.
pub(crate) fn assemble_table_ddl(parts: &TableDdlParts) -> String {
    let mut out = String::new();

    for seq in &parts.sequences {
        out.push_str(&render_sequence(seq));
        out.push('\n');
    }

    out.push_str("CREATE TABLE ");
    out.push_str(&qualified(&parts.schema, &parts.table));
    out.push_str(" (\n");

    // Columns and inline constraints share one comma-separated list.
    let mut items: Vec<String> = Vec::with_capacity(parts.columns.len() + parts.constraints.len());
    for col in &parts.columns {
        items.push(render_column(col));
    }
    for con in &parts.constraints {
        items.push(format!("CONSTRAINT {} {}", quote_ident(&con.name), con.def));
    }
    let body = items
        .iter()
        .map(|item| format!("    {item}"))
        .collect::<Vec<_>>()
        .join(",\n");
    out.push_str(&body);
    out.push_str("\n);\n");

    for index in &parts.indexes {
        out.push_str(index.trim_end());
        out.push_str(";\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, type_name: &str, not_null: bool, default: Option<&str>) -> ColumnDef {
        ColumnDef {
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            not_null,
            default_expr: default.map(str::to_owned),
        }
    }

    fn base_parts() -> TableDdlParts {
        TableDdlParts {
            schema: "public".to_owned(),
            table: "users".to_owned(),
            columns: vec![
                col("id", "integer", true, None),
                col("email", "character varying(255)", true, None),
            ],
            constraints: vec![],
            indexes: vec![],
            sequences: vec![],
        }
    }

    #[test]
    fn a_bare_table_renders_columns_only() {
        let ddl = assemble_table_ddl(&base_parts());
        assert_eq!(
            ddl,
            "CREATE TABLE \"public\".\"users\" (\n\
             \x20   \"id\" integer NOT NULL,\n\
             \x20   \"email\" character varying(255) NOT NULL\n\
             );\n"
        );
    }

    #[test]
    fn a_nullable_column_omits_not_null() {
        let mut parts = base_parts();
        parts.columns = vec![col("note", "text", false, None)];
        let ddl = assemble_table_ddl(&parts);
        assert!(ddl.contains("\"note\" text\n"), "unexpected: {ddl}");
        assert!(!ddl.contains("NOT NULL"));
    }

    #[test]
    fn a_default_is_rendered_verbatim() {
        let mut parts = base_parts();
        parts.columns = vec![col("created", "timestamptz", true, Some("now()"))];
        let ddl = assemble_table_ddl(&parts);
        assert!(
            ddl.contains("\"created\" timestamptz NOT NULL DEFAULT now()"),
            "unexpected: {ddl}"
        );
    }

    #[test]
    fn constraints_are_inlined_with_their_names() {
        let mut parts = base_parts();
        parts.constraints = vec![
            ConstraintDef {
                name: "users_pkey".to_owned(),
                def: "PRIMARY KEY (id)".to_owned(),
            },
            ConstraintDef {
                name: "users_email_check".to_owned(),
                def: "CHECK ((char_length(email) > 0))".to_owned(),
            },
        ];
        let ddl = assemble_table_ddl(&parts);
        assert!(ddl.contains("    CONSTRAINT \"users_pkey\" PRIMARY KEY (id),\n"));
        assert!(ddl.contains(
            "    CONSTRAINT \"users_email_check\" CHECK ((char_length(email) > 0))\n);\n"
        ));
    }

    #[test]
    fn standalone_indexes_follow_the_table_each_terminated() {
        let mut parts = base_parts();
        parts.indexes =
            vec!["CREATE INDEX idx_users_email ON public.users USING btree (email)".to_owned()];
        let ddl = assemble_table_ddl(&parts);
        assert!(ddl
            .trim_end()
            .ends_with("CREATE INDEX idx_users_email ON public.users USING btree (email);"));
    }

    #[test]
    fn owned_sequences_precede_the_table() {
        let mut parts = base_parts();
        parts.sequences = vec![SequenceDef {
            schema: "public".to_owned(),
            name: "users_id_seq".to_owned(),
            type_name: "integer".to_owned(),
            start: 1,
            increment: 1,
            min_value: 1,
            max_value: 2_147_483_647,
            cache: 1,
            cycle: false,
        }];
        parts.columns = vec![col(
            "id",
            "integer",
            true,
            Some("nextval('public.users_id_seq'::regclass)"),
        )];
        let ddl = assemble_table_ddl(&parts);
        let seq_at = ddl.find("CREATE SEQUENCE").expect("sequence present");
        let table_at = ddl.find("CREATE TABLE").expect("table present");
        assert!(seq_at < table_at, "sequence must precede table:\n{ddl}");
        assert!(ddl.contains(
            "CREATE SEQUENCE \"public\".\"users_id_seq\" AS integer START WITH 1 \
             INCREMENT BY 1 MINVALUE 1 MAXVALUE 2147483647 CACHE 1;"
        ));
        // The column keeps its nextval default so it wires to the sequence.
        assert!(ddl.contains("DEFAULT nextval('public.users_id_seq'::regclass)"));
    }

    #[test]
    fn a_cycling_sequence_appends_cycle() {
        let seq = SequenceDef {
            schema: "public".to_owned(),
            name: "s".to_owned(),
            type_name: "bigint".to_owned(),
            start: 5,
            increment: 2,
            min_value: 1,
            max_value: 100,
            cache: 10,
            cycle: true,
        };
        assert_eq!(
            render_sequence(&seq),
            "CREATE SEQUENCE \"public\".\"s\" AS bigint START WITH 5 INCREMENT BY 2 \
             MINVALUE 1 MAXVALUE 100 CACHE 10 CYCLE;"
        );
    }

    #[test]
    fn identifiers_with_double_quotes_are_escaped() {
        let mut parts = base_parts();
        parts.schema = "we\"ird".to_owned();
        parts.table = "ta\"ble".to_owned();
        parts.columns = vec![col("c\"ol", "text", false, None)];
        let ddl = assemble_table_ddl(&parts);
        assert!(ddl.contains("CREATE TABLE \"we\"\"ird\".\"ta\"\"ble\""));
        assert!(ddl.contains("\"c\"\"ol\" text"));
    }

    #[test]
    fn dsql_shaped_input_without_fks_or_sequences_still_renders() {
        // Aurora DSQL returns no foreign-key constraints and no sequences;
        // the assembler must produce a valid table from what remains
        // (columns + a primary key + a standalone index).
        let mut parts = base_parts();
        parts.constraints = vec![ConstraintDef {
            name: "users_pkey".to_owned(),
            def: "PRIMARY KEY (id)".to_owned(),
        }];
        parts.indexes = vec!["CREATE INDEX idx ON public.users USING btree (email)".to_owned()];
        // No sequences, no FK.
        let ddl = assemble_table_ddl(&parts);
        assert!(!ddl.contains("CREATE SEQUENCE"));
        assert!(!ddl.contains("FOREIGN KEY"));
        assert!(ddl.contains("CONSTRAINT \"users_pkey\" PRIMARY KEY (id)"));
        assert!(ddl.contains("CREATE INDEX idx ON public.users"));
    }
}
