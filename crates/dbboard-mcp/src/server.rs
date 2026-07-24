//! The MCP wire layer: wrap [`McpService`] as seven read-only tools.
//!
//! This is a thin adapter over [`crate::service`]. Each `#[tool]` method
//! deserializes its typed parameters, calls the matching service method,
//! serializes the result to a JSON text block, and maps a
//! [`ServiceError`] onto the MCP error envelope. All the real work — and
//! all the security invariants (read-only enforcement, secret redaction)
//! — live in the service; keeping this layer trivial is deliberate.
//!
//! The tool set: `list_connections`, `list_tables`, `describe_table`,
//! `run_read_query`, `get_annotations` (ADR-0046 Decision 5) plus
//! `search_schema` (ADR-0053) and `list_relationships` (ADR-0054). All
//! read-only — there is no write path.

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
                 run_read_query to read data (SELECT/WITH/EXPLAIN only — writes are \
                 rejected), and get_annotations for dbboard's local notes on tables \
                 and columns.",
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
        // A backend connection drop is an environment failure, not a bad
        // request — matched first so it wins over the blanket `Db` arm.
        ServiceError::Db(DbError::Connection(_)) => McpError::internal_error(message, None),
        // An unknown id, or any other DbError (rejected write, bad SQL,
        // unknown table, unsupported capability), is attributable to what
        // the caller sent.
        ServiceError::ConnectionNotFound(_)
        | ServiceError::InvalidRequest(_)
        | ServiceError::Db(_) => McpError::invalid_params(message, None),
        ServiceError::Config(_) | ServiceError::Annotations(_) | ServiceError::Task(_) => {
            McpError::internal_error(message, None)
        }
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
