//! What dbboard is, and what each version changed (ADR-0154).
//!
//! An agent that has just been handed a `dbboard-mcp` process knows the tool
//! names and nothing else: not what the product is for, not which engines it
//! speaks, not what the build in front of it gained or lost. This module
//! answers that in one call.
//!
//! **It does not parse the changelog's bullets.** Two readers of `CHANGELOG.md`
//! already exist — the About dialog's parser (ADR-0137) and
//! `scripts/release-notes.mjs` — and that file's own comment says two readers
//! of one file must not disagree about what a bullet says. A third would be a
//! third chance to disagree. So the release *headings* are scanned, because
//! their shape is fixed and test-enforced (`release-plan.test.mjs`), and the
//! body of a version is handed over as the markdown it already is. The reader
//! here is a language model; markdown is structure it can use.

use serde::Serialize;

/// The shipped changelog, compiled in so the answer always describes the
/// build that is answering rather than whatever is on disk beside it.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// The one-paragraph version of what this is. The long form lives in the
/// README and the download page, both linked below — this stays short
/// precisely so it does not become a second README to keep in step.
const WHAT_IT_IS: &str = "A local-first desktop client for several databases at once: \
libSQL/Turso, Cloudflare D1, Postgres (including Neon, Supabase and Aurora DSQL), MySQL, \
Firestore and MongoDB. Connections, credentials and notes live on the operator's machine; \
nothing is hosted. This MCP server is the read-only surface an agent gets: it can look at \
schemas and run read-only queries, while writing, dumping and restoring stay in the hands \
of whoever is sitting at the app.";

/// One release, as its heading states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseHeading {
    pub version: String,
    /// `None` for `[Unreleased]`, which has no date yet.
    pub date: Option<String>,
    /// The slot's headline — what the release was reserved to carry.
    pub headline: Option<String>,
}

/// The answer to "what is this, and what changed".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct About {
    pub name: &'static str,
    pub what_it_is: &'static str,
    /// The version of the running server, from its own package metadata.
    pub version: &'static str,
    pub download_page: &'static str,
    pub repository: &'static str,
    /// Every connection kind this build can speak, as `list_connections`
    /// reports them.
    pub adapter_kinds: Vec<&'static str>,
    /// Every release the shipped changelog describes, newest first.
    pub releases: Vec<ReleaseHeading>,
    /// The version whose changes are included below.
    pub changes_for: Option<String>,
    /// That version's section of the changelog, verbatim markdown.
    pub changes: Option<String>,
}

/// Adapter kinds, kept in the order `list_connections` documents them.
const ADAPTER_KINDS: &[&str] = &[
    "turso",
    "turso-remote",
    "d1",
    "postgres",
    "mysql",
    "neon",
    "supabase",
    "aurora-dsql",
    "aurora-dsql-iam",
    "firestore",
    "mongodb",
];

/// Split a `## [version] — date — headline` heading into its parts.
///
/// The em dash is the separator the release tooling writes and
/// `release-plan.test.mjs` enforces, so it is the only one accepted — every
/// heading in the file uses it, back to 0.1.0. A hyphen would also match the
/// ones inside a date and inside a headline, which is how a lenient parser
/// starts reporting `2026` as a version's date.
fn parse_heading(line: &str) -> Option<ReleaseHeading> {
    let rest = line.strip_prefix("## [")?;
    let (version, rest) = rest.split_once(']')?;
    let parts: Vec<&str> = rest
        .split('\u{2014}')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    // A dated release leads with its date; `[Unreleased]` leads with its
    // headline, because it has no date to lead with.
    let dated = parts
        .first()
        .is_some_and(|p| p.starts_with(|c: char| c.is_ascii_digit()));
    let (date, headline) = if dated {
        (parts.first().copied(), parts.get(1).copied())
    } else {
        (None, parts.first().copied())
    };

    Some(ReleaseHeading {
        version: version.to_owned(),
        date: date.map(str::to_owned),
        headline: headline.map(str::to_owned),
    })
}

/// Every release heading in the shipped changelog, newest first.
#[must_use]
pub fn releases() -> Vec<ReleaseHeading> {
    CHANGELOG.lines().filter_map(parse_heading).collect()
}

/// One version's section of the changelog, verbatim, heading included.
///
/// `None` when no such version ships in this build — which is the honest
/// answer for a version that is newer than the running one, and the reason
/// the caller gets the list as well.
#[must_use]
pub fn changes_for(version: &str) -> Option<String> {
    let mut out: Option<Vec<&str>> = None;
    for line in CHANGELOG.lines() {
        if let Some(heading) = parse_heading(line) {
            if out.is_some() {
                break;
            }
            if heading.version == version {
                out = Some(vec![line]);
            }
            continue;
        }
        if let Some(section) = out.as_mut() {
            section.push(line);
        }
    }
    out.map(|lines| lines.join("\n").trim_end().to_owned())
}

/// Assemble the answer. `version` defaults to the running build's.
#[must_use]
pub fn about(version: Option<&str>) -> About {
    let running = env!("CARGO_PKG_VERSION");
    let wanted = version.unwrap_or(running);
    let changes = changes_for(wanted);
    About {
        name: "dbboard",
        what_it_is: WHAT_IT_IS,
        version: running,
        download_page: "https://meta-taro.github.io/dbboard/",
        repository: "https://github.com/meta-taro/dbboard",
        adapter_kinds: ADAPTER_KINDS.to_vec(),
        releases: releases(),
        changes_for: changes.as_ref().map(|_| wanted.to_owned()),
        changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_changelog_parses_into_releases() {
        let found = releases();
        assert!(
            found.len() > 5,
            "the changelog should carry every release: {found:?}"
        );
        assert!(found.iter().any(|r| r.version == "0.16.1"));
    }

    #[test]
    fn a_heading_carries_its_date_and_headline() {
        let heading =
            parse_heading("## [0.16.1] — 2026-09-08 — What 0.16.0 showed on screen").unwrap();
        assert_eq!(heading.version, "0.16.1");
        assert_eq!(heading.date.as_deref(), Some("2026-09-08"));
        assert_eq!(
            heading.headline.as_deref(),
            Some("What 0.16.0 showed on screen")
        );
    }

    #[test]
    fn unreleased_has_no_date() {
        // It is a real heading and belongs in the list — an agent asking what
        // is coming should see it — but dating it would be a guess.
        let heading = parse_heading("## [Unreleased] — The half that was deferred").unwrap();
        assert_eq!(heading.version, "Unreleased");
        assert_eq!(heading.date, None);
        assert_eq!(
            heading.headline.as_deref(),
            Some("The half that was deferred")
        );
    }

    #[test]
    fn a_line_that_is_not_a_heading_is_not_a_release() {
        assert!(parse_heading("### Added").is_none());
        assert!(parse_heading("- **Something.** prose").is_none());
        assert!(parse_heading("## Not a version").is_none());
    }

    #[test]
    fn changes_stop_at_the_next_release() {
        let section = changes_for("0.16.1").expect("0.16.1 ships in this build");
        assert!(section.starts_with("## [0.16.1]"));
        assert!(
            !section.contains("## [0.16.0]"),
            "a version's section must not swallow the one below it"
        );
    }

    #[test]
    fn a_version_this_build_does_not_carry_has_no_changes() {
        // Honest for a version newer than the running one — and why the
        // caller also gets the list of versions that do ship.
        assert!(changes_for("99.0.0").is_none());
    }

    #[test]
    fn about_defaults_to_the_running_build() {
        let answer = about(None);
        assert_eq!(answer.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(answer.changes_for.as_deref(), Some(answer.version));
        assert!(answer.adapter_kinds.contains(&"postgres"));
    }
}
