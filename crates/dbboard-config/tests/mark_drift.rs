//! Keeps [`dbboard_config::CONNECTION_COLORS`] and the frontend's
//! `CONNECTION_COLORS` agreeing on which identity colours dbboard ships, and
//! keeps every one of them backed by a value the app can actually paint.
//!
//! The list exists twice for the same reason the locale list does (see
//! `locale_drift.rs`): the frontend owns what the colour looks like, while this
//! crate has to reject a name no build can render — the config file is written
//! by callers that never see the TypeScript. Duplication is only safe if
//! separating the two lists breaks the build, which is what this test is.
//!
//! The third test goes one step further than the locale pair has to: a colour
//! is a name *and* a value, and a name whose `--conn-` token was never defined
//! paints nothing at all. That failure is invisible on a screenshot of a light
//! theme if the token exists there and only the dark block was missed, so it is
//! checked per theme block rather than once.

use dbboard_config::{is_connection_color, CONNECTION_COLORS, CONNECTION_TAG_MAX_CHARS};

/// The frontend's palette, read from the workspace.
fn marks_ts() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/src/lib/connections/marks.ts"
    ))
    .expect("apps/desktop/src/lib/connections/marks.ts is readable from the workspace")
}

/// The theme tokens, read from the workspace.
fn tokens_css() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/src/lib/styles/tokens.css"
    ))
    .expect("apps/desktop/src/lib/styles/tokens.css is readable from the workspace")
}

/// Every entry of the frontend's `CONNECTION_COLORS` array, in source order.
///
/// A deliberately small parser rather than a regex dependency, matching
/// `locale_drift.rs`: the array is hand-maintained and one name per line, and
/// anything that breaks that shape should fail loudly here rather than be
/// silently tolerated.
fn frontend_colors(source: &str) -> Vec<String> {
    let body = source
        .split_once("export const CONNECTION_COLORS")
        .expect("marks.ts declares CONNECTION_COLORS")
        .1
        .split_once(']')
        .expect("the CONNECTION_COLORS array is terminated")
        .0;

    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let quote = line.chars().next()?;
            if quote != '\'' && quote != '"' {
                return None;
            }
            Some(
                line[1..]
                    .split(quote)
                    .next()
                    .expect("the colour literal is closed")
                    .to_string(),
            )
        })
        .collect()
}

#[test]
fn the_rust_and_frontend_palettes_are_identical() {
    let names = frontend_colors(&marks_ts());

    assert_eq!(
        names,
        CONNECTION_COLORS.to_vec(),
        "dbboard_config::CONNECTION_COLORS and \
         apps/desktop/src/lib/connections/marks.ts disagree. Order matters \
         too: both are the order the picker offers, and it is the spectrum \
         rather than the alphabet. A name in only one of them either cannot be \
         saved (frontend-only) or can be saved but never rendered (Rust-only)."
    );
}

#[test]
fn every_frontend_color_is_accepted_by_the_validator() {
    // The list agreeing is not quite the same as the validator agreeing —
    // `is_connection_color` is what actually gates a write, and it is exact and
    // case-sensitive, so a casing slip in either file lands here.
    for name in frontend_colors(&marks_ts()) {
        assert!(
            is_connection_color(&name),
            "the picker offers {name}, but is_connection_color rejects it, \
             so choosing it could not be saved"
        );
    }
}

#[test]
fn every_color_has_a_value_in_every_theme() {
    let css = tokens_css();

    // The blocks a colour has to survive. A token defined only in `:root` looks
    // right until someone switches theme, and the explicit `[data-theme]`
    // blocks are what the in-app switcher sets — the media query alone would
    // leave a manual override painting the light value on a dark surface.
    let blocks = [
        (":root {", "the default (light) palette"),
        (
            "@media (prefers-color-scheme: dark)",
            "the system-dark palette",
        ),
        (":root[data-theme='light']", "the forced-light palette"),
        (":root[data-theme='dark']", "the forced-dark palette"),
    ];

    for (marker, what) in blocks {
        let start = css
            .find(marker)
            .unwrap_or_else(|| panic!("tokens.css has no {marker} block, so {what} is missing"));
        let body = &css[start..];
        let body = body.split_once("\n}").map_or(body, |(head, _)| head);

        for name in CONNECTION_COLORS {
            assert!(
                body.contains(&format!("--conn-{name}:")),
                "{what} defines no --conn-{name}, so a connection marked \
                 {name} would paint an empty swatch there"
            );
        }
    }
}

#[test]
fn the_two_sides_agree_on_how_long_a_tag_may_be() {
    // The form stops the operator at its own number; this crate rejects past
    // its own. A frontend limit that is higher silently truncates on save; one
    // that is lower makes a tag the backend would have taken untypeable.
    let source = marks_ts();
    let literal = source
        .split_once("export const CONNECTION_TAG_MAX_CHARS")
        .expect("marks.ts declares CONNECTION_TAG_MAX_CHARS")
        .1
        .split_once('=')
        .expect("CONNECTION_TAG_MAX_CHARS is assigned")
        .1
        .split_once(';')
        .expect("the assignment is terminated")
        .0
        .trim()
        .to_string();

    assert_eq!(
        literal.parse::<usize>().expect("a plain integer"),
        CONNECTION_TAG_MAX_CHARS,
        "marks.ts and dbboard_config::CONNECTION_TAG_MAX_CHARS disagree"
    );
}
