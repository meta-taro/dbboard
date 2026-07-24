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

use std::sync::{Arc, PoisonError, RwLock, Weak};

use async_trait::async_trait;
use dbboard_core::{
    classify_read_only, too_many_rows_error, Capabilities, Column, ColumnInfo, DatabaseAdapter,
    DbError, DbResult, ForeignKey, QueryResult, ReadOnlyStatement, Row, SqlDialect, TableInfo,
    TableSchema, Value, MAX_RESULT_ROWS,
};
use futures_util::TryStreamExt;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgRow, PgSslMode, PgValueRef};
use sqlx::{Column as _, Either, Row as _, TypeInfo as _, ValueRef as _};

mod dsql_auth;
mod table_ddl;

use table_ddl::{assemble_table_ddl, ColumnDef, ConstraintDef, SequenceDef, TableDdlParts};

/// Where an adapter gets the `PgPool` to run the next statement.
///
/// Every flavor except `aurora-dsql-iam` holds a plain [`PgPool`] for the
/// process lifetime ([`PoolHandle::Static`]). The IAM flavor's token
/// expires (~15 min), and sqlx 0.8 has no per-connection password callback,
/// so its pool is periodically rebuilt with a fresh token and swapped
/// behind an `RwLock` by a background task ([`PoolHandle::Refreshing`],
/// ADR-0037 段階B).
enum PoolHandle {
    Static(PgPool),
    Refreshing(Arc<RwLock<PgPool>>),
}

impl PoolHandle {
    /// The pool to use for the next statement. Cheap — [`PgPool`] is an
    /// `Arc` internally, so this clones a handle, not the connections. For
    /// the refreshing variant the read lock is held only long enough to
    /// clone that handle and is released before the caller `.await`s, so a
    /// mid-flight token swap never blocks a running query. A poisoned lock
    /// (a panic in the refresh task) is recovered rather than propagated,
    /// so a background hiccup cannot wedge the whole adapter.
    fn current(&self) -> PgPool {
        match self {
            PoolHandle::Static(pool) => pool.clone(),
            PoolHandle::Refreshing(lock) => {
                lock.read().unwrap_or_else(PoisonError::into_inner).clone()
            }
        }
    }
}

/// Small pool: a desktop client issues one statement at a time, so a
/// handful of connections is plenty and keeps server-side resource use
/// (and `CockroachDB` Cloud connection limits) modest.
const MAX_CONNECTIONS: u32 = 5;

/// `statement_timeout` applied inside the read-only transaction
/// ([`PostgresAdapter::query_read_only`], ADR-0046 §8). It is the real
/// cancellation backstop: an MCP client that drops a tool future only
/// cancels the Rust side at an await point, so the server-side timeout is
/// what stops an abandoned query from pinning a pooled connection.
const READ_ONLY_STATEMENT_TIMEOUT_SECS: u32 = 30;

/// Name of the server-side cursor used to row-cap a read-only query. A
/// fixed identifier is safe because each cursor lives and dies inside one
/// short-lived transaction on its own pooled connection.
const READ_ONLY_CURSOR: &str = "dbboard_ro_cursor";

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
     is_nullable::TEXT, column_default::TEXT, ordinal_position::INT4 \
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

/// Foreign keys of one table, one row per key column in key order
/// (ADR-0054). Read from `pg_catalog` so composite keys keep their column
/// order: `con.conkey`/`con.confkey` are parallel `smallint[]` arrays of
/// local/referenced attribute numbers, unnested `WITH ORDINALITY` and
/// re-joined on the shared position so local and referenced columns stay
/// aligned. `contype = 'f'` selects foreign keys; the referenced table is
/// resolved from `con.confrelid`. Grouped by constraint in the assembler.
/// Names are cast to `TEXT` and the ordinal to `INT4` for the same
/// cross-flavor decode reasons as [`DESCRIBE_COLUMNS_SQL`].
const FOREIGN_KEYS_SQL: &str = "SELECT con.conname::TEXT, \
     latt.attname::TEXT, \
     fn.nspname::TEXT, \
     fc.relname::TEXT, \
     fatt.attname::TEXT \
     FROM pg_catalog.pg_constraint con \
     JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     JOIN pg_catalog.pg_class fc ON fc.oid = con.confrelid \
     JOIN pg_catalog.pg_namespace fn ON fn.oid = fc.relnamespace \
     JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS lk(attnum, ord) ON TRUE \
     JOIN LATERAL unnest(con.confkey) WITH ORDINALITY AS rk(attnum, ord) ON rk.ord = lk.ord \
     JOIN pg_catalog.pg_attribute latt ON latt.attrelid = con.conrelid AND latt.attnum = lk.attnum \
     JOIN pg_catalog.pg_attribute fatt ON fatt.attrelid = con.confrelid AND fatt.attnum = rk.attnum \
     WHERE con.contype = 'f' AND n.nspname = $1 AND c.relname = $2 \
     ORDER BY con.conname, lk.ord";

/// Columns of one table for DDL reconstruction (ADR-0049), read from
/// `pg_catalog` so the type comes back canonicalised by `format_type` and
/// the default verbatim from `pg_get_expr` — richer than the
/// `information_schema` view `describe_table` uses.
const DDL_COLUMNS_SQL: &str = "SELECT a.attname::TEXT, \
     format_type(a.atttypid, a.atttypmod)::TEXT, \
     a.attnotnull, \
     pg_get_expr(ad.adbin, ad.adrelid)::TEXT \
     FROM pg_catalog.pg_attribute a \
     JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
     WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
     ORDER BY a.attnum";

/// All constraints of one table, with names preserved and the body from
/// `pg_get_constraintdef`. Ordered primary-key-first for conventional
/// output; constraint order does not affect the DDL's validity.
const DDL_CONSTRAINTS_SQL: &str = "SELECT con.conname::TEXT, pg_get_constraintdef(con.oid)::TEXT \
     FROM pg_catalog.pg_constraint con \
     JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 \
     ORDER BY (con.contype <> 'p'), con.conname";

/// Standalone indexes of one table — those *not* backing a constraint,
/// which are recreated implicitly by their constraint. Body verbatim from
/// `pg_get_indexdef`.
const DDL_INDEXES_SQL: &str = "SELECT pg_get_indexdef(idx.indexrelid)::TEXT \
     FROM pg_catalog.pg_index idx \
     JOIN pg_catalog.pg_class ic ON ic.oid = idx.indexrelid \
     JOIN pg_catalog.pg_class tc ON tc.oid = idx.indrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = tc.relnamespace \
     WHERE n.nspname = $1 AND tc.relname = $2 \
       AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint con \
                       WHERE con.conindid = idx.indexrelid) \
     ORDER BY ic.relname";

/// Sequences owned by a column of one table (a `SERIAL`/`GENERATED`
/// column's backing sequence). Emitted ahead of the table. Aurora DSQL has
/// no sequences, so this query is skipped there (ADR-0021).
const DDL_SEQUENCES_SQL: &str = "SELECT sn.nspname::TEXT, s.relname::TEXT, \
     format_type(seq.seqtypid, NULL)::TEXT, \
     seq.seqstart, seq.seqincrement, seq.seqmin, seq.seqmax, seq.seqcache, seq.seqcycle \
     FROM pg_catalog.pg_class s \
     JOIN pg_catalog.pg_namespace sn ON sn.oid = s.relnamespace \
     JOIN pg_catalog.pg_sequence seq ON seq.seqrelid = s.oid \
     JOIN pg_catalog.pg_depend d ON d.objid = s.oid \
       AND d.classid = 'pg_class'::regclass AND d.deptype = 'a' \
     JOIN pg_catalog.pg_class t ON t.oid = d.refobjid \
     JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace \
     WHERE s.relkind = 'S' AND tn.nspname = $1 AND t.relname = $2 \
     ORDER BY s.relname";

/// Connection parameters for a PostgreSQL-wire database.
///
/// `url` is a secret: it embeds the password and is never logged, never
/// echoed in a [`DbError`], and never derived into `Debug`.
pub struct PostgresConfig {
    pub url: String,
}

/// Connection parameters for an Aurora DSQL IAM connection (ADR-0036).
///
/// Unlike [`PostgresConfig`], no token is supplied: it is minted from the
/// AWS credentials at connect time. `secret_key` is a secret and is never
/// logged, so this struct deliberately does not derive `Debug`.
/// `endpoint` is the bare cluster host (no scheme, no port); `username`
/// is typically `admin`.
pub struct AuroraDsqlIamParams {
    pub endpoint: String,
    pub region: String,
    pub database: String,
    pub username: String,
    pub access_key_id: String,
    pub secret_key: String,
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
    // is intentionally not stored, so it cannot leak through Debug. For
    // `aurora-dsql-iam` this is a refreshing handle whose token is re-minted
    // in the background (ADR-0037); every other flavor holds a static pool.
    pool: PoolHandle,
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

    /// Connect to an AWS Aurora DSQL database using IAM authentication
    /// (ADR-0036). Unlike [`connect_aurora_dsql`](Self::connect_aurora_dsql),
    /// which expects a pre-generated token embedded in the URL, this path
    /// **mints a fresh IAM token at connect time** from the supplied AWS
    /// access-key / secret-key pair, so the connection never carries a
    /// stale token. Admin (`admin`) usernames get a `DbConnectAdmin`
    /// token; any other role gets `DbConnect`.
    ///
    /// The token is short-lived (~15 min). To keep an unattended 24/7
    /// connection alive (段階B, ADR-0037), a background task re-mints the
    /// token and swaps in a freshly authenticated pool every
    /// [`dsql_auth::refresh_interval`] — so a new physical connection is
    /// always dialled with a current token and Aurora DSQL's idle-recycle
    /// `access denied` cannot occur. The task stops when this adapter is
    /// dropped (process exit or a connection switch), because it only holds
    /// a [`Weak`] handle to the shared pool.
    ///
    /// # Errors
    ///
    /// Same as [`connect`](Self::connect). A clock skew or wrong
    /// credentials surface as [`DbError::Connection`] (auth rejection)
    /// from the underlying pool. A later refresh failure is non-fatal: the
    /// current pool is kept and the next tick retries.
    pub async fn connect_aurora_dsql_iam(params: AuroraDsqlIamParams) -> DbResult<Self> {
        // The `admin` role authenticates against the admin action; every
        // other role uses the plain connect action.
        let is_admin = params.username == "admin";
        let pool = build_dsql_pool(&params, is_admin).await?;

        // Share the pool behind an RwLock so the refresh task can swap in a
        // freshly authenticated one without disturbing in-flight queries.
        let shared = Arc::new(RwLock::new(pool));
        spawn_token_refresh(Arc::downgrade(&shared), params, is_admin);
        Ok(Self {
            pool: PoolHandle::Refreshing(shared),
            flavor: FLAVOR_AURORA_DSQL,
        })
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
        Self::connect_options(options, flavor).await
    }

    /// Build the pool from already-parsed options, hardening the TLS
    /// policy first. Shared by the URL-based connect paths (the IAM path
    /// builds its pool through [`build_dsql_pool`] instead so it can be
    /// re-run by the refresh task).
    async fn connect_options(options: PgConnectOptions, flavor: &'static str) -> DbResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(harden_ssl_mode(options))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(Self {
            pool: PoolHandle::Static(pool),
            flavor,
        })
    }
}

/// Mint a fresh Aurora DSQL IAM token from `params` and open a pool
/// authenticated with it. Re-run by [`spawn_token_refresh`] on every
/// refresh tick, so it must depend on nothing but its arguments and the
/// clock.
///
/// The options are built programmatically rather than via a URL string:
/// the token is itself percent-encoded, and round-tripping it through URL
/// parsing would double-decode `%2F` back to `/` and corrupt the
/// signature. `.password()` takes the token verbatim. DSQL mandates TLS;
/// `Require` encrypts without pinning the cert, matching the rest of the
/// Postgres-family adapters.
async fn build_dsql_pool(params: &AuroraDsqlIamParams, is_admin: bool) -> DbResult<PgPool> {
    let token = dsql_auth::generate_dsql_token(&dsql_auth::DsqlTokenParams {
        endpoint: &params.endpoint,
        region: &params.region,
        access_key_id: &params.access_key_id,
        secret_key: &params.secret_key,
        is_admin,
        expires_secs: dsql_auth::DEFAULT_EXPIRES_SECS,
    });
    let options = PgConnectOptions::new()
        .host(&params.endpoint)
        .port(5432)
        .database(&params.database)
        .username(&params.username)
        .password(&token)
        .ssl_mode(PgSslMode::Require);
    PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await
        .map_err(|e| classify_error(&e))
}

/// Spawn the background token-refresh loop for an `aurora-dsql-iam` pool
/// (ADR-0037 段階B).
///
/// The task owns `params` (so it can re-sign forever) and holds only a
/// [`Weak`] to the shared pool: once the adapter is dropped, the last
/// strong `Arc` goes and the next `upgrade()` returns `None`, so the loop
/// exits on its own — no shutdown channel, no task that outlives a
/// connection switch. `params` carries the AWS secret key, so it lives in
/// memory for the adapter's whole lifetime here; it is never logged and
/// never in a `Debug`, matching the 段階A posture.
fn spawn_token_refresh(pool: Weak<RwLock<PgPool>>, params: AuroraDsqlIamParams, is_admin: bool) {
    let interval = dsql_auth::refresh_interval(dsql_auth::DEFAULT_EXPIRES_SECS);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            // Adapter gone while we slept → nothing left to refresh.
            if pool.upgrade().is_none() {
                return;
            }

            // A transient mint/connect failure is non-fatal: keep the
            // current pool (its token may still be valid) and retry next
            // tick rather than tearing down a working connection.
            let Ok(fresh) = build_dsql_pool(&params, is_admin).await else {
                continue;
            };

            // Re-check liveness after the (awaiting) build, then swap. The
            // write lock is held only for the pointer swap, never across an
            // await.
            let Some(shared) = pool.upgrade() else {
                return;
            };
            let old = {
                let mut guard = shared.write().unwrap_or_else(PoisonError::into_inner);
                std::mem::replace(&mut *guard, fresh)
            };
            drop(shared);

            // Drain the retired pool off the refresh path: `close()` waits
            // for in-flight connections to return, which must not hold up
            // the next refresh tick.
            tokio::spawn(async move { old.close().await });
        }
    });
}

#[async_trait]
impl DatabaseAdapter for PostgresAdapter {
    fn id(&self) -> &'static str {
        self.flavor
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            has_table_ddl: true,
            has_execute: true,
            // Foreign keys are read from `pg_catalog` (ADR-0054). Aurora DSQL
            // does not support foreign-key constraints, so the query simply
            // returns no rows there — the capability stays advertised because
            // the introspection path itself works on every flavor.
            has_foreign_keys: true,
            // Aurora DSQL rejects mixed DDL+DML in one transaction and caps a
            // transaction at a single DDL statement (ADR-0021), so it cannot
            // honour an atomic multi-statement restore — it falls back to
            // per-statement, best-effort execution (ADR-0051). Every other
            // Postgres flavor has ordinary multi-statement transactions.
            has_atomic_restore: self.flavor != FLAVOR_AURORA_DSQL,
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        let pool = self.pool.current();
        sqlx::raw_sql("SELECT 1")
            .execute(&pool)
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

        let pool = self.pool.current();

        // Unlike `query`, this path uses the extended protocol
        // (`sqlx::query` + binds): schema/table names come from
        // introspection data, and binding keeps them out of the SQL text.
        let column_rows = sqlx::query(DESCRIBE_COLUMNS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&pool)
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
            .fetch_all(&pool)
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
                column_from_parts(
                    name,
                    data_type,
                    &is_nullable,
                    default_value,
                    ordinal,
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

    async fn foreign_keys(&self, table: &TableInfo) -> DbResult<Vec<ForeignKey>> {
        // Unqualified `TableInfo` defaults to `public`, mirroring
        // `describe_table`. Unlike SQLite, a table without foreign keys is
        // simply an empty result — not an error — so no missing-table check
        // is needed here; the caller already holds the table from
        // `list_tables`.
        let schema = table.schema.as_deref().unwrap_or("public");
        let pool = self.pool.current();

        // Extended protocol with binds: the schema/table names are
        // introspection data and stay out of the SQL text.
        let rows = sqlx::query(FOREIGN_KEYS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&pool)
            .await
            .map_err(|e| classify_error(&e))?;

        let fk_rows = rows
            .iter()
            .map(|row| -> DbResult<FkRow> {
                Ok(FkRow {
                    constraint_name: row.try_get(0).map_err(|e| classify_error(&e))?,
                    local_column: row.try_get(1).map_err(|e| classify_error(&e))?,
                    referenced_schema: row.try_get(2).map_err(|e| classify_error(&e))?,
                    referenced_table: row.try_get(3).map_err(|e| classify_error(&e))?,
                    referenced_column: row.try_get(4).map_err(|e| classify_error(&e))?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        Ok(assemble_foreign_keys(fk_rows))
    }

    async fn table_ddl(&self, table: &TableInfo) -> DbResult<String> {
        // Unqualified `TableInfo` defaults to `public`, matching
        // `describe_table` and where unqualified DDL lands.
        let schema = table.schema.as_deref().unwrap_or("public");
        let pool = self.pool.current();

        // Columns first: an empty set means the table does not exist
        // (pg_catalog returns no rows rather than erroring), surfaced the
        // same way `describe_table` reports a missing relation.
        let column_rows = sqlx::query(DDL_COLUMNS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&pool)
            .await
            .map_err(|e| classify_error(&e))?;
        if column_rows.is_empty() {
            return Err(DbError::Query(format!(
                "relation \"{schema}.{}\" does not exist",
                table.name
            )));
        }
        let columns = column_rows
            .iter()
            .map(|row| -> DbResult<ColumnDef> {
                Ok(ColumnDef {
                    name: row.try_get(0).map_err(|e| classify_error(&e))?,
                    type_name: row.try_get(1).map_err(|e| classify_error(&e))?,
                    not_null: row.try_get(2).map_err(|e| classify_error(&e))?,
                    default_expr: row.try_get(3).map_err(|e| classify_error(&e))?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        let constraint_rows = sqlx::query(DDL_CONSTRAINTS_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&pool)
            .await
            .map_err(|e| classify_error(&e))?;
        let constraints = constraint_rows
            .iter()
            .map(|row| -> DbResult<ConstraintDef> {
                Ok(ConstraintDef {
                    name: row.try_get(0).map_err(|e| classify_error(&e))?,
                    def: row.try_get(1).map_err(|e| classify_error(&e))?,
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        let index_rows = sqlx::query(DDL_INDEXES_SQL)
            .bind(schema)
            .bind(&table.name)
            .fetch_all(&pool)
            .await
            .map_err(|e| classify_error(&e))?;
        let indexes = index_rows
            .iter()
            .map(|row| row.try_get::<String, _>(0).map_err(|e| classify_error(&e)))
            .collect::<DbResult<Vec<_>>>()?;

        // Aurora DSQL has no sequences (ADR-0021): skip the query rather
        // than depend on it returning empty against a catalog that may not
        // model `pg_sequence`. Other flavors run it.
        let sequences = if self.flavor == FLAVOR_AURORA_DSQL {
            Vec::new()
        } else {
            let seq_rows = sqlx::query(DDL_SEQUENCES_SQL)
                .bind(schema)
                .bind(&table.name)
                .fetch_all(&pool)
                .await
                .map_err(|e| classify_error(&e))?;
            seq_rows
                .iter()
                .map(|row| -> DbResult<SequenceDef> {
                    Ok(SequenceDef {
                        schema: row.try_get(0).map_err(|e| classify_error(&e))?,
                        name: row.try_get(1).map_err(|e| classify_error(&e))?,
                        type_name: row.try_get(2).map_err(|e| classify_error(&e))?,
                        start: row.try_get(3).map_err(|e| classify_error(&e))?,
                        increment: row.try_get(4).map_err(|e| classify_error(&e))?,
                        min_value: row.try_get(5).map_err(|e| classify_error(&e))?,
                        max_value: row.try_get(6).map_err(|e| classify_error(&e))?,
                        cache: row.try_get(7).map_err(|e| classify_error(&e))?,
                        cycle: row.try_get(8).map_err(|e| classify_error(&e))?,
                    })
                })
                .collect::<DbResult<Vec<_>>>()?
        };

        Ok(assemble_table_ddl(&TableDdlParts {
            schema: schema.to_owned(),
            table: table.name.clone(),
            columns,
            constraints,
            indexes,
            sequences,
        }))
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        // sqlx::raw_sql uses the simple query protocol, which streams
        // row data and command-completion counts in one pass — so SELECT
        // and DML need no separate routing. Row-returning statements
        // expose rows and leave `rows_affected` at 0; pure DML leaves
        // `rows` empty and reports the affected count. Mixing both in
        // one call is not supported (`columns` would reflect the first
        // row-returning statement only).
        let pool = self.pool.current();
        let mut stream = sqlx::raw_sql(sql).fetch_many(&pool);

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

    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        // Prove a single read-only statement under the Postgres grammar,
        // and learn whether it is a cursor-able query or an EXPLAIN (a
        // utility statement that cannot be a cursor source).
        let kind = classify_read_only(sql, SqlDialect::Postgres)?;
        // The transaction body lives in a free `async fn`: nesting the
        // sqlx `Executor` borrows inside an `#[async_trait]` method trips
        // the "implementation of `Executor` is not general enough" HRTB
        // error, which a plain async fn with concrete lifetimes avoids.
        run_read_only_txn(self.pool.current(), sql, max_rows, kind).await
    }

    async fn execute(&self, sql: &str) -> DbResult<u64> {
        // Reuse the simple-query path: it already streams command-completion
        // counts, so a DML statement reports its affected count and a
        // row-returning statement (rare in a restore) runs and reports 0.
        self.query(sql).await.map(|result| result.rows_affected)
    }

    async fn execute_in_transaction(&self, statements: &[String]) -> DbResult<()> {
        // An empty batch would open and commit an empty transaction; skip it.
        if statements.is_empty() {
            return Ok(());
        }
        run_restore_txn(self.pool.current(), statements).await
    }
}

/// Apply `statements` as one atomic transaction on a pooled connection.
///
/// The sqlx `Transaction` rolls back on drop, so an early `?` return from a
/// failed statement leaves the target untouched rather than half-populated —
/// the all-or-nothing guarantee ADR-0051 relies on for `has_atomic_restore`
/// engines. Lives as a free `async fn` for the same reason as
/// [`run_read_only_txn`]: borrowing the sqlx `Executor` inside an
/// `#[async_trait]` method trips the "implementation of `Executor` is not
/// general enough" HRTB error.
async fn run_restore_txn(pool: PgPool, statements: &[String]) -> DbResult<()> {
    let mut tx = pool.begin().await.map_err(|e| classify_error(&e))?;
    for stmt in statements {
        // Deref-coerce `&mut Transaction` to a concrete `&mut PgConnection`
        // so the executor borrow has a single nameable lifetime, the same
        // reason `fetch_via_cursor` takes a concrete connection.
        exec_in_txn(&mut tx, stmt).await?;
    }
    tx.commit().await.map_err(|e| classify_error(&e))?;
    Ok(())
}

/// Run one statement inside the restore transaction via the extended query
/// protocol.
///
/// The read-only path uses the same protocol for the same reason: a held
/// `Transaction` future must stay `Send` across the `#[async_trait]`
/// boundary, and `raw_sql`'s simple-protocol executor bound trips the sqlx
/// `Executor`/`Send` HRTB error there. The extended protocol carries exactly
/// one command per round-trip, which the restore splitter already guarantees.
/// (The per-statement, non-atomic path — used by Aurora DSQL — goes through
/// [`PostgresAdapter::query`]'s `raw_sql`, so it keeps the simple protocol's
/// broader statement support.)
async fn exec_in_txn(conn: &mut sqlx::PgConnection, sql: &str) -> DbResult<()> {
    sqlx::query(sql)
        .execute(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;
    Ok(())
}

/// Execute a validated read-only statement inside a server-side
/// `READ ONLY` transaction and return at most `max_rows` rows.
///
/// A `READ ONLY` transaction makes Postgres itself reject every write for
/// its whole duration — INSERT / UPDATE / DELETE / DDL, `nextval()`,
/// data-modifying CTEs, and a writing `FOR UPDATE` — closing the
/// simple-query multi-statement and CTE-DML hazards even if the
/// classifier's grammar missed one. The sqlx `Transaction` rolls back on
/// drop, so an early `?` return never leaves the pooled connection
/// mid-transaction.
async fn run_read_only_txn(
    pool: PgPool,
    sql: &str,
    max_rows: usize,
    kind: ReadOnlyStatement,
) -> DbResult<QueryResult> {
    let mut tx = pool.begin().await.map_err(|e| classify_error(&e))?;
    // Two single statements (not one `raw_sql` batch): the simple-query
    // batch protocol widens the sqlx `Executor` lifetime bounds enough to
    // trip the "not general enough" HRTB error under `#[async_trait]`.
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|e| classify_error(&e))?;
    let timeout = format!("SET LOCAL statement_timeout = '{READ_ONLY_STATEMENT_TIMEOUT_SECS}s'");
    sqlx::query(&timeout)
        .execute(&mut *tx)
        .await
        .map_err(|e| classify_error(&e))?;

    let fetched = match kind {
        // A plain query becomes a server-side cursor so at most
        // `max_rows` rows ever cross the wire — an engine-level cap,
        // not a textual `LIMIT` wrapped around arbitrary SQL.
        ReadOnlyStatement::Query => fetch_via_cursor(&mut tx, sql, max_rows).await,
        // EXPLAIN returns a small, bounded plan and cannot be a cursor
        // source, so run it directly and materialise its rows.
        ReadOnlyStatement::Explain => run_capped(&mut tx, sql).await,
    };

    // Read-only txn: nothing to commit. Roll back to release the snapshot
    // promptly. Surface a fetch failure ahead of a rollback failure so
    // the caller sees the real cause.
    let rollback = tx.rollback().await;
    let mut result = fetched?;
    rollback.map_err(|e| classify_error(&e))?;
    result.truncate_rows(max_rows);
    Ok(result)
}

/// Row-cap a read-only query with a server-side cursor: `DECLARE` it over
/// the (already validated) statement, then `FETCH FORWARD max_rows`, so
/// the server materialises only the rows we keep.
///
/// Takes a concrete `&mut PgConnection` (not `&mut Transaction`) so the
/// executor borrow has a single, nameable lifetime — passing the
/// transaction and deref-ing inside trips the sqlx `Executor` HRTB error
/// under `#[async_trait]`.
async fn fetch_via_cursor(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    max_rows: usize,
) -> DbResult<QueryResult> {
    let declare = format!("DECLARE {READ_ONLY_CURSOR} NO SCROLL CURSOR FOR {sql}");
    sqlx::query(&declare)
        .execute(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;

    let fetch = format!("FETCH FORWARD {max_rows} FROM {READ_ONLY_CURSOR}");
    let rows = sqlx::query(&fetch)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;
    pg_rows_to_result(&rows)
}

/// Run `sql` directly on the connection (used for EXPLAIN, which cannot
/// be a cursor source) and materialise its rows.
async fn run_capped(conn: &mut sqlx::PgConnection, sql: &str) -> DbResult<QueryResult> {
    let rows = sqlx::query(sql)
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| classify_error(&e))?;
    pg_rows_to_result(&rows)
}

/// Build a [`QueryResult`] from already-fetched rows: columns come from
/// the first row (empty when there are none), matching the row-streaming
/// path in [`PostgresAdapter::query`].
fn pg_rows_to_result(rows: &[PgRow]) -> DbResult<QueryResult> {
    let columns = rows.first().map(columns_of).unwrap_or_default();
    let rows = rows
        .iter()
        .map(|row| Ok(Row::new(row_to_values(row)?)))
        .collect::<DbResult<Vec<_>>>()?;
    Ok(QueryResult {
        columns,
        rows,
        rows_affected: 0,
    })
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

/// One decoded row of [`FOREIGN_KEYS_SQL`], before rows are grouped into
/// composite [`ForeignKey`]s. Rows arrive ordered by constraint name then
/// key position, so a composite key's columns are consecutive and in order.
struct FkRow {
    constraint_name: String,
    local_column: String,
    referenced_schema: String,
    referenced_table: String,
    referenced_column: String,
}

/// Fold [`FOREIGN_KEYS_SQL`] rows into one [`ForeignKey`] per constraint.
///
/// Rows are pre-sorted by `(conname, key position)`, and a constraint name
/// is unique within a single relation, so every row of one constraint is
/// consecutive and in key order — folding against the last-built edge is
/// enough to assemble composite keys without a secondary group pass.
fn assemble_foreign_keys(rows: Vec<FkRow>) -> Vec<ForeignKey> {
    let mut out: Vec<ForeignKey> = Vec::new();
    for r in rows {
        let extends_last = out
            .last()
            .and_then(|fk| fk.constraint_name.as_deref())
            .is_some_and(|name| name == r.constraint_name);
        if extends_last {
            let last = out.last_mut().expect("extends_last implies a last edge");
            last.columns.push(r.local_column);
            last.referenced_columns.push(r.referenced_column);
        } else {
            out.push(ForeignKey {
                columns: vec![r.local_column],
                referenced_table: tuple_to_table(r.referenced_schema, r.referenced_table),
                referenced_columns: vec![r.referenced_column],
                constraint_name: Some(r.constraint_name),
            });
        }
    }
    out
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
    })
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
        assemble_foreign_keys, classify_error, column_from_parts, harden_ssl_mode,
        reclassify_schema, truncate, tuple_to_table, FkRow, FLAVOR_AURORA_DSQL, FLAVOR_NEON,
        FLAVOR_POSTGRES, FLAVOR_SUPABASE,
    };
    use dbboard_core::{DatabaseAdapter, DbError, ForeignKey, TableInfo};
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
            pool: super::PoolHandle::Static(pool),
            flavor: FLAVOR_POSTGRES,
        };
        assert!(adapter.capabilities().has_describe_table);
    }

    /// The DDL-reconstruction capability (ADR-0049) is advertised by every
    /// Postgres-wire flavor, including Aurora DSQL — DSQL degrades the
    /// *contents* (no FK/sequence sections), not the capability itself.
    #[tokio::test]
    async fn capabilities_advertise_table_ddl() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        for flavor in [FLAVOR_POSTGRES, FLAVOR_AURORA_DSQL] {
            let adapter = super::PostgresAdapter {
                pool: super::PoolHandle::Static(pool.clone()),
                flavor,
            };
            assert!(adapter.capabilities().has_table_ddl);
        }
    }

    /// Per-statement `execute` (ADR-0051) is advertised by every Postgres-wire
    /// flavor, Aurora DSQL included — a single statement runs everywhere.
    #[tokio::test]
    async fn capabilities_advertise_execute_on_every_flavor() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        for flavor in [
            FLAVOR_POSTGRES,
            FLAVOR_NEON,
            FLAVOR_SUPABASE,
            FLAVOR_AURORA_DSQL,
        ] {
            let adapter = super::PostgresAdapter {
                pool: super::PoolHandle::Static(pool.clone()),
                flavor,
            };
            assert!(adapter.capabilities().has_execute, "flavor {flavor}");
        }
    }

    /// Atomic restore (ADR-0051) is advertised by ordinary Postgres flavors
    /// but *not* Aurora DSQL, which cannot mix DDL and DML in one transaction
    /// (ADR-0021) and so falls back to per-statement execution.
    #[tokio::test]
    async fn only_non_dsql_flavors_advertise_atomic_restore() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        for flavor in [FLAVOR_POSTGRES, FLAVOR_NEON, FLAVOR_SUPABASE] {
            let adapter = super::PostgresAdapter {
                pool: super::PoolHandle::Static(pool.clone()),
                flavor,
            };
            assert!(adapter.capabilities().has_atomic_restore, "flavor {flavor}");
        }
        let dsql = super::PostgresAdapter {
            pool: super::PoolHandle::Static(pool),
            flavor: FLAVOR_AURORA_DSQL,
        };
        assert!(
            !dsql.capabilities().has_atomic_restore,
            "Aurora DSQL must not advertise atomic restore"
        );
    }

    /// Every flavor advertises foreign-key introspection (ADR-0054),
    /// including Aurora DSQL — the `pg_catalog` query works there and just
    /// returns no rows, since DSQL has no foreign-key constraints.
    #[tokio::test]
    async fn every_flavor_advertises_foreign_keys() {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        for flavor in [
            FLAVOR_POSTGRES,
            FLAVOR_NEON,
            FLAVOR_SUPABASE,
            FLAVOR_AURORA_DSQL,
        ] {
            let adapter = super::PostgresAdapter {
                pool: super::PoolHandle::Static(pool.clone()),
                flavor,
            };
            assert!(adapter.capabilities().has_foreign_keys, "flavor {flavor}");
        }
    }

    fn fk_row(
        constraint: &str,
        local: &str,
        ref_schema: &str,
        ref_table: &str,
        ref_col: &str,
    ) -> FkRow {
        FkRow {
            constraint_name: constraint.into(),
            local_column: local.into(),
            referenced_schema: ref_schema.into(),
            referenced_table: ref_table.into(),
            referenced_column: ref_col.into(),
        }
    }

    #[test]
    fn assemble_foreign_keys_builds_one_edge_per_constraint() {
        let edges = assemble_foreign_keys(vec![fk_row(
            "orders_customer_id_fkey",
            "customer_id",
            "public",
            "customers",
            "id",
        )]);
        assert_eq!(
            edges,
            vec![ForeignKey {
                columns: vec!["customer_id".into()],
                referenced_table: TableInfo::qualified("public", "customers"),
                referenced_columns: vec!["id".into()],
                constraint_name: Some("orders_customer_id_fkey".into()),
            }]
        );
    }

    #[test]
    fn assemble_foreign_keys_folds_a_composite_key_in_order() {
        // Two consecutive rows sharing a constraint name are one composite
        // edge; the SQL orders them by key position.
        let edges = assemble_foreign_keys(vec![
            fk_row("fk_ab", "a", "public", "parent", "pa"),
            fk_row("fk_ab", "b", "public", "parent", "pb"),
        ]);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].columns, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            edges[0].referenced_columns,
            vec!["pa".to_string(), "pb".to_string()]
        );
    }

    #[test]
    fn assemble_foreign_keys_separates_distinct_constraints() {
        let edges = assemble_foreign_keys(vec![
            fk_row("fk_one", "customer_id", "public", "customers", "id"),
            fk_row("fk_two", "product_id", "sales", "products", "id"),
        ]);
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].constraint_name.as_deref(), Some("fk_one"));
        assert_eq!(
            edges[1].referenced_table,
            TableInfo::qualified("sales", "products")
        );
    }

    #[test]
    fn assemble_foreign_keys_is_empty_without_rows() {
        assert!(assemble_foreign_keys(vec![]).is_empty());
    }

    /// A `Static` handle hands back exactly the pool it wraps. `max_connections`
    /// is an observable, network-free property of a lazily-built pool, so it
    /// stands in for pool identity here (`PgPool` exposes no identity of its
    /// own).
    #[tokio::test]
    async fn static_pool_handle_returns_the_wrapped_pool() {
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_lazy_with(PgConnectOptions::new());
        let handle = super::PoolHandle::Static(pool);
        assert_eq!(handle.current().options().get_max_connections(), 3);
    }

    /// Build an adapter over a lazily-built pool: no network I/O happens
    /// until a statement is actually executed, so tests that only exercise
    /// the pre-connection classifier stay hermetic.
    fn lazy_adapter() -> super::PostgresAdapter {
        let pool = PgPoolOptions::new().connect_lazy_with(PgConnectOptions::new());
        super::PostgresAdapter {
            pool: super::PoolHandle::Static(pool),
            flavor: FLAVOR_POSTGRES,
        }
    }

    /// `query_read_only` classifies before it connects: a write is
    /// rejected by the AST guard, so `pool.begin()` is never reached and
    /// the lazy (never-connected) pool never touches the network. This is
    /// the belt in front of the `BEGIN READ ONLY` engine backstop.
    #[tokio::test]
    async fn query_read_only_rejects_a_write_before_connecting() {
        let err = lazy_adapter()
            .query_read_only("DELETE FROM users", 100)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
        assert!(err.message().contains("read-only"), "message: {err}");
    }

    /// The simple-query multi-statement hazard (`SELECT 1; DROP TABLE t`
    /// would run both under the batch protocol) is rejected by the
    /// classifier before any connection is opened.
    #[tokio::test]
    async fn query_read_only_rejects_a_multi_statement_batch_before_connecting() {
        let err = lazy_adapter()
            .query_read_only("SELECT 1; DROP TABLE users", 100)
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
    }

    /// A data-modifying CTE (`WITH x AS (DELETE ...) SELECT`) is a write
    /// dressed as a query; the classifier rejects it pre-connection too.
    #[tokio::test]
    async fn query_read_only_rejects_a_data_modifying_cte_before_connecting() {
        let err = lazy_adapter()
            .query_read_only(
                "WITH gone AS (DELETE FROM users RETURNING id) SELECT * FROM gone",
                100,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, DbError::Query(_)));
    }

    /// The refreshing handle reads the *current* pool through the lock, so a
    /// background swap is visible to the next `current()` — the exact
    /// behaviour the token-refresh task (ADR-0037) relies on. Two lazy pools
    /// with distinct `max_connections` stand in for a stale vs. fresh pool.
    #[tokio::test]
    async fn refreshing_pool_handle_reflects_a_swap() {
        use std::sync::{Arc, RwLock};

        let stale = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(PgConnectOptions::new());
        let shared = Arc::new(RwLock::new(stale));
        let handle = super::PoolHandle::Refreshing(Arc::clone(&shared));
        assert_eq!(handle.current().options().get_max_connections(), 1);

        // Simulate the refresh task swapping a freshly authenticated pool in.
        let fresh = PgPoolOptions::new()
            .max_connections(2)
            .connect_lazy_with(PgConnectOptions::new());
        *shared.write().unwrap() = fresh;

        assert_eq!(handle.current().options().get_max_connections(), 2);
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

        let note = column_from_parts("note".into(), "text".into(), "YES", None, 2, &pk)
            .expect("note column");
        assert!(note.nullable);
        assert!(!note.primary_key);
        assert_eq!(note.declared_type.as_deref(), Some("text"));
    }

    #[test]
    fn column_from_parts_rejects_a_non_positive_ordinal() {
        // information_schema.ordinal_position is 1-based; anything else
        // indicates a broken catalog and must not be silently cast.
        let err = column_from_parts("x".into(), "text".into(), "YES", None, 0, &[])
            .expect_err("ordinal 0 should fail");
        assert!(matches!(err, DbError::TypeConversion(_)));
        let err = column_from_parts("x".into(), "text".into(), "YES", None, -1, &[])
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
