//! The identity marks a connection can carry: a colour and a short tag.
//!
//! A closed, named set rather than a colour picker (issue #192, plan
//! `.claude/issues/0026`). Three reasons, in the order they mattered:
//!
//! - A name is what makes the mark usable when the colour cannot be seen —
//!   said out loud, read by a screen reader, written in a note. A hex string
//!   is none of those.
//! - Each colour needs a light and a dark value, because a hue tuned for a
//!   white row goes muddy on a dark one. Storing the name lets the theme pick;
//!   storing `#1a73e8` freezes one theme into the config file.
//! - Eight is enough to tell production from a copy at a glance, and few
//!   enough that every one of them stays distinguishable from the others.
//!
//! The values themselves are not here — they are CSS custom properties in
//! `apps/desktop/src/lib/styles/tokens.css`, listed in `DESIGN.md`. This crate
//! only decides which names are legal, because it is the layer that writes the
//! file.

//!
//! The tag is the other half, and the half that survives a greyscale
//! screenshot or a colour-blind reader: a few characters the operator writes
//! themselves — `prod`, `本番`, `staging`. Colour alone was ruled out as a mark
//! for exactly that reason (plan `.claude/issues/0026`, section D). It is free
//! text rather than another closed set because the words a team uses for its
//! own servers are not ones this crate can enumerate.

/// Every colour a connection may be marked with, in palette order.
///
/// Order is the order the picker shows them in: it is a spectrum, so the
/// operator can find "the green one" without reading eight labels.
pub const CONNECTION_COLORS: [&str; 8] = [
    "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
];

/// Whether `name` is one of [`CONNECTION_COLORS`].
///
/// Case-sensitive: the stored value is one of these literals, not a
/// human-typed word, and accepting `"Blue"` here would put a value in the file
/// that no stylesheet has a token for.
#[must_use]
pub fn is_connection_color(name: &str) -> bool {
    CONNECTION_COLORS.contains(&name)
}

/// How many characters a tag may carry.
///
/// A row is one line, and the connection list already loses the *name* first
/// when something else takes the width (issue #192, section C). An unbounded
/// tag would make that worse in the place it is meant to help. Twelve fits
/// every word a server is actually called, in either script.
pub const CONNECTION_TAG_MAX_CHARS: usize = 12;

/// Whether `tag` is short enough to sit on a connection row.
///
/// Counts **characters, not bytes**: twelve kanji are thirty-six bytes and take
/// less width on the row than twelve latin letters, so a byte limit would
/// reject the shorter label of the two.
///
/// A blank tag passes. Blank means "no tag", which the caller resolves to
/// `None` before storing; rejecting it here would make the two layers disagree
/// about what an empty box means.
#[must_use]
pub fn is_connection_tag(tag: &str) -> bool {
    tag.chars().count() <= CONNECTION_TAG_MAX_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_has_no_duplicates() {
        let mut seen = CONNECTION_COLORS.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            CONNECTION_COLORS.len(),
            "two entries of the palette would be indistinguishable in the picker"
        );
    }

    #[test]
    fn known_names_are_accepted() {
        for name in CONNECTION_COLORS {
            assert!(is_connection_color(name), "{name} is in the palette");
        }
    }

    #[test]
    fn anything_else_is_not_a_colour() {
        for name in ["", " ", "Blue", "#1a73e8", "chartreuse"] {
            assert!(
                !is_connection_color(name),
                "{name:?} has no token in any stylesheet"
            );
        }
    }
    #[test]
    fn a_tag_within_the_limit_is_accepted() {
        // The three the operator actually types, in both scripts.
        assert!(is_connection_tag("prod"));
        assert!(is_connection_tag("本番"));
        assert!(is_connection_tag("staging"));
    }

    #[test]
    fn a_tag_is_measured_in_characters_not_bytes() {
        // Twelve kanji are 36 bytes. Measuring bytes would reject a tag that
        // takes less room on the row than the English one beside it.
        let twelve = "本".repeat(12);
        assert_eq!(twelve.len(), 36);
        assert!(is_connection_tag(&twelve));
        assert!(!is_connection_tag(&"本".repeat(13)));
    }

    #[test]
    fn a_blank_tag_is_not_the_validators_problem() {
        // Blank means "no tag", which the normaliser turns into `None` before
        // this is ever asked. Accepting it here keeps the two from disagreeing.
        assert!(is_connection_tag(""));
    }
}
