//! The MCP wire layer: wrap [`McpService`] as the exposed tool set.
//!
//! This is a thin adapter over [`crate::service`]. Each `#[tool]` method
//! deserializes its typed parameters, calls the matching service method,
//! serializes the result to a JSON text block, and maps a
//! [`ServiceError`] onto the MCP error envelope. All the real work — and
//! all the security invariants (read-only enforcement, the write gate,
//! secret redaction) — live in the service; keeping this layer trivial is
//! deliberate.
//!
//! The tool set: `list_connections`, `list_tables`, `describe_table`,
//! `run_read_query`, `get_annotations` (ADR-0046 Decision 5), plus
//! `search_schema` (ADR-0053), `list_relationships` (ADR-0054) and
//! `run_write` (ADR-0087).
//!
//! Tool *descriptions* carry more of the write policy than a reader might
//! expect. They are the only documentation the agent gets before it acts:
//! a rule an agent only meets as an error is a rule it discovers by
//! breaking, and a permanent refusal it did not expect looks like a syntax
//! problem worth retrying.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use dbboard_core::DbError;

use crate::service::{McpService, ServiceError};

/// Parameters for [`DbboardMcp::list_tables`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTablesParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
}

/// Parameters for [`DbboardMcp::describe_table`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DescribeTableParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
    /// Schema namespace (e.g. `public` on Postgres). Omit for
    /// SQLite/libSQL/D1, which have no schema concept.
    #[serde(default)]
    pub schema: Option<String>,
    /// The table name.
    pub table: String,
}

/// Parameters for [`DbboardMcp::run_read_query`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunReadQueryParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
    /// A single read-only SQL statement (`SELECT` / `WITH` / `EXPLAIN`).
    pub sql: String,
    /// Maximum rows to return (default 200, hard cap 1000). More rows
    /// than this are dropped and `truncated` is set.
    #[serde(default)]
    pub max_rows: Option<usize>,
}

/// Parameters for [`DbboardMcp::get_annotations`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAnnotationsParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
    /// Restrict to one table. Use the schema-qualified key where the
    /// engine has schemas (`public.orders`), the bare name otherwise.
    #[serde(default)]
    pub table: Option<String>,
    /// Restrict to one column (keeps the table-level note as context).
    #[serde(default)]
    pub column: Option<String>,
}

/// Parameters for [`DbboardMcp::search_schema`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchSchemaParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
    /// Case-insensitive substring to match against table and column
    /// names. Must not be blank.
    pub pattern: String,
}

/// Parameters for [`DbboardMcp::list_relationships`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRelationshipsParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
    /// Restrict to relationships touching this table at either endpoint
    /// (the bare name, or the schema-qualified `public.orders` key).
    /// Case-insensitive. Omit for every relationship in the connection.
    #[serde(default)]
    pub table: Option<String>,
}

/// Parameters for [`DbboardMcp::run_write`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunWriteParams {
    /// The connection id from `list_connections`. It must have
    /// `mcp_write = true` set by a human in `connections.toml`.
    pub connection_id: String,
    /// A single write statement: `INSERT` / `UPDATE` / `DELETE` / `MERGE`,
    /// or `CREATE TABLE` / `VIEW` / `INDEX` / `SCHEMA` / `ALTER TABLE`.
    pub sql: String,
}

/// The MCP server: holds the shared [`McpService`] plus the generated
/// tool router. Cloned per request by `rmcp`, so both fields are cheap
/// to clone (`Arc` and a router of function pointers).
#[derive(Clone)]
pub struct DbboardMcp {
    service: Arc<McpService>,
    tool_router: ToolRouter<DbboardMcp>,
}

#[tool_router]
impl DbboardMcp {
    /// Wrap a service in the tool router.
    #[must_use]
    pub fn new(service: Arc<McpService>) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List the database connections dbboard is configured with. Returns each connection's id, display name, and kind (turso, postgres, d1, neon, supabase, aurora-dsql). Secrets are never included. Use a returned id with the other tools."
    )]
    async fn list_connections(&self) -> Result<CallToolResult, McpError> {
        let views = self
            .service
            .list_connections()
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&views)
    }

    #[tool(
        description = "List the tables in a connection's database. Pass a connection_id from list_connections."
    )]
    async fn list_tables(
        &self,
        Parameters(ListTablesParams { connection_id }): Parameters<ListTablesParams>,
    ) -> Result<CallToolResult, McpError> {
        let tables = self
            .service
            .list_tables(&connection_id)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&tables)
    }

    #[tool(
        description = "Describe one table: its columns (name, declared type, nullability, primary-key flag, ordinal) and primary key. `schema` is optional (the Postgres schema namespace; omit for SQLite/libSQL/D1)."
    )]
    async fn describe_table(
        &self,
        Parameters(DescribeTableParams {
            connection_id,
            schema,
            table,
        }): Parameters<DescribeTableParams>,
    ) -> Result<CallToolResult, McpError> {
        let out = self
            .service
            .describe_table(&connection_id, schema.as_deref(), &table)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    #[tool(
        description = "Run a single READ-ONLY SQL statement (SELECT / WITH / EXPLAIN) and return the rows. Writes, DDL, multi-statement batches, and locking reads (FOR UPDATE) are rejected at the database engine, not just by string matching. Returns at most `max_rows` rows (default 200, hard cap 1000) plus a `truncated` flag telling you there were more."
    )]
    async fn run_read_query(
        &self,
        Parameters(RunReadQueryParams {
            connection_id,
            sql,
            max_rows,
        }): Parameters<RunReadQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let out = self
            .service
            .run_read_query(&connection_id, &sql, max_rows)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    #[tool(
        description = "Get dbboard's local table/column notes for a connection — documentation the database itself may not store (SQLite/D1 have no column comments). Optionally filter to one `table` (schema-qualified key like `public.orders`, or the bare name) and/or one `column`."
    )]
    async fn get_annotations(
        &self,
        Parameters(GetAnnotationsParams {
            connection_id,
            table,
            column,
        }): Parameters<GetAnnotationsParams>,
    ) -> Result<CallToolResult, McpError> {
        let out = self
            .service
            .get_annotations(&connection_id, table.as_deref(), column.as_deref())
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    #[tool(
        description = "Find the tables and columns whose NAME contains a substring (case-insensitive) across a whole connection — the fast way to answer 'which table has the email column?' or 'which tables relate to orders?' without describe_table on every table. Returns each matching table with a `table_name_matched` flag and the list of matched columns (empty when only the table name matched — call describe_table for its full columns). Matches identifiers only, not row data. On a very large schema, narrow with a specific substring."
    )]
    async fn search_schema(
        &self,
        Parameters(SearchSchemaParams {
            connection_id,
            pattern,
        }): Parameters<SearchSchemaParams>,
    ) -> Result<CallToolResult, McpError> {
        let out = self
            .service
            .search_schema(&connection_id, &pattern)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    #[tool(
        description = "Discover the foreign-key relationships in a connection — the schema's join graph. Returns directed edges (from child columns to the parent table's columns) so you can plan JOINs and understand the data model without reading DDL. Pass a `table` to get every relationship touching it on EITHER side at once: both what it references (its parents) and what references it (its children) — the fast way to answer 'how is orders connected?'. Omit `table` for the whole graph. Engines without foreign keys (Aurora DSQL) return no edges. Results are capped with a `truncated` flag."
    )]
    async fn list_relationships(
        &self,
        Parameters(ListRelationshipsParams {
            connection_id,
            table,
        }): Parameters<ListRelationshipsParams>,
    ) -> Result<CallToolResult, McpError> {
        let out = self
            .service
            .list_relationships(&connection_id, table.as_deref())
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    // The description spells out the closed list and the flag because an
    // agent that only learns them from an error will try the statement
    // first. Saying "a human must enable it" up front is what stops a
    // refused write turning into a retry loop.
    #[tool(
        description = "Run a single WRITE statement: INSERT / UPDATE / DELETE / MERGE, or CREATE TABLE / VIEW / INDEX / SCHEMA / ALTER TABLE. Returns the category it was allowed under (`data` or `schema`) and the engine's affected-row count (DDL usually reports 0, which is not a failure). \
        \n\nTwo things will refuse you. (1) The connection must have `mcp_write = true` set by a human in connections.toml; it is off by default and you cannot turn it on — report it and ask. (2) GRANT / REVOKE, anything creating or altering a database USER or ROLE, TRUNCATE, and DROP are refused permanently on every connection: no setting enables them, so do not rephrase — say a human must run it in the dbboard app. Reads are refused here too; use run_read_query, which caps its rows. One statement per call — batches are refused."
    )]
    async fn run_write(
        &self,
        Parameters(RunWriteParams { connection_id, sql }): Parameters<RunWriteParams>,
    ) -> Result<CallToolResult, McpError> {
        let out = self
            .service
            .run_write(&connection_id, &sql)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }
}

// `router = self.tool_router` points the generated `call_tool`/`list_tools`
// at the router stored on the struct. Without it the macro defaults to
// `Self::tool_router()`, which rebuilds the router on every call and
// leaves the field unread (a denied dead-code warning under our lints).
#[tool_handler(router = self.tool_router)]
impl ServerHandler for DbboardMcp {
    fn get_info(&self) -> ServerInfo {
        // `ServerInfo::new` seeds `server_info` from the crate's build env
        // (name + version); the builder methods layer on the rest. The
        // struct is `#[non_exhaustive]`, so a literal is not an option.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Read-only access to the databases dbboard is configured with. \
                 Start with list_connections to discover connection ids, then \
                 list_tables / describe_table to explore a schema (or search_schema \
                 to jump straight to the tables/columns whose name matches a term, \
                 or list_relationships to map the foreign-key join graph), \
                 run_read_query to read data (SELECT/WITH/EXPLAIN only), and \
                 get_annotations for dbboard's local notes on tables and columns. \
                 \n\nWriting goes through run_write, and only on a connection a human \
                 has set mcp_write = true on — it is off by default. Privilege and \
                 role changes, TRUNCATE and DROP are refused on every connection and \
                 no setting enables them.",
            )
    }
}

/// Serialize a tool result to a pretty-printed JSON text block.
fn json_block<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(format!("failed to serialize result: {e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// Map a service error onto the MCP error envelope. A bad connection id
/// or a statement the engine rejected (a write, DDL, bad SQL, an unknown
/// table) is the caller's mistake — `invalid_params`. A config/keyring/
/// task failure, or a transient backend outage, is not something the
/// caller can fix by editing its request — `internal_error`, which tells
/// the agent to retry rather than rewrite. Neither path embeds a secret.
fn to_mcp(err: &ServiceError) -> McpError {
    let message = err.to_string();
    match err {
        // Environment failures, not bad requests. `Db(Connection)` is matched
        // here (ahead of the blanket `Db` arm below) so a backend drop never
        // reads as a caller error. Config/Annotations/Task are local I/O; Dump
        // is a desktop-only sink failure (a full disk, a revoked path) and
        // Restore a desktop-only whole-run failure (a rolled-back atomic
        // batch) — both never reach an MCP tool but are environment faults if
        // they ever did.
        ServiceError::Db(DbError::Connection(_))
        | ServiceError::Config(_)
        | ServiceError::Annotations(_)
        | ServiceError::Task(_)
        | ServiceError::Dump(_)
        | ServiceError::Restore(_) => McpError::internal_error(message, None),
        // An unknown id, or any other DbError (rejected write, bad SQL,
        // unknown table, unsupported capability), is attributable to what the
        // caller sent. WriteBack / NotEditable / NotDumpable / NotRestorable
        // belong to the desktop write path and never reach an MCP tool call,
        // but they are still caller-attributable if they ever did — refusing a
        // bad plan or an un-dumpable/un-restorable adapter is not an
        // environment fault.
        //
        // The two write gates (ADR-0087) are caller-attributable too, but for
        // opposite reasons. `WriteRefused` is the statement's fault and the
        // agent can act on it — rephrase, or stop if the refusal says
        // permanent. `WriteNotEnabled` is not something the agent can fix at
        // all, but it is still not a retry: retrying a closed gate loops
        // forever, whereas `invalid_params` tells the agent to say so and ask
        // a human to open it.
        ServiceError::ConnectionNotFound(_)
        | ServiceError::InvalidRequest(_)
        | ServiceError::Db(_)
        | ServiceError::WriteBack(_)
        | ServiceError::NotEditable(_)
        | ServiceError::NotDumpable(_)
        | ServiceError::NotRestorable(_)
        | ServiceError::WriteNotEnabled(_)
        | ServiceError::WriteRefused(_) => McpError::invalid_params(message, None),
    }
}

#[cfg(test)]
mod tests {
    use super::to_mcp;
    use crate::service::ServiceError;
    use dbboard_core::DbError;
    use rmcp::model::ErrorCode;

    #[test]
    fn unknown_connection_is_a_bad_request() {
        let err = to_mcp(&ServiceError::ConnectionNotFound("nope".into()));
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn a_blank_search_pattern_is_a_bad_request() {
        let err = to_mcp(&ServiceError::InvalidRequest("blank".into()));
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn a_rejected_write_is_a_bad_request() {
        // A read-only violation surfaces as DbError::Query — the caller
        // sent a statement it should not have.
        let err = to_mcp(&ServiceError::Db(DbError::Query("write rejected".into())));
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn a_transient_connection_drop_is_our_problem_not_a_bad_request() {
        // The agent should retry, not treat its own SQL as invalid.
        let err = to_mcp(&ServiceError::Db(DbError::Connection(
            "host unreachable".into(),
        )));
        assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    }
}
