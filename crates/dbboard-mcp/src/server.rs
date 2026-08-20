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
//! `search_schema` (ADR-0053), `list_relationships` (ADR-0054),
//! `run_write` + `dump_database` (ADR-0087), the `get_ui_locale` /
//! `set_ui_locale` pair, `capture_window` (ADR-0108), and the four that
//! work the window — `set_editor_sql`, `run_query`, `open_ai_panel`,
//! `open_ai_settings` (ADR-0109). Those last seven reach no database at
//! all, and `get_server_info` (#195) reaches nothing whatever: it names
//! the build, because this binary is installed by hand and a stale one
//! is otherwise indistinguishable from a broken one.
//!
//! Tool *descriptions* carry more of the write policy than a reader might
//! expect. They are the only documentation the agent gets before it acts:
//! a rule an agent only meets as an error is a rule it discovers by
//! breaking, and a permanent refusal it did not expect looks like a syntax
//! problem worth retrying.

use std::path::Path;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use dbboard_config::UiCommand;
use dbboard_core::DbError;

use crate::capture::{self, CaptureError};
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
    /// SQLite/libSQL/D1/Firestore/MongoDB, which have no schema concept.
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
    /// The query text, in whatever language the connection speaks: a single
    /// read-only SQL statement (`SELECT` / `WITH` / `EXPLAIN`) on a SQL
    /// connection, a Firestore `StructuredQuery` as JSON on a `firestore`
    /// one, or a `MongoDB` command document as JSON on a `mongodb` one. Named
    /// `sql` for compatibility with agents that already call this
    /// tool; renaming it would break them for a connection kind most of them
    /// will never see.
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

/// Parameters for [`DbboardMcp::set_ui_locale`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetUiLocaleParams {
    /// One of the codes `get_ui_locale` returns in `supported`, exactly as
    /// spelled there (`ja`, `zh-CN`, `pt-BR`). Matching is case-sensitive and
    /// there is no fuzzy resolution: `ja-JP` and `JA` are refused.
    pub locale: String,
}

/// Parameters for [`DbboardMcp::capture_window`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptureWindowParams {
    /// Longest edge of the returned image, in pixels. Defaults to 1400,
    /// which is full size for an unmaximised window. A capture is never
    /// enlarged, so asking for more than the window is does nothing.
    #[serde(default)]
    pub max_edge: Option<u32>,
}

/// Parameters for [`DbboardMcp::set_editor_sql`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetEditorSqlParams {
    /// The text to put in the editor. Replaces what is there — there is no
    /// append, and no way to read back what it displaced.
    pub sql: String,
}

/// Parameters for [`DbboardMcp::dump_database`].
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DumpDatabaseParams {
    /// The connection id from `list_connections`.
    pub connection_id: String,
    /// Absolute path of the `.sql` file to create. Its directory must
    /// already exist and the file must not — dumps never overwrite.
    pub output_path: String,
}

/// The MCP server: holds the shared [`McpService`] plus the generated
/// tool router. Cloned per request by `rmcp`, so both fields are cheap
/// to clone (`Arc` and a router of function pointers).
#[derive(Clone)]
/// Which build is answering (#195).
///
/// `dbboard-mcp` is copied into place by hand and never replaces itself, so
/// nothing about a running instance says how old it is. This is the answer.
///
/// It is deliberately only the build. The obvious companion field — which
/// `connections.toml` this instance reads — is the one field that must not
/// be here: on Windows that path is `C:\Users\<operator>\…`, and a tool
/// result lands in the calling agent's transcript as plaintext on disk. A
/// version number diagnoses a stale binary without naming anybody.
#[derive(Debug, Serialize)]
pub struct BuildInfo {
    /// The crate name, so a result pasted into a bug report says what it is.
    pub name: &'static str,
    /// The version this binary was built at.
    pub version: &'static str,
}

/// The build answering this session.
#[must_use]
pub fn build_info() -> BuildInfo {
    BuildInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    }
}

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

    /// Translate the `connection_id` an agent supplied into the real id
    /// (ADR-0088).
    ///
    /// Every tool below goes through this. What `list_connections` hands out
    /// may be an operator-chosen alias, and once a connection has one its
    /// real id stops being a valid handle — so a tool that skipped this would
    /// both reject the only id the agent was given and quietly accept the one
    /// it was not supposed to know.
    async fn resolve(&self, handle: &str) -> Result<String, McpError> {
        self.service
            .resolve_agent_handle(handle)
            .await
            .map_err(|e| to_mcp(&e))
    }

    #[tool(
        description = "List the database connections dbboard is configured with. Returns each connection's id, display name, and kind (turso, d1, postgres, mysql, neon, supabase, aurora-dsql, aurora-dsql-iam, firestore, mongodb). Check the kind before you write a query: every kind above except firestore and mongodb takes SQL, firestore takes a Firestore StructuredQuery as JSON, and mongodb takes a MongoDB command document as JSON. Secrets are never included, and an operator may have replaced a connection's id and name with a neutral alias — the id you get back is the one to use, and there is no other. Use a returned id with the other tools."
    )]
    async fn list_connections(&self) -> Result<CallToolResult, McpError> {
        let views = self
            .service
            .list_agent_connections()
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
        let connection_id = self.resolve(&connection_id).await?;
        let tables = self
            .service
            .list_tables(&connection_id)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&tables)
    }

    #[tool(
        description = "Describe one table: its columns (name, declared type, nullability, primary-key flag, ordinal) and primary key. `schema` is optional (the Postgres schema namespace; omit for SQLite/libSQL/D1/Firestore/MongoDB). On a firestore or mongodb connection the table is a collection and the columns are inferred from a bounded sample of documents, so treat the result as evidence of what is usually there rather than a declared schema."
    )]
    async fn describe_table(
        &self,
        Parameters(DescribeTableParams {
            connection_id,
            schema,
            table,
        }): Parameters<DescribeTableParams>,
    ) -> Result<CallToolResult, McpError> {
        let connection_id = self.resolve(&connection_id).await?;
        let out = self
            .service
            .describe_table(&connection_id, schema.as_deref(), &table)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    #[tool(
        description = "Run a single READ-ONLY query and return the rows. On a SQL connection that is one statement (SELECT / WITH / EXPLAIN); writes, DDL, multi-statement batches, and locking reads (FOR UPDATE) are rejected at the database engine, not just by string matching. On a firestore connection the query is not SQL at all: send a Firestore StructuredQuery as JSON, with or without the outer structuredQuery wrapper — a bounded example is {`from`: [{`collectionId`: `orders`}], `limit`: 100} with real double quotes. Firestore reads through an endpoint that has no write form, so there is nothing there to reject. On a mongodb connection send a command document as JSON — a bounded example is {`find`: `orders`, `limit`: 100} with real double quotes, and `aggregate`, `count`, `distinct`, `listCollections`, and `listIndexes` are accepted too; anything that writes is refused. Returns at most `max_rows` rows (default 200, hard cap 1000) plus a `truncated` flag telling you there were more."
    )]
    async fn run_read_query(
        &self,
        Parameters(RunReadQueryParams {
            connection_id,
            sql,
            max_rows,
        }): Parameters<RunReadQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let connection_id = self.resolve(&connection_id).await?;
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
        let connection_id = self.resolve(&connection_id).await?;
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
        let connection_id = self.resolve(&connection_id).await?;
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
        let connection_id = self.resolve(&connection_id).await?;
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
        let connection_id = self.resolve(&connection_id).await?;
        let out = self
            .service
            .run_write(&connection_id, &sql)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    // The description leads with "before you write" because that is when a
    // dump is worth anything, and an agent that only reads the parameters
    // will take a backup after the change it wanted to be able to undo.
    #[tool(
        description = "Write a logical SQL dump of a whole connection to a file — take this before a run_write you might need to undo. Returns the path, table and row counts, byte size, and a `complete` flag; any tables the engine refused or cut short are named in `failed_tables` / `truncated_tables`, and the file is still valid SQL without them. \
        \n\nThis reads the database, so it does NOT need `mcp_write` — it works on every connection. `output_path` must be absolute, its directory must already exist, and the file must not: dumps never overwrite, so pick a fresh name rather than retrying the same one. Restoring is not available here; a human does that in the dbboard app."
    )]
    async fn dump_database(
        &self,
        Parameters(DumpDatabaseParams {
            connection_id,
            output_path,
        }): Parameters<DumpDatabaseParams>,
    ) -> Result<CallToolResult, McpError> {
        let connection_id = self.resolve(&connection_id).await?;
        let out = self
            .service
            .dump_to_file(&connection_id, Path::new(&output_path))
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&out)
    }

    // Reaches nothing at all — no database, no configuration, not even the
    // window. It exists because the operator's copy of this binary is a file
    // someone put somewhere once: it can sit several releases behind the
    // behaviour anyone expects of it, and an agent does not notice that a
    // result looks like a version-old bug (#195).
    #[tool(
        description = "Report which dbboard-mcp build is answering: `name` and `version`. This binary is installed by hand and never updates itself, so it can be stale — older than the fix for whatever you are looking at. Call it before reporting anything that behaves oddly, and quote the version in the report. It touches no database, reads no configuration, and returns nothing about the machine it runs on."
    )]
    async fn get_server_info(&self) -> Result<CallToolResult, McpError> {
        json_block(&build_info())
    }

    // The only pair that touches no database. They exist because switching
    // the language by hand — eleven times, restarting between — is the whole
    // cost of verifying the translations; an agent that can set it can walk
    // the sheet instead. The description says "when asked" because the effect
    // lands on someone's screen, and a language that changes on its own reads
    // as a fault, not a feature.
    #[tool(
        description = "Get dbboard's UI language setting. Returns `locale` (the persisted code, or null when the user has made no explicit choice and the app follows the OS language) and `supported` (every code set_ui_locale accepts). Nothing here touches a database."
    )]
    async fn get_ui_locale(&self) -> Result<CallToolResult, McpError> {
        json_block(&self.service.ui_locale())
    }

    #[tool(
        description = "Set dbboard's UI language. Takes one code from get_ui_locale's `supported` list, spelled exactly as it appears there — matching is case-sensitive and there is no fuzzy resolution, so `ja-JP` and `JA` are refused where `ja` is accepted. A running dbboard window picks the change up within about a second, with no restart. \
        \n\nDo this only when the user asks for it: it changes what they see on screen, and a language that changes on its own looks like a bug. It writes ui-settings.toml and touches no database."
    )]
    async fn set_ui_locale(
        &self,
        Parameters(SetUiLocaleParams { locale }): Parameters<SetUiLocaleParams>,
    ) -> Result<CallToolResult, McpError> {
        self.service
            .set_ui_locale(&locale)
            .map_err(|e| to_mcp(&e))?;
        json_block(&self.service.ui_locale())
    }

    // Reaches no database and not even dbboard's config — it photographs
    // whatever the app is drawing. The counterpart to set_ui_locale: that
    // one changes the interface, this one is how anything can be said about
    // the result. Checking that a locale renders, that a grid is not full of
    // tofu, that an error message is legible, are all questions about pixels.
    //
    // The privacy sentence in the description is not boilerplate. The window
    // lists the operator's real connections by name, so a capture is closer
    // to a screenshot of someone's desktop than to a tool result, and an
    // agent that pastes one into an issue has published it.
    #[tool(
        description = "Photograph the running dbboard window and return it as a PNG image, plus the window title and the size before and after scaling. Use it to check what the app actually renders — that a language change took effect, that text is not showing as boxes, that a message on screen is readable. Nothing here reads a database. \
        \n\nThe capture is of the real window, so it shows the operator's real connection names and whatever data is on screen. Treat it as their screen: describe what you see, but do not paste the image or its contents anywhere public — an issue, a pull request, a commit — without asking. \
        \n\nIt fails when dbboard is not running or its window is minimised. Neither is something you can fix by retrying: say so and ask a human to open or restore the window."
    )]
    async fn capture_window(
        &self,
        Parameters(CaptureWindowParams { max_edge }): Parameters<CaptureWindowParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_edge = max_edge.unwrap_or(capture::DEFAULT_MAX_EDGE);
        // Enumerating windows and copying pixels are blocking OS calls; on
        // the shared stdio runtime they would stall every other tool.
        let shot = tokio::task::spawn_blocking(move || capture::capture_window(max_edge))
            .await
            .map_err(|e| McpError::internal_error(format!("capture task failed: {e}"), None))?
            .map_err(|e| capture_to_mcp(&e))?;

        let text = serde_json::to_string_pretty(&shot).map_err(|e| {
            McpError::internal_error(format!("failed to serialize result: {e}"), None)
        })?;
        Ok(CallToolResult::success(vec![
            ContentBlock::image(shot.png_base64, "image/png"),
            ContentBlock::text(text),
        ]))
    }

    // The three below work the window rather than read a database (ADR-0109).
    // `capture_window` made the app's screen legible to an agent; without
    // these, everything it might want to look at still had to be typed and
    // clicked by the person sitting there. They go through ui-command.toml
    // and block until the window says what happened, so a success here means
    // the app really did it — not that the request was filed.
    #[tool(
        description = "Type SQL into the running dbboard window's query editor, replacing whatever is there, and bring the Query tab to the front. Does not run it — call run_query for that. Returns once the window has taken the text. \
        \n\nUse it to put a statement in front of the user, or to set up a check you are about to make with run_query and capture_window. It touches no database, and it does not save anything: the text lives in that window until someone changes it. \
        \n\nIt fails when dbboard is not running, and you cannot fix that by retrying — ask a human to open it."
    )]
    async fn set_editor_sql(
        &self,
        Parameters(SetEditorSqlParams { sql }): Parameters<SetEditorSqlParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ui_command(UiCommand::SetEditorSql { sql }).await
    }

    #[tool(
        description = "Press Run in the dbboard window: execute the SQL currently in its query editor against the connection the window has selected, and return once the rows are on screen. The answer says how many rows came back; use capture_window to see them. \
        \n\nThis is the window's own connection and the window's own row limit, not run_read_query's — reach for it when the point is what the app displays. When the point is the data, run_read_query is cheaper and does not disturb anyone's screen. \
        \n\nThe editor is not read-only: whatever is in it runs. Set it with set_editor_sql first unless you know what is there. \
        \n\nIt fails when dbboard is not running, when no connection is selected in the window, or when a query is already running there. None of those change by retrying."
    )]
    async fn run_query(&self) -> Result<CallToolResult, McpError> {
        self.ui_command(UiCommand::RunQuery).await
    }

    #[tool(
        description = "Open the AI panel in the dbboard window. Returns once it is open, and says so if it already was. \
        \n\nIt is how you reach the AI provider settings, which live inside that panel. Reaches no database and changes no configuration — it opens a panel on someone's screen, so do it when the user asks or when you are checking that screen for them. \
        \n\nIt fails when dbboard is not running, which retrying will not change."
    )]
    async fn open_ai_panel(&self) -> Result<CallToolResult, McpError> {
        self.ui_command(UiCommand::OpenAiPanel).await
    }

    #[tool(
        description = "Open the AI provider settings in the dbboard window. Returns once the dialog is up, and says so if it already was. \
        \n\nThese settings live *inside* the AI panel, so this is refused while that panel is closed — call open_ai_panel first. That refusal is not a bug to work around: there is no top-level route to provider settings, and the refusal is how you can tell. \
        \n\nReaches no database and writes no configuration by itself — it opens a dialog on someone's screen. \
        \n\nIt fails when dbboard is not running and when the AI panel is closed. Neither changes by retrying."
    )]
    async fn open_ai_settings(&self) -> Result<CallToolResult, McpError> {
        self.ui_command(UiCommand::OpenAiSettings).await
    }

    /// Send one instruction to the window and report what it said back.
    ///
    /// The four tools above differ only in the verb, and the answer shape is
    /// the same for all of them: `{ "ok": true, "detail": ... }`, where
    /// `detail` is the window's own words for what happened. A failure is an
    /// error, never an `ok: false` body — an agent that has to read the body
    /// to notice a refusal will eventually not read it.
    async fn ui_command(&self, command: UiCommand) -> Result<CallToolResult, McpError> {
        let detail = self
            .service
            .send_ui_command(command)
            .await
            .map_err(|e| to_mcp(&e))?;
        json_block(&UiCommandOutcome { ok: true, detail })
    }
}

/// What a UI command tool returns when the window carried it out.
#[derive(Debug, Serialize)]
struct UiCommandOutcome {
    ok: bool,
    /// The window's account of what it did — a row count, "editor set to N
    /// characters". `None` when it had nothing to add.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
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
            .with_instructions(instructions())
    }
}

/// The handshake text the client hands to the model.
///
/// A function rather than a literal so the running version can open it
/// (#195). The version is on two channels because neither alone is
/// dependable: this text reaches the model without anyone asking for it but
/// some clients drop it, and `get_server_info` always arrives but only if
/// something thinks to call it.
fn instructions() -> String {
    format!(
        "You are talking to dbboard-mcp {version}. This binary is installed by \
         hand and never updates itself, so it can be older than the behaviour \
         expected of it — if anything below looks like a bug, put that version \
         number in the report first, and call get_server_info if you need it \
         again later. \
         \n\n{GUIDE}",
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// How to use the tool set, minus the version preamble above.
const GUIDE: &str = "Read-only access to the databases dbboard is configured with. \
                 Start with list_connections to discover connection ids, then \
                 list_tables / describe_table to explore a schema (or search_schema \
                 to jump straight to the tables/columns whose name matches a term, \
                 or list_relationships to map the foreign-key join graph), \
                 run_read_query to read data (SELECT/WITH/EXPLAIN only), and \
                 get_annotations for dbboard's local notes on tables and columns. \
                 \n\nWriting goes through run_write, and only on a connection a human \
                 has set mcp_write = true on — it is off by default. Privilege and \
                 role changes, TRUNCATE and DROP are refused on every connection and \
                 no setting enables them. \
                 \n\nSeven tools are the exception to all of the above: they reach \
                 no database and instead work the dbboard window itself. \
                 get_ui_locale / set_ui_locale change the app's display language \
                 — use them only when the user asks. capture_window photographs \
                 the app's own window, which is how you check what it actually \
                 renders. set_editor_sql, run_query, open_ai_panel and \
                 open_ai_settings drive it: together with capture_window they let \
                 you check what the app shows rather than only what the database \
                 holds. \
                 \n\nThose seven act on a screen someone is sitting in front of. A \
                 capture shows the operator's real connection names — treat it as \
                 their screen and keep it out of anywhere public — and the other \
                 six change what they are looking at. All six fail outright when \
                 dbboard is not running, and no amount of retrying opens it. \
                 \n\nrun_query is not a substitute for run_read_query: it runs \
                 whatever is in that window's editor, against the connection that \
                 window has selected, and leaves the rows on the operator's screen. \
                 Reach for it when what the app displays is the point.";

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
        | ServiceError::WriteRefused(_)
        // Same reasoning as `capture_window`'s "not running" (ADR-0108): a
        // closed app stays closed until a human opens it, and `internal_error`
        // is the code that reads as "try again". A refusal from the window is
        // its own answer — the tab the command needed was not there — and
        // retrying the identical call cannot change that either.
        | ServiceError::UiNoResponse(_)
        | ServiceError::UiRefused(_) => McpError::invalid_params(message, None),
    }
}

/// Map a capture failure onto the MCP error envelope.
///
/// "Not running" and "minimised" are `invalid_params` for the same reason
/// `WriteNotEnabled` is: the agent cannot fix either by rewriting its call,
/// but neither is it worth retrying — a closed window stays closed until a
/// human opens it, and `internal_error` is the code that reads as "try
/// again". Only a genuine platform failure is retryable.
fn capture_to_mcp(err: &CaptureError) -> McpError {
    let message = err.to_string();
    match err {
        CaptureError::NotRunning | CaptureError::Minimized => {
            McpError::invalid_params(message, None)
        }
        CaptureError::Backend(_) => McpError::internal_error(message, None),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_info, capture_to_mcp, instructions, to_mcp};
    use crate::capture::CaptureError;
    use crate::service::ServiceError;
    use dbboard_core::DbError;
    use rmcp::model::ErrorCode;

    // Why any of the next four tests exist (#195): this binary is copied
    // into place by hand and never updates itself. A bug was once reported
    // against the MySQL path that had been fixed a release earlier — the
    // installed binary predated the fix, and nothing about it said so. The
    // agent could not tell, and neither could the operator.
    //
    // The version is therefore put on two channels, because neither alone
    // is reliable: `instructions` reaches the model without anyone asking
    // but some clients drop it, and a tool result always arrives but only
    // if something thinks to call it.

    #[test]
    fn the_handshake_names_the_running_build() {
        let text = instructions();
        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "the handshake does not say which build is answering"
        );
    }

    // Knowing the version is no use if the agent does not think to pass it
    // on. The report arrives as prose written by the agent, so the version
    // is in it only if the agent was told to put it there.
    #[test]
    fn the_handshake_asks_for_the_version_in_any_bug_report() {
        let text = instructions();
        assert!(
            text.contains("never updates itself"),
            "the handshake does not say the binary can be stale"
        );
        assert!(
            text.contains("get_server_info"),
            "the handshake does not name the tool that repeats the version"
        );
    }

    #[test]
    fn get_server_info_reports_the_running_build() {
        let info = build_info();
        assert_eq!(info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    // The obvious next field to add here is "which connections.toml am I
    // reading", and it is the one field that must never be added: on
    // Windows that path is `C:\Users\<operator>\...`, and a tool result
    // lands in the calling agent's transcript as plaintext on disk. The
    // version diagnoses a stale binary without naming anybody.
    #[test]
    fn the_build_report_carries_no_filesystem_path() {
        let json = serde_json::to_string(&build_info()).expect("BuildInfo serializes");
        for needle in ['\\', '/'] {
            assert!(
                !json.contains(needle),
                "a path separator reached the build report: {json}"
            );
        }
    }

    // A tool an agent has no reason to call is a tool that answers nothing.
    // Its description has to supply the reason, because the moment it is
    // useful — something behaved oddly — is not a moment anyone is reading
    // documentation.
    #[test]
    fn get_server_info_says_when_an_agent_would_want_it() {
        let description = description_above("get_server_info");
        assert!(
            description.contains("stale"),
            "get_server_info does not say the binary can be out of date"
        );
        assert!(
            description.contains("touches no database"),
            "get_server_info does not say it reaches no database"
        );
    }

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

    /// Every tool that takes a `connection_id` must translate it through
    /// [`DbboardMcp::resolve`] (ADR-0088).
    ///
    /// Checked against the source text rather than by calling the tools,
    /// because the failure this guards against is a *new* tool added without
    /// the line — which no test of the existing eight would catch. A tool that
    /// forgets it rejects the alias the agent was given and accepts the real
    /// id the operator hid, and both look like ordinary behaviour until
    /// someone reads a transcript.
    #[test]
    fn every_tool_taking_a_connection_id_resolves_it_first() {
        let source = include_str!("server.rs");
        for chunk in source.split("#[tool(").skip(1) {
            // Stop at the next attribute so a chunk is exactly one tool.
            let body = chunk.split("\n    #[").next().unwrap_or(chunk);
            if !body.contains("connection_id") {
                continue;
            }
            let name = body
                .split("async fn ")
                .nth(1)
                .and_then(|rest| rest.split('(').next())
                .unwrap_or("<unknown>");
            assert!(
                body.contains("self.resolve(&connection_id).await?"),
                "tool `{name}` uses the agent's connection_id without resolving it"
            );
        }
    }

    /// Extract the kind labels `service::kind_label` can return, from its
    /// source. The list in the `list_connections` description is the agent's
    /// only advance notice of what it might be handed, and it went stale twice
    /// (`MySQL`, then Aurora DSQL IAM) before Firestore made staleness costly:
    /// a kind the agent has never heard of is also the one kind whose queries
    /// are not SQL.
    fn kind_labels() -> Vec<String> {
        let source = include_str!("service.rs");
        let body = source
            .split("fn kind_label(kind: &ConnectionKind) -> &'static str {")
            .nth(1)
            .expect("kind_label has moved or been renamed")
            .split("\n}")
            .next()
            .expect("kind_label body is not terminated");
        body.split("=> \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .map(ToOwned::to_owned)
            .collect()
    }

    #[test]
    fn list_connections_names_every_kind_it_can_return() {
        let source = include_str!("server.rs");
        let description = source
            .split("description = \"List the database connections")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the list_connections description has moved");
        let labels = kind_labels();
        assert!(!labels.is_empty(), "no kind labels were extracted");
        for label in labels {
            assert!(
                description.contains(&label),
                "kind `{label}` is missing from the list_connections description"
            );
        }
    }

    /// Not every connection speaks SQL. Firestore's query text is a
    /// `StructuredQuery` in JSON (ADR-0093), so a description that says only
    /// "SQL" tells an agent to send something that cannot parse — and the
    /// error it gets back looks like its own mistake.
    #[test]
    fn the_read_query_tool_says_that_some_kinds_are_not_sql() {
        let source = include_str!("server.rs");
        let chunk = source
            .split("async fn run_read_query")
            .next()
            .expect("run_read_query has moved");
        let description = chunk
            .rsplit("description = \"")
            .next()
            .and_then(|rest| rest.split('"').next())
            .expect("the run_read_query description has moved");
        assert!(
            description.contains("firestore"),
            "run_read_query does not tell the agent what a firestore connection expects"
        );
        assert!(
            description.contains("StructuredQuery"),
            "run_read_query names firestore but not the query form it takes"
        );
        // MongoDB's query text is JSON too, but not the same JSON: the adapter
        // dispatches on the command name, so an agent that sends Firestore's
        // `from`/`collectionId` shape gets a rejected command.
        assert!(
            description.contains("mongodb"),
            "run_read_query does not tell the agent what a mongodb connection expects"
        );
        assert!(
            description.contains("find"),
            "run_read_query names mongodb but not the command it takes"
        );
    }

    #[test]
    fn a_transient_connection_drop_is_our_problem_not_a_bad_request() {
        // The agent should retry, not treat its own SQL as invalid.
        let err = to_mcp(&ServiceError::Db(DbError::Connection(
            "host unreachable".into(),
        )));
        assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    }

    // A closed window does not open by itself, so the one thing the agent
    // must not do is retry until it does.
    #[test]
    fn a_closed_window_is_not_something_to_retry() {
        assert_eq!(
            capture_to_mcp(&CaptureError::NotRunning).code,
            ErrorCode::INVALID_PARAMS
        );
        assert_eq!(
            capture_to_mcp(&CaptureError::Minimized).code,
            ErrorCode::INVALID_PARAMS
        );
    }

    #[test]
    fn a_platform_capture_failure_is_worth_retrying() {
        let err = capture_to_mcp(&CaptureError::Backend("GDI is busy".into()));

        assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    }

    // The window shows the operator's real connections, so a capture is a
    // photograph of someone's screen. An agent that only learns that after
    // pasting one into an issue has learned it too late.
    #[test]
    fn capture_window_warns_that_the_image_is_someones_screen() {
        let source = include_str!("server.rs");
        let chunk = source
            .split("async fn capture_window")
            .next()
            .expect("capture_window has moved");
        let description = chunk
            .rsplit("description = \"")
            .next()
            .and_then(|rest| rest.split('"').next())
            .expect("the capture_window description has moved");

        assert!(
            description.contains("connection names"),
            "capture_window does not warn that the image carries real connection names"
        );
        assert!(
            description.contains("public"),
            "capture_window does not tell the agent to keep the image out of public places"
        );
    }

    /// The `description = "..."` immediately above `async fn <name>`.
    fn description_above(name: &str) -> &'static str {
        let source = include_str!("server.rs");
        let chunk = source
            .split(&format!("async fn {name}"))
            .next()
            .unwrap_or_else(|| panic!("{name} has moved"));
        chunk
            .rsplit("description = \"")
            .next()
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_else(|| panic!("the {name} description has moved"))
    }

    // A window that is not open does not open by itself. An agent told only
    // that the tool "failed" will retry — three times, then give up and
    // report the app as broken, when the fix was to ask someone to start it.
    #[test]
    fn every_ui_command_says_a_closed_window_is_not_worth_retrying() {
        for name in [
            "set_editor_sql",
            "run_query",
            "open_ai_panel",
            "open_ai_settings",
        ] {
            let description = description_above(name);
            assert!(
                description.contains("not running"),
                "{name} does not tell the agent that dbboard may not be running"
            );
            assert!(
                description.contains("retry"),
                "{name} does not tell the agent that retrying will not help"
            );
        }
    }

    // run_read_query and run_query read the same databases by different
    // routes, and the cheap one disturbs nobody. An agent that reaches for
    // the window by default is putting rows on someone's screen to answer a
    // question that never needed a screen.
    #[test]
    fn run_query_points_at_run_read_query_for_plain_reads() {
        let description = description_above("run_query");

        assert!(
            description.contains("run_read_query"),
            "run_query does not tell the agent when to prefer run_read_query"
        );
        assert!(
            description.contains("connection the window has selected"),
            "run_query does not say whose connection it runs against"
        );
    }
}
