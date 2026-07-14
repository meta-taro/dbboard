//! Guards the shipped collector setup template against schema drift.
//!
//! `docs/collector-setup/connections.template.toml` is handed to whoever
//! sets up the data-collection machine (see that folder's README). If a
//! future schema change silently broke the template, the collector would
//! only find out at launch. Compiling the real file in with `include_str!`
//! and parsing it through the production `ConnectionFile::parse` keeps the
//! template honest at `cargo test` time instead.

use dbboard_config::{ConnectionFile, ConnectionKind};

/// The exact template bytes shipped in the repo. Path is relative to this
/// test file: `crates/dbboard-config/tests/` → repo root is three up.
const TEMPLATE: &str = include_str!("../../../docs/collector-setup/connections.template.toml");

#[test]
fn collector_template_parses_against_the_current_schema() {
    let file = ConnectionFile::parse(TEMPLATE)
        .expect("shipped collector template must parse against the current schema");
    assert_eq!(file.version, dbboard_config::CONFIG_VERSION);
}

#[test]
fn collector_template_defines_the_three_expected_connections() {
    let file = ConnectionFile::parse(TEMPLATE).expect("template parses");

    // The three real collector connections, in file order. The ids are
    // load-bearing: the README seeding commands reference them verbatim.
    let by_id: Vec<(&str, &ConnectionKind)> = file
        .connections
        .iter()
        .map(|c| (c.id.as_str(), &c.kind))
        .collect();

    assert_eq!(
        by_id.len(),
        3,
        "template must define exactly three connections"
    );

    assert!(matches!(
        by_id[0],
        ("store-cabaret", ConnectionKind::D1 { .. })
    ));
    assert!(matches!(
        by_id[1],
        ("store-lovehotel", ConnectionKind::AuroraDsqlIam { .. })
    ));
    assert!(matches!(
        by_id[2],
        ("vegas-gift", ConnectionKind::Supabase { .. })
    ));
}

/// The template must reference secrets, never embed them: the keyring
/// reference keys are present, and no raw secret-value key ever appears.
#[test]
fn collector_template_carries_no_secret_material() {
    for reference_key in [
        "keyring_token_ref",      // store-cabaret (D1 API token)
        "keyring_secret_key_ref", // store-lovehotel (AWS secret key)
        "keyring_url_ref",        // vegas-gift (Supabase URL)
    ] {
        assert!(
            TEMPLATE.contains(reference_key),
            "template must reference the {reference_key} secret slot"
        );
    }
    for forbidden in ["password =", "secret =", "token =", "secret_key ="] {
        assert!(
            !TEMPLATE.contains(forbidden),
            "template must not embed a `{forbidden}` value field"
        );
    }
}
