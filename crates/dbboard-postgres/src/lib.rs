//! PostgreSQL-wire adapter for dbboard.
//!
//! `CockroachDB` speaks the PostgreSQL wire protocol, so a desktop client
//! reaches it with an ordinary `postgresql://…` connection string and a
//! Postgres driver. This adapter uses `sqlx` over a `PgPool` and
//! implements the workspace-wide [`DatabaseAdapter`] contract
//! (ADR-0012); the only optional capability advertised is
//! `describe_table` (ADR-0028).
//!
//! The crate is deliberately generic: `CockroachDB` is the first target,
//! but Neon and any other PostgreSQL-wire database connect the same way
//! by pointing the connection string at them.
//!
//! # Dynamic decoding
//!
//! `dbboard-core`'s [`Value`] has only the five SQLite storage classes,
//! while PostgreSQL has a rich type system. Rather than enumerate every
//! type, we run statements through [`sqlx::raw_sql`] (the simple query
//! protocol), which makes the server return every value in its **text**
//! representation. Each cell is then read as a string and surfaced as
//! [`Value::Text`] (NULL becomes [`Value::Null`]). This is lossless for
//! `int8`/`numeric` and covers every type — including `uuid`,
//! `timestamptz`, `jsonb`, arrays, and user-defined types — without
//! pulling in per-type decode features.

use async_trait::async_trait;
use dbboard_core::{
    too_many_rows_error, Capabilities, Column, ColumnInfo, DatabaseAdapter, DbError, DbResult,
    QueryResult, Row, TableInfo, TableSchema, Value, MAX_RESULT_ROWS,
};
use futures_util::TryStreamExt;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow, PgSslMode, PgValueRef};
use sqlx::{Column as _, Either, Row as _, TypeInfo as _, ValueRef as _};

/// Small pool: a desktop client issues one statement at a time, so a
/// handful of connections is plenty and keeps server-side resource use
/// (and `CockroachDB` Cloud connection limits) modest.
const MAX_CONNECTIONS: u32 = 5;

/// Cap on error text surfaced into a [`DbError`], so a hostile or
/// oversized server message cannot dump an unbounded string into the UI.
const MAX_ERROR_DETAIL: usize = 2048;

/// Lists user tables across schemas, excluding the system catalogs.
/// `crdb_internal` is `CockroachDB`-specific and must be excluded too.
const LIST_TABLES_SQL: &str = "SELECT table_schema, table_name FROM information_schema.tables \
     WHERE table_schema NOT IN ('pg_catalog', 'information_schema', 'crdb_internal') \
     AND table_type = 'BASE TABLE' \
     ORDER BY table_schema, table_name";

/// Columns of one table in ordinal order (ADR-0028). Each text column is
/// cast to `TEXT` so the `information_schema` domain types
/// (`sql_identifier`, `character_data`, ...) decode as plain strings
/// under the extended protocol, and `ordinal_position` is cast to `INT4`
/// because `CockroachDB` reports it as `INT8`.
const DESCRIBE_COLUMNS_SQL: &str = "SELECT column_name::TEXT, data_type::TEXT, \
     is_nullable::TEXT, column_default::TEXT, ordinal_position::INT4, \
     col_description(format('%I.%I', table_schema, table_name)::regclass, \
     ordinal_position)::TEXT AS comment \
     FROM information_schema.columns \
     WHERE table_schema = $1 AND table_name = $2 \
     ORDER BY ordinal_position";

/// Primary-key column names of one table in key order (ADR-0028).
const DESCRIBE_PK_SQL: &str = "SELECT kcu.column_name::TEXT \
     FROM information_schema.table_constraints tc \
     JOIN information_schema.key_column_usage kcu \
       ON kcu.constraint_name = tc.constraint_name \
      AND kcu.constraint_schema = tc.constraint_schema \
      AND kcu.table_schema = tc.table_schema \
      AND kcu.table_name = tc.table_name \
     WHERE tc.constraint_type = 'PRIMARY KEY' \
       AND tc.table_schema = $1 AND tc.table_name = $2 \
     ORDER BY kcu.ordinal_position";

/// Relation kind (`r` table, `p` partitioned table, `v` view,
/// `m` materialized view, …) for one object (ADR-0038 slice b). Used to
/// pick the DDL source: views expose their definition via
/// `pg_get_viewdef`, while tables are reconstructed from their columns.
const RELKIND_SQL: &str = "SELECT c.relkind::TEXT \
     FROM pg_catalog.pg_class c \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2";

/// The `SELECT` body of a view / materialized view, pretty-printed
/// (ADR-0038 slice b). `format('%I.%I', …)` quotes the identifiers so
/// mixed-case and reserved-word names resolve through the `regclass` cast.
const VIEWDEF_SQL: &str = "SELECT pg_get_viewdef(format('%I.%I', $1, $2)::regclass, true)::TEXT";

/// Connection parameters for a PostgreSQL-wire database.
///
/// `url` is a secret: it embeds the password and is never logged, never
/// echoed in a [`DbError`], and never derived into `Debug`.
pub struct PostgresConfig {
    pub url: String,
}

/// Stable adapter identifier reported by [`DatabaseAdapter::id`] for a
/// generic PostgreSQL-wire connection (`CockroachDB`, self-hosted
/// Postgres, or any other Postgres-flavoured server the user did not
/// specifically label).
pub const FLAVOR_POSTGRES: &str = "postgres";

/// Stable adapter identifier reported by [`DatabaseAdapter::id`] for a
/// connection the user declared as Neon (ADR-0018). Wire and SQL path
/// are identical to [`FLAVOR_POSTGRES`]; the difference is the label
/// the rest of the system sees in capability output and connection
/// picker UI.
pub const FLAVOR_NEON: &str = "neon";

/// Stable adapter identifier reported by [`DatabaseAdapter::id`] for a
/// connection the user declared as Supabase (ADR-0019). Wire and SQL
/// path are identical to [`FLAVOR_POSTGRES`]; the difference is the
/// label the rest of the system sees in capability output and
/// connection picker UI. REST surfaces (auth / storage / realtime /
/// functions) are out of scope for this flavor and will land via a
/// future ADR.
pub const FLAVOR_SUPABASE: &str = "supabase";

/// Stable adapter identifier reported by [`DatabaseAdapter::id`] for a
/// connection the user declared as AWS Aurora DSQL (ADR-0021). Wire and
/// SQL path are identical to [`FLAVOR_POSTGRES`]; the difference is the
/// label the rest of the system sees in capability output and
/// connection picker UI. The connection URL's password field is
/// expected to carry a short-lived IAM authentication token (~15 min
/// TTL) generated via `aws dsql generate-db-connect-admin-auth-token`
/// or an equivalent SDK call; automatic token refresh via the AWS SDK
/// is out of scope for this flavor and will land via a future ADR.
pub const FLAVOR_AURORA_DSQL: &str = "aurora-dsql";

pub struct PostgresAdapter {
    // Only the pool is retained; the connection URL (with its password)
    // is intentionally not stored, so it cannot leak through Debug.
    pool: sqlx::PgPool,
    // Stable label reported by `id()`. See [`FLAVOR_POSTGRES`] /
    // [`FLAVOR_NEON`]. A `'static str` because the only flavors are
    // compile-time constants in this crate.
    flavor: &'static str,
}

impl PostgresAdapter {
    /// Connect to a PostgreSQL-wire database and build a connection pool.
    ///
    /// The adapter reports [`FLAVOR_POSTGRES`] as its id. Use
    /// [`connect_neon`](Self::connect_neon) when the user has explicitly
    /// declared a Neon connection (ADR-0018).
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] when the URL is empty or the pool
    /// cannot establish a connection (bad host, TLS failure, auth
    /// rejection, timeout, ...).
    pub async fn connect(config: PostgresConfig) -> DbResult<Self> {
        Self::connect_with_flavor(config, FLAVOR_POSTGRES).await
    }

    /// Connect to a Neon database (ADR-0018). The wire protocol and SQL
    /// path are identical to [`connect`](Self::connect); the only
    /// difference is that the adapter reports [`FLAVOR_NEON`] as its id,
    /// so capabilities output and the connection picker can label the
    /// connection as Neon rather than generic Postgres.
    ///
    /// # Errors
    ///
    /// Same as [`connect`](Self::connect).
    pub async fn connect_neon(config: PostgresConfig) -> DbResult<Self> {
        Self::connect_with_flavor(config, FLAVOR_NEON).await
    }

    /// Connect to a Supabase Postgres database (ADR-0019). The wire
    /// protocol and SQL path are identical to [`connect`](Self::connect);
    /// the only difference is that the adapter reports
    /// [`FLAVOR_SUPABASE`] as its id, so capabilities output and the
    /// connection picker can label the connection as Supabase rather
    /// than generic Postgres. Both the direct connection (`:5432`) and
    /// the transaction-pooler endpoint (`:6543`) accept this entry
    /// point — the URL itself encodes the choice.
    ///
    /// # Errors
    ///
    /// Same as [`connect`](Self::connect).
    pub async fn connect_supabase(config: PostgresConfig) -> DbResult<Self> {
        Self::connect_with_flavor(config, FLAVOR_SUPABASE).await
    }

    /// Connect to an AWS Aurora DSQL database (ADR-0021). The wire
    /// protocol and SQL path are identical to [`connect`](Self::connect);
    /// the only difference is that the adapter reports
    /// [`FLAVOR_AURORA_DSQL`] as its id, so capabilities output and the
    /// connection picker can label the connection as Aurora DSQL rather
    /// than generic Postgres.
    ///
    /// Aurora DSQL only accepts short-lived IAM authentication tokens
    /// (typical TTL ~15 minutes) in place of a static password. The
    /// caller is expected to embed a freshly minted token in
    /// [`PostgresConfig::url`]; this constructor does not refresh
    /// tokens. Refresh integration via the AWS SDK is a future ADR.
    ///
    /// # Errors
    ///
    /// Same as [`connect`](Self::connect). An expired IAM token surfaces
    /// as [`DbError::Connection`] (auth rejection) from the underlying
    /// pool.
    pub async fn connect_aurora_dsql(config: PostgresConfig) -> DbResult<Self> {
        Self::connect_with_flavor(config, FLAVOR_AURORA_DSQL).await
    }

    async fn connect_with_flavor(config: PostgresConfig, flavor: &'static str) -> DbResult<Self> {
        if config.url.trim().is_empty() {
            return Err(DbError::Connection(
                "PostgreSQL connection URL is empty".to_string(),
            ));
        }

        // Parse the URL ourselves so we can harden the TLS policy before
        // connecting; a parse failure is reduced to a fixed string by
        // `classify_error` so the password cannot leak.
        let options: PgConnectOptions = config.url.parse().map_err(|e| classify_error(&e))?;
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(harden_ssl_mode(options))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(Self { pool, flavor })
    }
}

#[async_trait]
impl DatabaseAdapter for PostgresAdapter {
    fn id(&self) -> &'static str {
        self.flavor
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            has_create_statement: true,
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        sqlx::raw_sql("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| classify_error(&e))
            .map(|_| ())
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        // A failed introspection query is a schema error to the rest of
        // the system, not a user query error.
        let result = self
            .query(LIST_TABLES_SQL)
            .await
            .map_err(reclassify_schema)?;
        result
            .rows
            .iter()
            .map(|row| match (row.get(0), row.get(1)) {
                (Some(Value::Text(schema)), Some(Value::Text(name))) => {
                    Ok(tuple_to_table(schema.clone(), name.clone()))
                }
                other => Err(DbError::Schema(format!(
                    "unexpected row shape from information_schema.tables: {other:?}"
                ))),
            })
            .collect()
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        // Unqualified `TableInfo` defaults to `public` — where
        // unqualified DDL lands on both Postgres and CockroachDB.
        let schema = table.schema.as_deref().unwrap_or("public");

        // Unlike `query`, this path uses the extended protocol
        // (`sqlx::query` + binds): schema/table names come from
        // introspection data, and binding keeps them out of the SQL text.
        let column_rows = sqlx::query(DESCRIBE_COLUMNS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;
        // information_schema returns an empty set (not an error) for an
        // unknown table; surface it as a query error like the SQLite
        // adapters do.
        if column_rows.is_empty() {
            return Err(DbError::Query(format!(
                "relation \"{schema}.{}\" does not exist",
                table.name
            )));
        }

        let pk_rows = sqlx::query(DESCRIBE_PK_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;
        let primary_key = pk_rows
            .iter()
            .map(|row| row.try_get(0).map_err(|e| classify_error(&e)))
            .collect::<DbResult<Vec<String>>>()?;

        let columns = column_rows
            .iter()
            .map(|row| -> DbResult<ColumnInfo> {
                let name: String = row.try_get(0).map_err(|e| classify_error(&e))?;
                let data_type: String = row.try_get(1).map_err(|e| classify_error(&e))?;
                let is_nullable: String = row.try_get(2).map_err(|e| classify_error(&e))?;
                let default_value: Option<String> =
                    row.try_get(3).map_err(|e| classify_error(&e))?;
                let ordinal: i32 = row.try_get(4).map_err(|e| classify_error(&e))?;
                let comment: Option<String> = row.try_get(5).map_err(|e| classify_error(&e))?;
                column_from_parts(
                    name,
                    data_type,
                    &is_nullable,
                    default_value,
                    ordinal,
                    comment,
                    &primary_key,
                )
            })
            .collect::<DbResult<Vec<_>>>()?;

        Ok(TableSchema {
            table: table.clone(),
            columns,
            primary_key,
        })
    }

    async fn create_statement(&self, table: &TableInfo) -> DbResult<String> {
        let schema = table.schema.as_deref().unwrap_or("public");
        // relkind decides the DDL source: Postgres has no single
        // "CREATE TABLE text" function, but views expose their body via
        // pg_get_viewdef. An empty result means the object is absent.
        let kind_rows = sqlx::query(RELKIND_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| classify_error(&e))?;
        let Some(kind_row) = kind_rows.first() else {
            return Err(DbError::Query(format!(
                "relation \"{schema}.{}\" does not exist",
                table.name
            )));
        };
        let relkind: String = kind_row.try_get(0).map_err(|e| classify_error(&e))?;
        let qualified = pg_qualified(schema, &table.name);

        if relkind == "v" || relkind == "m" {
            let rows = sqlx::query(VIEWDEF_SQL)
                .bind(schema)
                .bind(&table.name)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| classify_error(&e))?;
            let body: String = rows
                .first()
                .ok_or_else(|| DbError::Query(format!("no view definition for {qualified}")))?
                .try_get(0)
                .map_err(|e| classify_error(&e))?;
            let keyword = if relkind == "m" {
                "CREATE MATERIALIZED VIEW"
            } else {
                "CREATE VIEW"
            };
            return Ok(format!("{keyword} {qualified} AS\n{}", body.trim_end()));
        }

        // Tables (ordinary/partitioned) and anything else column-shaped:
        // reconstruct from the column list. Best-effort — the catalogs do
        // not hand back the original CREATE TABLE verbatim, so types come
        // from information_schema (e.g. "character varying" without length).
        let described = self.describe_table(table).await?;
        Ok(render_create_table(&qualified, &described))
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        // sqlx::raw_sql uses the simple query protocol, which streams
        // row data and command-completion counts in one pass — so SELECT
        // and DML need no separate routing. Row-returning statements
        // expose rows and leave `rows_affected` at 0; pure DML leaves
        // `rows` empty and reports the affected count. Mixing both in
        // one call is not supported (`columns` would reflect the first
        // row-returning statement only).
        let mut stream = sqlx::raw_sql(sql).fetch_many(&self.pool);

        let mut columns: Option<Vec<Column>> = None;
        let mut rows: Vec<Row> = Vec::new();
        let mut affected: u64 = 0;

        while let Some(item) = stream.try_next().await.map_err(|e| classify_error(&e))? {
            match item {
                // Command-completion: carries the DML/DDL affected count.
                Either::Left(done) => affected = affected.saturating_add(done.rows_affected()),
                Either::Right(row) => {
                    if columns.is_none() {
                        columns = Some(columns_of(&row));
                    }
                    // Refuse to load past the workspace-wide cap before
                    // decoding the next row's cells (see
                    // dbboard-core::limits).
                    if rows.len() >= MAX_RESULT_ROWS {
                        return Err(too_many_rows_error());
                    }
                    rows.push(Row::new(row_to_values(&row)?));
                }
            }
        }

        // Row-returning statements report rows; only report an affected
        // count when no rows came back (i.e. a pure DML/DDL statement).
        let rows_affected = if rows.is_empty() { affected } else { 0 };
        Ok(QueryResult {
            columns: columns.unwrap_or_default(),
            rows,
            rows_affected,
        })
    }
}

/// Build the column list from a row, recording the Postgres type name
/// (e.g. `INT8`, `TEXT`, `TIMESTAMPTZ`) as the declared type.
fn columns_of(row: &PgRow) -> Vec<Column> {
    row.columns()
        .iter()
        .map(|col| Column {
            name: col.name().to_string(),
            declared_type: Some(col.type_info().name().to_string()),
        })
        .collect()
}

/// Decode every cell of a row into a domain [`Value`].
fn row_to_values(row: &PgRow) -> DbResult<Vec<Value>> {
    let count = row.len();
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let raw = row.try_get_raw(i).map_err(|e| classify_error(&e))?;
        values.push(decode_cell(raw)?);
    }
    Ok(values)
}

/// Decode a single cell. Under the simple query protocol every value
/// arrives in text format, so reading it as a string yields the same
/// representation PostgreSQL itself prints. NULL maps to [`Value::Null`].
fn decode_cell(raw: PgValueRef<'_>) -> DbResult<Value> {
    if raw.is_null() {
        return Ok(Value::Null);
    }
    // Invariant: the simple query protocol delivers every value in text
    // format. Assert in debug builds so a future regression (e.g. a path
    // that switches to the extended/binary protocol) fails loudly here
    // instead of silently mis-decoding binary bytes as a UTF-8 string.
    debug_assert_eq!(
        raw.format(),
        sqlx::postgres::PgValueFormat::Text,
        "expected text-format value under the simple query protocol"
    );
    // Decode (not `try_get`) so the column's declared Postgres type does
    // not gate reading it as text — the value is already text-format.
    let text = <String as sqlx::Decode<sqlx::Postgres>>::decode(raw)
        .map_err(|e| DbError::TypeConversion(truncate(&e.to_string())))?;
    Ok(Value::Text(text))
}

fn tuple_to_table(schema: String, name: String) -> TableInfo {
    TableInfo::qualified(schema, name)
}

/// Assemble a [`ColumnInfo`] from one `information_schema.columns` row.
///
/// `is_nullable` is the SQL-standard `"YES"`/`"NO"` string, compared
/// case-insensitively. `ordinal` must be positive —
/// `information_schema` guarantees a 1-based `ordinal_position`, so
/// anything else means a broken catalog and is rejected instead of
/// silently cast (ADR-0028 Decision 3).
fn column_from_parts(
    name: String,
    data_type: String,
    is_nullable: &str,
    default_value: Option<String>,
    ordinal: i32,
    comment: Option<String>,
    primary_key: &[String],
) -> DbResult<ColumnInfo> {
    let ordinal = u32::try_from(ordinal)
        .ok()
        .filter(|o| *o > 0)
        .ok_or_else(|| {
            DbError::TypeConversion(format!(
                "non-positive ordinal_position {ordinal} for column {name}"
            ))
        })?;
    let in_primary_key = primary_key.iter().any(|k| k == &name);
    Ok(ColumnInfo {
        name,
        declared_type: Some(data_type),
        nullable: is_nullable.eq_ignore_ascii_case("YES"),
        primary_key: in_primary_key,
        ordinal,
        default_value,
        comment,
    })
}

/// Wrap a single SQL identifier in double quotes, doubling any embedded
/// quote (ADR-0038 slice b).
fn pg_quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// A schema-qualified, double-quoted relation reference —
/// `"public"."users"` (ADR-0038 slice b).
fn pg_qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", pg_quote_ident(schema), pg_quote_ident(name))
}

/// Reconstruct a `CREATE TABLE` statement from a table's columns and
/// primary key (ADR-0038 slice b). Best-effort: types come from
/// `information_schema.data_type`, which drops length/precision
/// modifiers, and non-primary-key constraints (foreign keys, uniques,
/// checks) are not reproduced. `qualified` is the pre-quoted relation
/// name from [`pg_qualified`].
fn render_create_table(qualified: &str, schema: &TableSchema) -> String {
    let mut lines: Vec<String> = Vec::new();
    for col in &schema.columns {
        let mut parts = vec![pg_quote_ident(&col.name)];
        if let Some(ty) = &col.declared_type {
            parts.push(ty.clone());
        }
        if !col.nullable {
            parts.push("NOT NULL".to_string());
        }
        if let Some(default) = &col.default_value {
            parts.push(format!("DEFAULT {default}"));
        }
        lines.push(format!("    {}", parts.join(" ")));
    }
    if !schema.primary_key.is_empty() {
        let cols = schema
            .primary_key
            .iter()
            .map(|c| pg_quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("    PRIMARY KEY ({cols})"));
    }
    format!("CREATE TABLE {qualified} (\n{}\n);", lines.join(",\n"))
}

/// Harden the connection's TLS policy.
///
/// sqlx defaults an unspecified `sslmode` to [`PgSslMode::Prefer`], which
/// silently falls back to a plaintext connection when the server does not
/// offer TLS — sending the password in the clear with no error. Upgrade
/// that default to [`PgSslMode::Require`]. Any explicit choice (including
/// `sslmode=disable` for a deliberately insecure local node) is preserved.
fn harden_ssl_mode(options: PgConnectOptions) -> PgConnectOptions {
    if matches!(options.get_ssl_mode(), PgSslMode::Prefer) {
        options.ssl_mode(PgSslMode::Require)
    } else {
        options
    }
}

/// Classify a sqlx error into a domain [`DbError`].
///
/// Server-reported SQL errors are [`DbError::Query`]; transport, TLS, and
/// pool failures are [`DbError::Connection`]; decode/type problems are
/// [`DbError::TypeConversion`]. The connection URL is never part of any
/// message — in particular [`sqlx::Error::Configuration`] (which can wrap
/// the URL while parsing it) is reduced to a fixed string so the password
/// cannot leak.
fn classify_error(err: &sqlx::Error) -> DbError {
    match err {
        // Server-side SQL failure. The database message is safe to show
        // (it never contains the connection password).
        sqlx::Error::Database(db) => DbError::Query(truncate(db.message())),

        // URL parsing/configuration: the source may embed the URL with
        // its password, so do not surface it.
        sqlx::Error::Configuration(_) => {
            DbError::Connection("invalid PostgreSQL connection configuration".to_string())
        }

        // Transport / availability failures.
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => DbError::Connection(truncate(&err.to_string())),

        // Decoding / type resolution problems.
        sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::TypeNotFound { .. } => DbError::TypeConversion(truncate(&err.to_string())),

        // `sqlx::Error` is `#[non_exhaustive]`; treat anything else as a
        // query-level failure with a bounded message.
        other => DbError::Query(truncate(&other.to_string())),
    }
}

/// Re-tag a `Query`/`TypeConversion` failure raised during introspection
/// as a `Schema` error, leaving connection failures intact.
fn reclassify_schema(err: DbError) -> DbError {
    match err {
        DbError::Query(msg) | DbError::TypeConversion(msg) => DbError::Schema(msg),
        other => other,
    }
}

/// Truncate `text` to [`MAX_ERROR_DETAIL`] bytes on a char boundary,
/// appending an ellipsis when shortened.
fn truncate(text: &str) -> String {
    if text.len() <= MAX_ERROR_DETAIL {
        return text.to_string();
    }
    let mut end = MAX_ERROR_DETAIL;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::{
        classify_error, column_from_parts, harden_ssl_mode, pg_qualified, reclassify_schema,
        render_create_table, truncate, tuple_to_table, FLAVOR_AURORA_DSQL, FLAVOR_NEON,
        FLAVOR_POSTGRES, FLAVOR_SUPABASE,
    };
    use dbboard_core::{ColumnInfo, DatabaseAdapter, DbError, TableInfo, TableSchema};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};

    /// `id()` is part of the public contract documented in
    /// `docs/architecture.md` (adapter identifiers `turso`, `neon`,
    /// `supabase`, `aurora-dsql` are stable strings). Every flavor
    /// constant must keep its byte-content stable across releases —
    /// capability consumers match on them — and must be different from
    /// each other.
    #[test]
    fn flavor_constants_are_stable_and_distinct() {
        assert_eq!(FLAVOR_POSTGRES, "postgres");
        assert_eq!(FLAVOR_NEON, "neon");
        assert_eq!(FLAVOR_SUPABASE, "supabase");
        assert_eq!(FLAVOR_AURORA_DSQL, "aurora-dsql");
        let all = [
            FLAVOR_POSTGRES,
            FLAVOR_NEON,
            FLAVOR_SUPABASE,
            FLAVOR_AURORA_DSQL,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b, "flavors {a} and {b} must be distinct");
            }
        }
    }

    #[test]
    fn unspecified_ssl_mode_is_upgraded_to_require() {
        // A bare `PgConnectOptions` defaults to `Prefer`, the silent
        // plaintext-fallback mode we refuse to ship. (`PgSslMode` is not
        // `PartialEq`, so assert with `matches!`.)
        let opts = PgConnectOptions::new();
        assert!(matches!(opts.get_ssl_mode(), PgSslMode::Prefer));
        assert!(matches!(
            harden_ssl_mode(opts).get_ssl_mode(),
            PgSslMode::Require
        ));
    }

    #[test]
    fn explicit_ssl_mode_is_preserved() {
        // An explicit `disable` (deliberately insecure local node) and an
        // explicit `verify-full` both pass through untouched.
        let disabled = PgConnectOptions::new().ssl_mode(PgSslMode::Disable);
        assert!(matches!(
            harden_ssl_mode(disabled).get_ssl_mode(),
            PgSslMode::Disable
        ));
        let verified = PgConnectOptions::new().ssl_mode(PgSslMode::VerifyFull);
        assert!(matches!(
            harden_ssl_mode(verified).get_ssl_mode(),
            PgSslMode::VerifyFull
        ));
    }

    /// `connect_lazy_with` builds a pool without any network I/O, which
    /// is enough to read the adapter's static capability flags. It still
    /// needs a Tokio context to spawn the pool's background worker,
    /// hence `#[tokio::test]`.
    #[tokio::test]
    async fn capabilities_advertise_describe_table() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        let adapter = super::PostgresAdapter {
            pool,
            flavor: FLAVOR_POSTGRES,
        };
        assert!(adapter.capabilities().has_describe_table);
        assert!(adapter.capabilities().has_create_statement);
    }

    fn col(name: &str, ty: &str, nullable: bool, default: Option<&str>) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            declared_type: Some(ty.into()),
            nullable,
            primary_key: false,
            ordinal: 1,
            default_value: default.map(Into::into),
            comment: None,
        }
    }

    #[test]
    fn pg_qualified_double_quotes_schema_and_name() {
        assert_eq!(pg_qualified("public", "users"), "\"public\".\"users\"");
        // Embedded quotes are doubled so a hostile name can't break out.
        assert_eq!(
            pg_qualified("public", "we\"ird"),
            "\"public\".\"we\"\"ird\""
        );
    }

    #[test]
    fn render_create_table_emits_columns_defaults_and_primary_key() {
        let schema = TableSchema {
            table: TableInfo::qualified("public", "users"),
            columns: vec![
                col(
                    "id",
                    "integer",
                    false,
                    Some("nextval('users_id_seq'::regclass)"),
                ),
                col("email", "text", false, None),
                col("note", "text", true, None),
            ],
            primary_key: vec!["id".into()],
        };
        let ddl = render_create_table("\"public\".\"users\"", &schema);
        assert_eq!(
            ddl,
            "CREATE TABLE \"public\".\"users\" (\n    \
             \"id\" integer NOT NULL DEFAULT nextval('users_id_seq'::regclass),\n    \
             \"email\" text NOT NULL,\n    \
             \"note\" text,\n    \
             PRIMARY KEY (\"id\")\n);"
        );
    }

    #[test]
    fn render_create_table_without_primary_key_omits_the_clause() {
        let schema = TableSchema {
            table: TableInfo::unqualified("logs"),
            columns: vec![col("msg", "text", true, None)],
            primary_key: vec![],
        };
        let ddl = render_create_table("\"public\".\"logs\"", &schema);
        assert_eq!(
            ddl,
            "CREATE TABLE \"public\".\"logs\" (\n    \"msg\" text\n);"
        );
    }

    #[test]
    fn column_from_parts_parses_nullability_and_pk_membership() {
        let pk = vec!["id".to_owned()];
        let id = column_from_parts(
            "id".into(),
            "integer".into(),
            "NO",
            Some("nextval('users_id_seq'::regclass)".into()),
            1,
            Some("surrogate key".into()),
            &pk,
        )
        .expect("id column");
        assert!(!id.nullable);
        assert!(id.primary_key);
        assert_eq!(id.ordinal, 1);
        assert_eq!(
            id.default_value.as_deref(),
            Some("nextval('users_id_seq'::regclass)")
        );
        assert_eq!(id.comment.as_deref(), Some("surrogate key"));

        // A column with no comment carries `None` (ADR-0037).
        let note = column_from_parts("note".into(), "text".into(), "YES", None, 2, None, &pk)
            .expect("note column");
        assert!(note.nullable);
        assert!(!note.primary_key);
        assert_eq!(note.declared_type.as_deref(), Some("text"));
        assert_eq!(note.comment, None);
    }

    #[test]
    fn column_from_parts_rejects_a_non_positive_ordinal() {
        // information_schema.ordinal_position is 1-based; anything else
        // indicates a broken catalog and must not be silently cast.
        let err = column_from_parts("x".into(), "text".into(), "YES", None, 0, None, &[])
            .expect_err("ordinal 0 should fail");
        assert!(matches!(err, DbError::TypeConversion(_)));
        let err = column_from_parts("x".into(), "text".into(), "YES", None, -1, None, &[])
            .expect_err("negative ordinal should fail");
        assert!(matches!(err, DbError::TypeConversion(_)));
    }

    #[test]
    fn tuple_to_table_is_schema_qualified() {
        let table = tuple_to_table("public".to_string(), "users".to_string());
        assert_eq!(table.schema.as_deref(), Some("public"));
        assert_eq!(table.name, "users");
    }

    #[test]
    fn configuration_error_hides_the_url() {
        // Simulate a URL-parse failure whose source carries the secret.
        let secret = "postgresql://admin:s3cr3t@host/db";
        let err = sqlx::Error::Configuration(secret.into());
        match classify_error(&err) {
            DbError::Connection(msg) => {
                assert!(!msg.contains("s3cr3t"), "password leaked into: {msg}");
                assert!(!msg.contains("admin"), "username leaked into: {msg}");
            }
            other => panic!("expected Connection, got {other:?}"),
        }
    }

    #[test]
    fn pool_timeout_is_a_connection_error() {
        assert!(matches!(
            classify_error(&sqlx::Error::PoolTimedOut),
            DbError::Connection(_)
        ));
        assert!(matches!(
            classify_error(&sqlx::Error::PoolClosed),
            DbError::Connection(_)
        ));
    }

    #[test]
    fn protocol_error_is_a_connection_error() {
        assert!(matches!(
            classify_error(&sqlx::Error::Protocol("bad message".to_string())),
            DbError::Connection(_)
        ));
    }

    #[test]
    fn type_not_found_is_a_type_conversion_error() {
        let err = sqlx::Error::TypeNotFound {
            type_name: "weird".to_string(),
        };
        assert!(matches!(classify_error(&err), DbError::TypeConversion(_)));
    }

    #[test]
    fn reclassify_retags_query_but_passes_connection_through() {
        assert!(matches!(
            reclassify_schema(DbError::Query("boom".into())),
            DbError::Schema(_)
        ));
        assert!(matches!(
            reclassify_schema(DbError::TypeConversion("bad".into())),
            DbError::Schema(_)
        ));
        assert!(matches!(
            reclassify_schema(DbError::Connection("down".into())),
            DbError::Connection(_)
        ));
    }

    #[test]
    fn truncate_caps_long_text_on_a_char_boundary() {
        let long = "x".repeat(super::MAX_ERROR_DETAIL + 100);
        let out = truncate(&long);
        assert!(out.len() <= super::MAX_ERROR_DETAIL + 4); // + ellipsis bytes
        assert!(out.ends_with('…'));
        // Short text is returned unchanged.
        assert_eq!(truncate("short"), "short");
    }
}
