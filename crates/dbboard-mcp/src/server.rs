//! The MCP wire layer: wrap [`McpService`] as five read-only tools.
//!
//! This is a thin adapter over [`crate::service`]. Each `#[tool]` method
//! deserializes its typed parameters, calls the matching service method,
//! serializes the result to a JSON text block, and maps a
//! [`ServiceError`] onto the MCP error envelope. All the real work — and
//! all the security invariants (read-only enforcement, secret redaction)
//! — live in the service; keeping this layer trivial is deliberate.
//!
//! The tool set is fixed at five (ADR-0046 Decision 5): `list_connections`,
//! `list_tables`, `describe_table`, `run_read_query`, `get_annotations`.
//! There is no write path.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
                 list_tables / describe_table to explore a schema, run_read_query \
                 to read data (SELECT/WITH/EXPLAIN only — writes are rejected), and \
                 get_annotations for dbboard's local notes on tables and columns.",
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
/// or a rejected (non-read-only) statement is the caller's mistake —
/// `invalid_params`; a config/keyring/task failure is ours —
/// `internal_error`. Neither path embeds a secret.
fn to_mcp(err: &ServiceError) -> McpError {
    let message = err.to_string();
    match err {
        ServiceError::ConnectionNotFound(_) | ServiceError::Db(_) => {
            McpError::invalid_params(message, None)
        }
        ServiceError::Config(_) | ServiceError::Annotations(_) | ServiceError::Task(_) => {
            McpError::internal_error(message, None)
        }
    }
}
