//! End-to-end against a real `MongoDB` server.
//!
//! The unit tests prove the crate builds the commands we *think* `MongoDB`
//! accepts. This proves it accepts them — that `run_cursor_command` really
//! wants the `cursor` option we inject, that an `_id` really comes back as an
//! `ObjectId`, and that a refused command really lands in the error kind the
//! adapter maps to [`DbError::Query`].
//!
//! Ignored by default: it needs a listening server, and CI has none. To run it,
//! in one shell
//!
//! ```sh
//! docker run -d --rm --name dbboard-mongo-test -p 27117:27017 mongo:8
//! ```
//!
//! and in another
//!
//! ```sh
//! DBBOARD_TEST_MONGODB_URI=mongodb://127.0.0.1:27117/dbboard_test \
//!   cargo test -p dbboard-mongodb --test live_mongodb -- --ignored
//! ```
//!
//! 27017 is the default; 27117 is used here because the default is often
//! already taken by something a developer is actually working on.
//!
//! Each test seeds the collection it reads, because a container started fresh
//! holds nothing. Seeding goes through the driver rather than the adapter —
//! the adapter has no write path at all, which is the property everything else
//! here relies on.

use dbboard_core::{DatabaseAdapter, DbError, TableInfo, Value};
use dbboard_mongodb::{MongoAdapter, MongoConfig};
use mongodb::bson::{doc, Document};
use mongodb::Client;

/// Where the server is listening. Absent means "nothing was started", and the
/// tests skip rather than fail — an ignored test a developer opted into should
/// still be honest about having done nothing.
fn uri() -> Option<String> {
    std::env::var("DBBOARD_TEST_MONGODB_URI")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

async fn adapter(uri: &str) -> MongoAdapter {
    MongoAdapter::connect(MongoConfig {
        uri: uri.to_string(),
        database: None,
    })
    .await
    .expect("the adapter should connect")
}

/// Replace `collection` with exactly `documents`, so a test reads what it put
/// there whatever an earlier run left behind.
async fn seed(uri: &str, collection: &str, documents: Vec<Document>) {
    let client = Client::with_uri_str(uri).await.expect("driver connects");
    let database = client.default_database().expect("the URI names a database");
    let handle = database.collection::<Document>(collection);
    handle.drop().await.expect("dropping is allowed for a test");
    if !documents.is_empty() {
        handle.insert_many(documents).await.expect("seeding works");
    }
}

fn cell<'a>(result: &'a dbboard_core::QueryResult, row: usize, column: &str) -> &'a Value {
    let index = result
        .columns
        .iter()
        .position(|c| c.name == column)
        .unwrap_or_else(|| panic!("no column named {column} in {:?}", result.columns));
    &result.rows[row].values()[index]
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn the_server_answers_a_ping() {
    let Some(uri) = uri() else { return };
    adapter(&uri)
        .await
        .ping()
        .await
        .expect("ping should answer");
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn collections_come_back_as_tables() {
    let Some(uri) = uri() else { return };
    seed(&uri, "listed", vec![doc! { "a": 1 }]).await;

    let tables = adapter(&uri)
        .await
        .list_tables()
        .await
        .expect("listing should work");
    assert!(
        tables.iter().any(|t| t.name == "listed"),
        "the seeded collection is missing from {tables:?}"
    );
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_find_returns_the_documents_as_rows() {
    let Some(uri) = uri() else { return };
    seed(
        &uri,
        "people",
        vec![
            doc! { "name": "a", "age": 30_i32 },
            doc! { "name": "b", "age": 40_i32 },
        ],
    )
    .await;

    let result = adapter(&uri)
        .await
        .query(r#"{"find": "people", "sort": {"age": 1}}"#)
        .await
        .expect("a find should run");

    assert_eq!(result.rows.len(), 2);
    // `_id` was never written by the test, so this also proves the server's
    // generated ObjectId survives the trip as its hex.
    assert_eq!(result.columns[0].name, "_id");
    assert!(matches!(cell(&result, 0, "_id"), Value::Text(_)));
    assert_eq!(cell(&result, 0, "name"), &Value::Text("a".to_string()));
    assert_eq!(cell(&result, 1, "age"), &Value::Integer(40));
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_query_by_id_finds_the_document_it_names() {
    let Some(uri) = uri() else { return };
    let id = mongodb::bson::oid::ObjectId::new();
    seed(&uri, "by_id", vec![doc! { "_id": id, "name": "found" }]).await;

    // The point of the extended-JSON parse: as a plain subdocument this filter
    // matches nothing.
    let query = format!(
        r#"{{"find": "by_id", "filter": {{"_id": {{"$oid": "{}"}}}}}}"#,
        id.to_hex()
    );
    let result = adapter(&uri)
        .await
        .query(&query)
        .await
        .expect("the query should run");

    assert_eq!(result.rows.len(), 1, "an $oid filter matched nothing");
    assert_eq!(cell(&result, 0, "name"), &Value::Text("found".to_string()));
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn an_aggregate_runs_with_the_cursor_option_the_crate_adds() {
    let Some(uri) = uri() else { return };
    seed(
        &uri,
        "sales",
        vec![doc! { "amount": 10_i32 }, doc! { "amount": 32_i32 }],
    )
    .await;

    // Written without a `cursor` option on purpose: the server refuses an
    // aggregate that has none, so this passing is what proves the injection.
    let result = adapter(&uri)
        .await
        .query(r#"{"aggregate": "sales", "pipeline": [{"$group": {"_id": null, "total": {"$sum": "$amount"}}}]}"#)
        .await
        .expect("an aggregate should run");

    assert_eq!(result.rows.len(), 1);
    assert_eq!(cell(&result, 0, "total"), &Value::Integer(42));
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_command_that_answers_in_one_document_reads_as_one_row() {
    let Some(uri) = uri() else { return };
    seed(&uri, "counted", vec![doc! { "a": 1 }, doc! { "a": 2 }]).await;

    let result = adapter(&uri)
        .await
        .query(r#"{"count": "counted"}"#)
        .await
        .expect("a count should run");

    assert_eq!(result.rows.len(), 1);
    // The server's bookkeeping is dropped, so what is left is the answer.
    assert_eq!(cell(&result, 0, "n"), &Value::Integer(2));
    assert!(
        result.columns.iter().all(|c| c.name != "ok"),
        "the reply kept the server's bookkeeping: {:?}",
        result.columns
    );
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_bounded_read_stops_at_the_row_it_was_given() {
    let Some(uri) = uri() else { return };
    let documents = (0..10).map(|n| doc! { "n": n }).collect();
    seed(&uri, "many", documents).await;

    let result = adapter(&uri)
        .await
        .query_read_only(r#"{"find": "many"}"#, 3)
        .await
        .expect("a bounded read should run");
    assert_eq!(result.rows.len(), 3);
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_collection_describes_from_a_sample() {
    let Some(uri) = uri() else { return };
    seed(
        &uri,
        "shaped",
        vec![doc! { "name": "a", "nickname": "n" }, doc! { "name": "b" }],
    )
    .await;

    let schema = adapter(&uri)
        .await
        .describe_table(&TableInfo::unqualified("shaped"))
        .await
        .expect("describing should work");

    let names: Vec<_> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["_id", "name", "nickname"]);
    let nickname = schema
        .columns
        .iter()
        .find(|c| c.name == "nickname")
        .unwrap();
    assert!(nickname.nullable);
    assert_eq!(
        nickname.declared_type.as_deref(),
        Some("string (1/2 sampled)")
    );
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_write_never_reaches_the_server() {
    let Some(uri) = uri() else { return };
    seed(&uri, "untouched", vec![doc! { "a": 1 }]).await;

    let adapter = adapter(&uri).await;
    let error = adapter
        .query(r#"{"delete": "untouched", "deletes": [{"q": {}, "limit": 0}]}"#)
        .await
        .unwrap_err();
    assert!(matches!(error, DbError::Query(_)), "{error:?}");

    // The refusal is only worth anything if the document is still there.
    let after = adapter
        .query(r#"{"count": "untouched"}"#)
        .await
        .expect("a count should run");
    assert_eq!(cell(&after, 0, "n"), &Value::Integer(1));
}

#[tokio::test]
#[ignore = "needs a MongoDB server; see the module docs"]
async fn a_command_the_server_turns_down_is_the_callers_fault() {
    let Some(uri) = uri() else { return };
    // A read the classifier allows but the server rejects: `$notAStage` is not
    // a pipeline stage. The mapping under test is that this is a query error
    // and not a connection error, because the connection is fine.
    let error = adapter(&uri)
        .await
        .query(r#"{"aggregate": "anything", "pipeline": [{"$notAStage": {}}]}"#)
        .await
        .unwrap_err();
    assert!(matches!(error, DbError::Query(_)), "{error:?}");
}
