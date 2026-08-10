//! End-to-end against a real Firestore server, in practice the emulator.
//!
//! The unit tests run against `wiremock`, so they prove the adapter sends what
//! we *think* Firestore accepts. This proves it accepts them — that
//! `:listCollectionIds` really answers with the ids `list_tables` reports,
//! that a `StructuredQuery` posted to `:runQuery` really comes back as the
//! `document`-wrapped stream the parser expects, and that `limit` really
//! bounds the reply rather than being ignored.
//!
//! Ignored by default: it needs a listening server, and CI has none. To run it,
//! in one shell
//!
//! ```sh
//! firebase emulators:start --only firestore --project demo-dbboard
//! ```
//!
//! with a `firebase.json` that pins the port, and in another
//!
//! ```sh
//! DBBOARD_TEST_FIRESTORE_URL=http://127.0.0.1:8385/v1 \
//!   DBBOARD_TEST_FIRESTORE_PROJECT=demo-dbboard \
//!   cargo test -p dbboard-firestore --test live_firestore -- --ignored
//! ```
//!
//! 8080 is the emulator's default; 8385 is used here because the default is
//! often already taken by something a developer is actually working on. The
//! `demo-` prefix on the project id is the Firebase tooling's own marker for
//! "this project does not exist" — it refuses to reach production for one.
//!
//! Each test seeds the collection it reads, because an emulator started fresh
//! holds nothing. Seeding goes over plain REST rather than through the adapter
//! — the adapter has no write path at all, which is the property everything
//! else here relies on.

use dbboard_core::{DatabaseAdapter, TableInfo, Value};
use dbboard_firestore::{FirestoreAdapter, FirestoreConfig, FirestoreCredentials};
use serde_json::{json, Value as JsonValue};

/// Where the emulator is listening, including the `/v1` version segment.
/// Absent means "nothing was started", and the tests skip rather than fail —
/// an ignored test a developer opted into should still be honest about having
/// done nothing.
fn base_url() -> Option<String> {
    std::env::var("DBBOARD_TEST_FIRESTORE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn project_id() -> String {
    std::env::var("DBBOARD_TEST_FIRESTORE_PROJECT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "demo-dbboard".to_owned())
}

fn adapter(base_url: &str) -> FirestoreAdapter {
    FirestoreAdapter::connect(FirestoreConfig {
        project_id: project_id(),
        database_id: None,
        credentials: FirestoreCredentials::Emulator,
        base_url: Some(base_url.to_owned()),
    })
    .expect("the adapter should build")
}

fn documents_url(base_url: &str) -> String {
    format!(
        "{base_url}/projects/{}/databases/(default)/documents",
        project_id()
    )
}

/// Replace `collection` with exactly `documents`, so a test reads what it put
/// there whatever an earlier run left behind. `documents` maps a document id
/// to its `fields` object in Firestore's typed-value encoding.
async fn seed(base_url: &str, collection: &str, documents: &[(&str, JsonValue)]) {
    let http = reqwest::Client::new();
    let root = documents_url(base_url);

    // The emulator has no "drop collection", so the ids this test is about to
    // write are deleted one by one. Anything else another test left behind in
    // the same collection would break the counts, which is why every test here
    // uses a collection of its own.
    for (id, _) in documents {
        let _ = http
            .delete(format!("{root}/{collection}/{id}"))
            .send()
            .await;
    }

    for (id, fields) in documents {
        let response = http
            .post(format!("{root}/{collection}"))
            .query(&[("documentId", *id)])
            .json(&json!({ "fields": fields }))
            .send()
            .await
            .expect("the emulator should accept a write");
        assert!(
            response.status().is_success(),
            "seeding {collection}/{id} failed: {}",
            response.status()
        );
    }
}

fn table(name: &str) -> TableInfo {
    TableInfo {
        name: name.to_owned(),
        schema: None,
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
#[ignore = "needs a Firestore emulator; see the module docs"]
async fn the_server_answers_a_ping() {
    let Some(url) = base_url() else { return };
    adapter(&url).ping().await.expect("ping should answer");
}

#[tokio::test]
#[ignore = "needs a Firestore emulator; see the module docs"]
async fn collections_come_back_as_tables() {
    let Some(url) = base_url() else { return };
    seed(
        &url,
        "live_listed",
        &[("only", json!({ "a": { "integerValue": "1" } }))],
    )
    .await;

    let tables = adapter(&url)
        .list_tables()
        .await
        .expect("listing should work");
    assert!(
        tables.iter().any(|t| t.name == "live_listed"),
        "the seeded collection is missing from {tables:?}"
    );
}

#[tokio::test]
#[ignore = "needs a Firestore emulator; see the module docs"]
async fn a_structured_query_returns_the_documents_as_rows() {
    let Some(url) = base_url() else { return };
    seed(
        &url,
        "live_people",
        &[
            (
                "a",
                json!({ "name": { "stringValue": "a" }, "age": { "integerValue": "30" } }),
            ),
            (
                "b",
                json!({ "name": { "stringValue": "b" }, "age": { "integerValue": "40" } }),
            ),
        ],
    )
    .await;

    let result = adapter(&url)
        .query(
            r#"{"from": [{"collectionId": "live_people"}],
                "orderBy": [{"field": {"fieldPath": "age"}, "direction": "ASCENDING"}]}"#,
        )
        .await
        .expect("a structured query should run");

    assert_eq!(result.rows.len(), 2);
    assert_eq!(cell(&result, 0, "name"), &Value::Text("a".to_string()));
    assert_eq!(cell(&result, 1, "age"), &Value::Integer(40));
}

#[tokio::test]
#[ignore = "needs a Firestore emulator; see the module docs"]
async fn a_bounded_read_stops_at_the_row_it_was_given() {
    let Some(url) = base_url() else { return };
    let documents: Vec<(String, JsonValue)> = (0..10)
        .map(|n| {
            (
                format!("d{n}"),
                json!({ "n": { "integerValue": n.to_string() } }),
            )
        })
        .collect();
    let borrowed: Vec<(&str, JsonValue)> = documents
        .iter()
        .map(|(id, fields)| (id.as_str(), fields.clone()))
        .collect();
    seed(&url, "live_many", &borrowed).await;

    let result = adapter(&url)
        .query_read_only(r#"{"from": [{"collectionId": "live_many"}]}"#, 3)
        .await
        .expect("a bounded read should run");
    assert_eq!(result.rows.len(), 3);
}

#[tokio::test]
#[ignore = "needs a Firestore emulator; see the module docs"]
async fn a_collection_describes_from_a_sample() {
    let Some(url) = base_url() else { return };
    seed(
        &url,
        "live_described",
        &[(
            "one",
            json!({
                "label": { "stringValue": "x" },
                "count": { "integerValue": "7" },
                "flag": { "booleanValue": true }
            }),
        )],
    )
    .await;

    let schema = adapter(&url)
        .describe_table(&table("live_described"))
        .await
        .expect("describing should work");

    let names: Vec<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
    for expected in ["label", "count", "flag"] {
        assert!(
            names.contains(&expected),
            "the sample missed {expected}: {names:?}"
        );
    }
}

#[tokio::test]
#[ignore = "needs a Firestore emulator; see the module docs"]
async fn a_query_the_server_turns_down_is_the_callers_fault() {
    let Some(url) = base_url() else { return };

    // An operator that does not exist, so the server rejects the query itself
    // rather than the request around it. A 400 is what tells the adapter to
    // blame the text.
    //
    // Not "a query with no `from`": Firestore accepts that one and scans every
    // collection under the documents root, which is a legal — if alarming —
    // read.
    let error = adapter(&url)
        .query(
            r#"{"from": [{"collectionId": "live_people"}],
                "where": {"fieldFilter": {"field": {"fieldPath": "age"},
                                          "op": "NOT_A_REAL_OPERATOR",
                                          "value": {"integerValue": "1"}}}}"#,
        )
        .await
        .expect_err("a query with an unknown operator should be refused");

    assert!(
        matches!(error, dbboard_core::DbError::Query(_)),
        "a malformed query surfaced as {error:?} rather than a query error"
    );
}
