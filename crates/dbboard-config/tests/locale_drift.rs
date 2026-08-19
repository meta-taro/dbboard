//! Keeps [`dbboard_config::SUPPORTED_LOCALES`] and the frontend's `LOCALES`
//! agreeing on which languages dbboard ships.
//!
//! The list exists twice because the two sides cannot see each other: the
//! frontend owns the translations, while this crate has to be able to reject a
//! code no build can display — for callers that are not the frontend (the MCP
//! server writes `locale` into `ui-settings.toml` without any access to the
//! TypeScript). Duplication is only safe if separating the two lists breaks the
//! build, which is what this test is.
//!
//! Reading the TypeScript from the workspace rather than embedding a copy is
//! deliberate, and the same shape as `dbboard-connect`'s
//! `api_contract_drift.rs`: the test fails when the other file changes
//! underneath it, which is the only moment the mismatch is cheap to fix.

use dbboard_config::{is_supported_locale, SUPPORTED_LOCALES};

/// The frontend's locale table, read from the workspace.
fn locales_ts() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/src/lib/i18n/locales.ts"
    ))
    .expect("apps/desktop/src/lib/i18n/locales.ts is readable from the workspace")
}

/// Every `code:` in the `LOCALES` array, in source order.
///
/// A deliberately small parser rather than a regex dependency: the array is
/// hand-maintained and one entry per line, and anything that breaks that shape
/// should fail loudly here rather than be silently tolerated.
fn frontend_codes(source: &str) -> Vec<String> {
    let body = source
        .split_once("export const LOCALES")
        .expect("locales.ts declares LOCALES")
        .1
        .split_once("];")
        .expect("the LOCALES array is terminated")
        .0;

    body.lines()
        .filter_map(|line| line.split_once("code:"))
        .map(|(_, rest)| {
            let rest = rest.trim_start();
            let quote = rest.chars().next().expect("a code value follows `code:`");
            assert!(
                quote == '\'' || quote == '"',
                "expected a quoted locale code, got: {rest}"
            );
            rest[1..]
                .split(quote)
                .next()
                .expect("the code literal is closed")
                .to_string()
        })
        .collect()
}

#[test]
fn the_rust_and_frontend_locale_lists_are_identical() {
    let codes = frontend_codes(&locales_ts());

    assert_eq!(
        codes,
        SUPPORTED_LOCALES.to_vec(),
        "dbboard_config::SUPPORTED_LOCALES and apps/desktop/src/lib/i18n/locales.ts \
         disagree. Order matters too: both are the order the switcher shows. \
         A code in only one of them either cannot be persisted (frontend-only) \
         or can be persisted but never displayed (Rust-only)."
    );
}

#[test]
fn every_frontend_code_is_accepted_by_the_validator() {
    // The list agreeing is not quite the same as the validator agreeing —
    // `is_supported_locale` is what actually gates a write, and it is exact
    // and case-sensitive, so a casing slip in either file lands here.
    for code in frontend_codes(&locales_ts()) {
        assert!(
            is_supported_locale(&code),
            "the switcher offers {code}, but is_supported_locale rejects it, \
             so choosing it could not be persisted"
        );
    }
}

#[test]
fn the_frontend_default_locale_is_one_this_crate_accepts() {
    let source = locales_ts();
    let default = source
        .split_once("export const DEFAULT_LOCALE")
        .expect("locales.ts declares DEFAULT_LOCALE")
        .1
        .split_once('\'')
        .expect("DEFAULT_LOCALE is a single-quoted string")
        .1
        .split_once('\'')
        .expect("DEFAULT_LOCALE is closed")
        .0
        .to_string();

    assert!(
        is_supported_locale(&default),
        "DEFAULT_LOCALE is {default}, which this crate would refuse to persist"
    );
}
