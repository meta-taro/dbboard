//! End-to-end against the local Firestore emulator.
//!
//! Every HTTP path in `dbboard-firestore` is already covered by `wiremock`,
//! which proves the crate sends what we *think* Firestore accepts. This proves
//! Firestore accepts it — and it goes through [`connect_adapter`], so it also
//! covers the wiring the desktop client and the MCP server both use, rather
//! than the adapter alone.
//!
//! Ignored by default: it needs a listening emulator, and CI has none. To run
//! it, in one shell
//!
//! ```sh
//! firebase emulators:start --only firestore --project demo-dbboard
//! ```
//!
//! and in another
//!
//! ```sh
//! DBBOARD_TEST_FIRESTORE_EMULATOR=http://127.0.0.1:8080/v1 \
//!   cargo test -p dbboard-connect --test firestore_emulator -- --ignored
//! ```
//!
//! 8080 is the emulator's default; the variable exists because it is often
//! already taken, and a `firebase.json` that moves it must be matched here.
//!
//! The emulator holds nothing between runs, so each test seeds the documents
//! it reads. Seeding is a plain HTTP write rather than an adapter call because
//! the adapter has no write path at all — which is the property being relied
//! on everywhere else.

use dbboard_connect::{connect_adapter, BackendConfig};
use dbboard_core::TableInfo;
use dbboard_firestore::{FirestoreConfig, FirestoreCredentials};
use serde_json::json;

/// The emulator's project. `demo-` prefixed ids are the documented way to say
/// "this must never reach a real project": the Firebase tooling refuses to
/// contact production for one.
const PROJECT: &str = "demo-dbboard";

/// Where the emulator is listening, including the `/v1` suffix. Absent means
/// "no emulator was started", and the test skips rather than fails — an
/// ignored test that a developer opted into should still be honest about
/// having done nothing.
fn base_url() -> Option<String> {
    std::env::var("DBBOARD_TEST_FIRESTORE_EMULATOR")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn config(base: &str) -> BackendConfig {
    BackendConfig::Firestore(FirestoreConfig {
        project_id: PROJECT.to_string(),
        database_id: None,
        credentials: FirestoreCredentials::Emulator,
        base_url: Some(base.to_string()),
    })
}

/// Create one document, replacing any document already at that id.
///
/// The emulator accepts the fixed bearer token `owner` and checks nothing
/// else, so this needs no credential handling of its own.
async fn seed(base: &str, collection: &str, id: &str, fields: serde_json::Value) {
    let url = format!(
        "{base}/projects/{PROJECT}/databases/(default)/documents/{collection}?documentId={id}"
    );
    let response = reqwest::Client::new()
        .post(&url)
        .bearer_auth("owner")
        .json(&json!({ "fields": fields }))
        .send()
        .await
        .expect("the emulator did not answer — is it running?");
    // 409 is "already there", which is fine: the assertions below only need
    // the document to exist, not to have been created by this call.
    assert!(
        response.status().is_success() || response.status().as_u16() == 409,
        "seeding {collection}/{id} failed: {}",
        response.status()
    );
}

#[tokio::test]
#[ignore = "needs a running Firestore emulator; set DBBOARD_TEST_FIRESTORE_EMULATOR"]
async fn ping_reaches_the_emulator() {
    let Some(base) = base_url() else { return };
    let adapter = connect_adapter(config(&base))
        .await
        .expect("connect_adapter refused a Firestore emulator config");
    adapter.ping().await.expect("ping failed");
}

#[tokio::test]
#[ignore = "needs a running Firestore emulator; set DBBOARD_TEST_FIRESTORE_EMULATOR"]
async fn list_tables_reports_a_seeded_collection() {
    let Some(base) = base_url() else { return };
    seed(
        &base,
        "orders",
        "listed",
        json!({ "total": { "integerValue": "1" } }),
    )
    .await;

    let adapter = connect_adapter(config(&base)).await.expect("connect");
    let tables = adapter.list_tables().await.expect("list_tables failed");
    assert!(
        tables.iter().any(|t| t.name == "orders"),
        "orders is missing from {tables:?}"
    );
    // Firestore has no schema namespace, and reporting one would make the
    // sidebar render a folder that does not exist.
    assert!(
        tables.iter().all(|t| t.schema.is_none()),
        "a collection was reported with a schema namespace: {tables:?}"
    );
}

#[tokio::test]
#[ignore = "needs a running Firestore emulator; set DBBOARD_TEST_FIRESTORE_EMULATOR"]
async fn a_structured_query_returns_the_documents_it_asked_for() {
    let Some(base) = base_url() else { return };
    seed(
        &base,
        "customers",
        "queried",
        json!({
            "name": { "stringValue": "Ada" },
            "orders": { "integerValue": "3" },
            "address": { "mapValue": { "fields": {
                "city": { "stringValue": "London" }
            }}},
        }),
    )
    .await;

    let adapter = connect_adapter(config(&base)).await.expect("connect");
    // Exactly the text the sidebar generates for a browse (ADR-0094 §4), so a
    // change to `browseQuery` that the emulator would reject shows up here.
    let sql = "{\n  \"from\": [{ \"collectionId\": \"customers\" }],\n  \"limit\": 100\n}";
    let result = adapter.query(sql).await.expect("query failed");

    let name_at = result
        .columns
        .iter()
        .position(|c| c.name == "name")
        .expect("the `name` field is missing from the result columns");
    assert!(
        result
            .rows
            .iter()
            .any(|row| format!("{:?}", row.get(name_at)).contains("Ada")),
        "the seeded document is not in {:?}",
        result.rows
    );
    // A nested map has to survive as a document, not be flattened or dropped
    // — that is what `Value::Json` was added for (issue 0018).
    assert!(
        result.columns.iter().any(|c| c.name == "address"),
        "the nested `address` field was dropped: {:?}",
        result.columns
    );
}

#[tokio::test]
#[ignore = "needs a running Firestore emulator; set DBBOARD_TEST_FIRESTORE_EMULATOR"]
async fn describe_table_infers_fields_from_a_sample() {
    let Some(base) = base_url() else { return };
    seed(
        &base,
        "products",
        "described",
        json!({
            "sku": { "stringValue": "A-1" },
            "price": { "integerValue": "980" },
        }),
    )
    .await;

    let adapter = connect_adapter(config(&base)).await.expect("connect");
    let schema = adapter
        .describe_table(&TableInfo::unqualified("products"))
        .await
        .expect("describe_table failed");

    for want in ["sku", "price"] {
        assert!(
            schema.columns.iter().any(|c| c.name == want),
            "`{want}` is missing from {:?}",
            schema.columns
        );
    }
    // The document path is the only thing Firestore actually guarantees, so it
    // is what the adapter reports as the primary key.
    assert!(
        !schema.primary_key.is_empty(),
        "no primary key was reported for a Firestore collection"
    );
}

#[tokio::test]
#[ignore = "needs a running Firestore emulator; set DBBOARD_TEST_FIRESTORE_EMULATOR"]
async fn a_write_is_refused_rather_than_attempted() {
    let Some(base) = base_url() else { return };
    let adapter = connect_adapter(config(&base)).await.expect("connect");
    // Not SQL, and not a read: whatever this is, the adapter has no endpoint
    // that could carry it out. The point is that it fails here rather than
    // reaching the emulator and changing something.
    let err = adapter
        .execute(
            "{\"delete\": \"projects/demo-dbboard/databases/(default)/documents/orders/listed\"}",
        )
        .await
        .expect_err("execute was accepted on a read-only adapter");
    let rendered = format!("{err}");
    assert!(
        !rendered.is_empty(),
        "a refusal must say something a user can act on"
    );
}
