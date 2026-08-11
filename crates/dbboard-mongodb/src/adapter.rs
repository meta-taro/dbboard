//! The `DatabaseAdapter` implementation.
//!
//! Two things here are `MongoDB`-specific enough to be worth stating up front.
//!
//! **The query text is a command document as JSON, not SQL.** So
//! [`MongoAdapter::query_read_only`] must override the trait default, which
//! would otherwise hand the JSON to the SQL classifier and refuse every query.
//! What it overrides it *with* is [`crate::read_only`], not nothing: unlike
//! Firestore, this adapter's read-only guarantee is a classifier, and every
//! path from caller text to the server runs through it (ADR-0095). The
//! adapter's own commands — the ping, the schema sample — are constants in
//! this file, and none of them writes.
//!
//! **A collection has no schema**, so `describe_table` samples and says so —
//! see [`crate::sample`].

use async_trait::async_trait;
use dbboard_core::{
    Capabilities, DatabaseAdapter, DbError, DbResult, QueryResult, TableInfo, TableSchema,
    MAX_RESULT_ROWS,
};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use mongodb::error::ErrorKind;
use mongodb::options::ClientOptions;
use mongodb::{Client, Database};

use crate::command;
use crate::document;
use crate::read_only::classify_read_only;
use crate::sample;

/// How this client names itself to the server, so a `currentOp` or a server
/// log says which tool ran a query.
const APP_NAME: &str = "dbboard";

/// Documents read to infer a collection's shape. Large enough that an
/// occasional field shows up, small enough that describing a table stays one
/// cheap query against a collection of any size.
const SCHEMA_SAMPLE_SIZE: i64 = 100;

#[derive(Debug, Clone)]
pub struct MongoConfig {
    /// A `mongodb://` or `mongodb+srv://` connection string.
    pub uri: String,
    /// The database to run commands against, or `None` to take the one the URI
    /// names in its path. A command has to go somewhere, so a connection with
    /// neither is refused rather than guessed at.
    pub database: Option<String>,
}

pub struct MongoAdapter {
    client: Client,
    database: String,
}

/// Hand-written so the connection string never reaches a log line: it usually
/// carries the password.
impl std::fmt::Debug for MongoAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoAdapter")
            .field("database", &self.database)
            .finish_non_exhaustive()
    }
}

impl MongoAdapter {
    /// Build an adapter from `config`.
    ///
    /// Performs no I/O for a `mongodb://` URI — call
    /// [`ping`](DatabaseAdapter::ping) to find out whether the server is
    /// actually reachable. A `mongodb+srv://` URI resolves its SRV record
    /// here, because that is what tells the driver which hosts to talk to.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Connection`] if the URI does not parse, if its SRV
    /// record cannot be resolved, or if no database is named by either the
    /// config or the URI.
    pub async fn connect(config: MongoConfig) -> DbResult<Self> {
        let mut options = ClientOptions::parse(&config.uri)
            .await
            .map_err(|e| DbError::Connection(e.to_string()))?;
        options.app_name = Some(APP_NAME.to_string());

        let database = config
            .database
            .filter(|name| !name.is_empty())
            .or_else(|| options.default_database.clone())
            .ok_or_else(|| {
                // Falling back to `admin` or `test` would run the caller's
                // query somewhere they never named, which is worse than not
                // connecting at all.
                DbError::Connection(
                    "the connection names no database: add one to the URI path or set it on the connection"
                        .to_owned(),
                )
            })?;

        let client =
            Client::with_options(options).map_err(|e| DbError::Connection(e.to_string()))?;
        Ok(Self { client, database })
    }

    fn db(&self) -> Database {
        self.client.database(&self.database)
    }

    /// Classify `text`, send what it approved, and shape the reply.
    ///
    /// The classification happens before anything touches the network, so a
    /// write is refused without the server ever hearing about it.
    async fn run(&self, text: &str, max_rows: usize) -> DbResult<QueryResult> {
        let command = classify_read_only(text)?;
        let wire = command::to_wire(text, command)?;

        if command.returns_cursor() {
            let documents = self.collect(wire, max_rows).await?;
            return Ok(document::to_result(&documents));
        }

        let reply = self
            .db()
            .run_command(wire)
            .await
            .map_err(|e| query_error(&e))?;
        Ok(document::reply_to_result(&reply))
    }

    /// Read at most `limit` documents from a cursor command.
    ///
    /// Stops pulling once `limit` is reached rather than draining the cursor:
    /// the rest of a collection is exactly what the caller asked not to be
    /// sent.
    async fn collect(&self, command: Document, limit: usize) -> DbResult<Vec<Document>> {
        let mut cursor = self
            .db()
            .run_cursor_command(command)
            .await
            .map_err(|e| query_error(&e))?;

        let mut documents = Vec::new();
        while documents.len() < limit {
            match cursor.try_next().await.map_err(|e| query_error(&e))? {
                Some(document) => documents.push(document),
                None => break,
            }
        }
        Ok(documents)
    }
}

/// Map a driver error onto the right kind of [`DbError`].
///
/// The distinction is the same one the Firestore adapter draws: a server
/// refusing the *command* is the caller's query being wrong, while everything
/// else — a closed socket, an unreachable host, a failed handshake — is the
/// connection.
fn query_error(error: &mongodb::error::Error) -> DbError {
    match *error.kind {
        // The server understood the command and turned it down; so did the
        // driver when it would not serialize. Either way the caller wrote
        // something the database will not run.
        ErrorKind::Command(_)
        | ErrorKind::InvalidArgument { .. }
        | ErrorKind::BsonSerialization(_)
        | ErrorKind::BsonDeserialization(_) => DbError::Query(error.to_string()),
        _ => DbError::Connection(error.to_string()),
    }
}

#[async_trait]
impl DatabaseAdapter for MongoAdapter {
    fn id(&self) -> &'static str {
        "mongodb"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            has_describe_table: true,
            // Everything else stays off. MongoDB has no foreign keys or DDL to
            // reconstruct, and this adapter cannot write at all — advertising
            // `execute` and then refusing it is worse than never offering it.
            ..Capabilities::default()
        }
    }

    async fn ping(&self) -> DbResult<()> {
        self.db()
            .run_command(doc! { "ping": 1 })
            .await
            .map(|_| ())
            .map_err(|e| DbError::Connection(e.to_string()))
    }

    async fn list_tables(&self) -> DbResult<Vec<TableInfo>> {
        let names = self
            .db()
            .list_collection_names()
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;
        Ok(names.iter().map(TableInfo::unqualified).collect())
    }

    async fn query(&self, sql: &str) -> DbResult<QueryResult> {
        // One past the cap, so "exactly at the limit" and "more than fits" are
        // distinguishable and the second one is an error rather than a silent
        // truncation.
        let result = self.run(sql, MAX_RESULT_ROWS + 1).await?;
        if result.rows.len() > MAX_RESULT_ROWS {
            return Err(dbboard_core::too_many_rows_error());
        }
        Ok(result)
    }

    /// Read-only through [`crate::read_only`].
    ///
    /// The trait default parses `sql` as Postgres, which would refuse every
    /// command document — and only on the MCP surface, where it would be
    /// hardest to notice. `run` classifies with the `MongoDB` classifier
    /// instead, so this is not a weaker check than the default, it is the
    /// applicable one.
    async fn query_read_only(&self, sql: &str, max_rows: usize) -> DbResult<QueryResult> {
        // Truncate rather than error — the caller asked for a bounded read.
        self.run(sql, max_rows).await
    }

    async fn describe_table(&self, table: &TableInfo) -> DbResult<TableSchema> {
        let command = doc! { "find": &table.name, "limit": SCHEMA_SAMPLE_SIZE };
        let documents = self
            .collect(
                command,
                usize::try_from(SCHEMA_SAMPLE_SIZE).unwrap_or(usize::MAX),
            )
            .await
            .map_err(|e| DbError::Schema(e.to_string()))?;
        Ok(sample::infer_schema(table, &documents))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port nothing listens on. Every test here stops before any I/O, and
    /// this address is what proves it: a test that reached the network would
    /// hang rather than pass.
    const NOWHERE: &str = "mongodb://127.0.0.1:1/shop";

    async fn adapter() -> MongoAdapter {
        MongoAdapter::connect(MongoConfig {
            uri: NOWHERE.to_string(),
            database: None,
        })
        .await
        .expect("a syntactically valid URI should connect lazily")
    }

    // --- construction -----------------------------------------------------

    #[tokio::test]
    async fn a_uri_that_is_not_mongodb_is_refused() {
        let error = MongoAdapter::connect(MongoConfig {
            uri: "postgres://localhost/shop".to_string(),
            database: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(error, DbError::Connection(_)), "{error:?}");
    }

    #[tokio::test]
    async fn the_database_can_come_from_the_uri() {
        assert_eq!(adapter().await.database, "shop");
    }

    #[tokio::test]
    async fn an_explicit_database_wins_over_the_uri() {
        let adapter = MongoAdapter::connect(MongoConfig {
            uri: NOWHERE.to_string(),
            database: Some("analytics".to_string()),
        })
        .await
        .unwrap();
        assert_eq!(adapter.database, "analytics");
    }

    #[tokio::test]
    async fn a_connection_that_names_no_database_anywhere_is_refused() {
        // Guessing `admin` or `test` would put a query somewhere the caller
        // did not ask for, which is worse than refusing to connect.
        let error = MongoAdapter::connect(MongoConfig {
            uri: "mongodb://127.0.0.1:1".to_string(),
            database: None,
        })
        .await
        .unwrap_err();
        assert!(matches!(error, DbError::Connection(_)), "{error:?}");
    }

    #[tokio::test]
    async fn the_debug_form_hides_the_connection_string() {
        let adapter = MongoAdapter::connect(MongoConfig {
            uri: "mongodb://user:hunter2@127.0.0.1:1/shop".to_string(),
            database: None,
        })
        .await
        .unwrap();
        let rendered = format!("{adapter:?}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(!rendered.contains("127.0.0.1"), "{rendered}");
    }

    // --- the classifier guards every caller path --------------------------

    #[tokio::test]
    async fn a_write_is_refused_rather_than_attempted() {
        let adapter = adapter().await;
        let write = r#"{"insert": "users", "documents": [{"a": 1}]}"#;
        let error = adapter.query(write).await.unwrap_err();
        assert!(matches!(error, DbError::Query(_)), "{error:?}");
    }

    #[tokio::test]
    async fn the_mcp_path_is_guarded_too() {
        let adapter = adapter().await;
        let write = r#"{"aggregate": "users", "pipeline": [{"$out": "copy"}]}"#;
        let error = adapter.query_read_only(write, 10).await.unwrap_err();
        assert!(matches!(error, DbError::Query(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_refusal_does_not_echo_the_command() {
        let adapter = adapter().await;
        let secret = "s3cret_collection";
        let write = format!(r#"{{"drop": "{secret}"}}"#);
        let error = adapter.query(&write).await.unwrap_err();
        assert!(
            !error.to_string().contains(secret),
            "the message repeated the caller's command: {error}"
        );
    }

    // --- what the adapter claims it can do --------------------------------

    #[tokio::test]
    async fn capabilities_claim_only_what_is_implemented() {
        let capabilities = adapter().await.capabilities();
        assert!(capabilities.has_describe_table);
        assert!(!capabilities.has_foreign_keys);
        assert!(!capabilities.has_table_ddl);
    }

    #[tokio::test]
    async fn writing_through_execute_is_not_offered() {
        // The trait default answers with a capability error, and that default
        // is deliberately not overridden.
        let error = adapter().await.execute("{\"drop\": \"users\"}").await;
        assert!(error.is_err());
    }
}
