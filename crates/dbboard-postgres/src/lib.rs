//! PostgreSQL-wire adapter for dbboard.
//!
//! `CockroachDB` speaks the PostgreSQL wire protocol, so a desktop client
//! reaches it with an ordinary `postgresql://…` connection string and a
//! Postgres driver. This adapter uses `sqlx` over a `PgPool` and
//! implements the workspace-wide [`DatabaseAdapter`] contract
//! (ADR-0012); Phase 2 advertises no optional capabilities.
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
    too_many_rows_error, Capabilities, Column, DatabaseAdapter, DbError, DbResult, QueryResult,
    Row, TableInfo, Value, MAX_RESULT_ROWS,
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
        Capabilities::default()
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
        classify_error, harden_ssl_mode, reclassify_schema, truncate, tuple_to_table, FLAVOR_NEON,
        FLAVOR_POSTGRES, FLAVOR_SUPABASE,
    };
    use dbboard_core::DbError;
    use sqlx::postgres::{PgConnectOptions, PgSslMode};

    /// `id()` is part of the public contract documented in
    /// `docs/architecture.md` (adapter identifiers `turso`, `neon`,
    /// `supabase` are stable strings). Every flavor constant must keep
    /// its byte-content stable across releases — capability consumers
    /// match on them — and must be different from each other.
    #[test]
    fn flavor_constants_are_stable_and_distinct() {
        assert_eq!(FLAVOR_POSTGRES, "postgres");
        assert_eq!(FLAVOR_NEON, "neon");
        assert_eq!(FLAVOR_SUPABASE, "supabase");
        assert_ne!(FLAVOR_POSTGRES, FLAVOR_NEON);
        assert_ne!(FLAVOR_POSTGRES, FLAVOR_SUPABASE);
        assert_ne!(FLAVOR_NEON, FLAVOR_SUPABASE);
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
