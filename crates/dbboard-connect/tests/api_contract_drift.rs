//! Keeps `docs/api-contract.md` honest about two things the wire actually
//! carries: the set of `Capabilities` flags, and the set of adapter ids.
//!
//! Both drifted before this test existed. `has_foreign_keys` (ADR-0054) was
//! serialized for months without appearing in the contract, and the `id` list
//! still named three adapters after nine had shipped. The contract is the
//! public API for `SemVer` purposes (ADR-0011), so a reader of the document —
//! including `dbboard-web`, which implements against it — was being told
//! something false about the payload.
//!
//! This crate is the home for both checks because it is the one that depends
//! on every concrete adapter *and* on `dbboard-core`.

use dbboard_connect::BackendConfig;
use dbboard_core::Capabilities;

/// The contract, read from the workspace rather than embedded, so the test
/// fails when the document changes underneath it.
fn contract() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/api-contract.md"
    ))
    .expect("docs/api-contract.md is readable from the workspace")
}

/// Every field name `Capabilities` puts on the wire.
///
/// Taken from a serialized value rather than a hand-written list: a flag
/// added to the struct shows up here with no test edit, which is the whole
/// point. `Default` is all-`false`, and the names are what matter.
fn wire_capability_flags() -> Vec<String> {
    let json = serde_json::to_value(Capabilities::default())
        .expect("Capabilities serializes to a JSON object");
    json.as_object()
        .expect("Capabilities is a flat object, per ADR-0012")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn contract_documents_every_capability_flag() {
    let doc = contract();
    let missing: Vec<_> = wire_capability_flags()
        .into_iter()
        .filter(|flag| !doc.contains(flag.as_str()))
        .collect();

    assert!(
        missing.is_empty(),
        "these Capabilities flags are on the wire but absent from \
         docs/api-contract.md: {missing:?}. A client reading the contract \
         cannot know they exist."
    );
}

/// The adapter id each backend puts in `GET /capabilities`.
///
/// The match is exhaustive on purpose. Adding a backend stops the build
/// here, which is the prompt to add its id to [`WIRE_IDS`] below and to the
/// `id` list in `docs/api-contract.md`.
fn wire_id(config: &BackendConfig) -> &'static str {
    match config {
        BackendConfig::Turso { .. } => "turso",
        BackendConfig::D1(_) => "d1",
        BackendConfig::Postgres { .. } => "postgres",
        BackendConfig::MySql { .. } => "mysql",
        BackendConfig::Neon { .. } => "neon",
        BackendConfig::Supabase { .. } => "supabase",
        // Both Aurora DSQL variants label themselves the same way; only the
        // way the token is obtained differs (ADR-0021 vs ADR-0036).
        BackendConfig::AuroraDsql { .. } | BackendConfig::AuroraDsqlIam { .. } => "aurora-dsql",
        BackendConfig::Firestore(_) => "firestore",
        BackendConfig::MongoDb(_) => "mongodb",
    }
}

/// Distinct ids reachable through [`wire_id`].
const WIRE_IDS: &[&str] = &[
    "turso",
    "d1",
    "postgres",
    "mysql",
    "neon",
    "supabase",
    "aurora-dsql",
    "firestore",
    "mongodb",
];

#[test]
fn wire_id_covers_the_documented_list() {
    // Keeps the exhaustive match live, so a new variant is a build failure
    // rather than a silently uncovered id.
    assert_eq!(wire_id(&BackendConfig::turso(":memory:")), "turso");

    let doc = contract();
    let missing: Vec<_> = WIRE_IDS
        .iter()
        .filter(|id| !doc.contains(&format!("`\"{id}\"`")))
        .copied()
        .collect();

    assert!(
        missing.is_empty(),
        "these adapter ids ship but are not listed in docs/api-contract.md: \
         {missing:?}. The `id` field is documented as adapter-stable, so an \
         unlisted value is a contract a client cannot implement against."
    );
}
