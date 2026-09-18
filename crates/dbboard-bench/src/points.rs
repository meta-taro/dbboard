//! The catalogue of measurement points.
//!
//! Declared as data, and separately from the code that measures them, for one
//! reason: a test can read this list without running a single benchmark. That
//! is what lets `docs/performance-baseline.md` be checked rather than trusted
//! — the same "checked, not remembered" shape as `toolchain_pin_drift.rs` and
//! `release-plan.test.mjs`.
//!
//! The numbers in that document are not asserted anywhere. Machine-to-machine
//! variance is larger than most of what would be worth catching, and a
//! threshold on a CI runner's timings is a flaky test waiting to happen
//! (ADR-0141). What *is* asserted is that the set of points here and the set
//! in the document are the same set, so a measurement cannot quietly vanish.

/// Which of the three areas `docs/roadmap.md` reserved the v0.14 slot for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Work the desktop shell does before a window appears.
    Startup,
    /// Opening a database and reading enough of it to show a table.
    Browse,
    /// Carrying a full result set to the frontend.
    ResultSets,
}

impl Group {
    /// Every group, in the order the roadmap names them and the order the
    /// baseline document renders them.
    pub const ALL: [Self; 3] = [Self::Startup, Self::Browse, Self::ResultSets];

    /// The `id` prefix every point in this group carries.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Browse => "browse",
            Self::ResultSets => "result",
        }
    }

    /// The heading this group gets in the baseline document.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Browse => "Connect and browse",
            Self::ResultSets => "Large result sets",
        }
    }
}

/// One thing that gets timed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    /// Stable identifier, `<group slug>/<what>`. It appears verbatim in the
    /// baseline document, so renaming one is a visible change to that file.
    pub id: &'static str,
    pub group: Group,
    /// What is inside the timed region, in one line. Written for someone
    /// reading the number a year from now who has not read the code.
    pub what: &'static str,
}

/// Every measurement point, grouped and in render order.
///
/// Sizes are baked into the identifiers (`_20`, `_10k`) rather than left as
/// parameters. A median for "some number of tables" is not comparable with
/// anything; a median for twenty is.
pub const POINTS: &[Point] = &[
    // ---- Startup ---------------------------------------------------------
    //
    // `run()` in apps/desktop/src-tauri/src/lib.rs does all of this before
    // the window exists. Measured against synthetic fixtures in a temporary
    // directory, never the operator's real config: the real file's size is a
    // property of one machine, and reading it here would put its contents in
    // reach of a tool that prints things.
    Point {
        id: "startup/config_paths",
        group: Group::Startup,
        what: "Resolve the platform config directory (`dbboard_config::default_path`)",
    },
    Point {
        id: "startup/connections_open_20",
        group: Group::Startup,
        what: "Open and parse a `connections.toml` holding 20 connections",
    },
    Point {
        id: "startup/annotations_open_20",
        group: Group::Startup,
        what: "Open and parse an `annotations.toml` holding 20 annotated connections",
    },
    // ---- Connect and browse ---------------------------------------------
    //
    // In-memory libSQL, so these are adapter and schema-handling costs with
    // no network in them. A hosted database's numbers would be a measurement
    // of somebody's link, and would not reproduce.
    Point {
        id: "browse/connect_memory",
        group: Group::Browse,
        what: "`TursoAdapter::connect_local(\":memory:\")`, including the read-only probe",
    },
    Point {
        id: "browse/list_tables_20",
        group: Group::Browse,
        what: "`list_tables` against a schema of 20 tables",
    },
    Point {
        id: "browse/describe_table_12col",
        group: Group::Browse,
        what: "`describe_table` for one 12-column table",
    },
    Point {
        id: "browse/foreign_keys",
        group: Group::Browse,
        what: "`foreign_keys` for one table with 2 outgoing references",
    },
    Point {
        id: "browse/first_page_100",
        group: Group::Browse,
        what: "The first page the grid asks for: `SELECT * … LIMIT 100`",
    },
    Point {
        id: "browse/next_page_100",
        group: Group::Browse,
        what: "The *last* page of the same table, reached by keyset cursor — \
               the point of the choice is that it matches `first_page_100` \
               rather than drifting from it (ADR-0145)",
    },
    // ---- Large result sets ----------------------------------------------
    //
    // `MAX_RESULT_ROWS` is 10,000, so that is the largest result the UI can
    // ever be handed. Every one of these is on the path between a query
    // finishing and rows appearing.
    Point {
        id: "result/query_10k",
        group: Group::ResultSets,
        what: "Materialise 10,000 rows × 8 columns out of the driver into `QueryResult`",
    },
    Point {
        id: "result/serialize_10k",
        group: Group::ResultSets,
        what: "`serde_json` the same `QueryResult` — what crossing the Tauri IPC boundary costs",
    },
    Point {
        id: "result/sort_10k",
        group: Group::ResultSets,
        what: "`sorted_row_order` over 10,000 rows on one text column",
    },
    Point {
        id: "result/truncate_10k_to_100",
        group: Group::ResultSets,
        what: "`truncate_rows` from 10,000 down to 100 (the read-only path's soft cap)",
    },
];

/// Look one up by identifier.
#[must_use]
pub fn point(id: &str) -> Option<&'static Point> {
    POINTS.iter().find(|p| p.id == id)
}

/// The points belonging to one group, in catalogue order.
#[must_use]
pub fn in_group(group: Group) -> Vec<&'static Point> {
    POINTS.iter().filter(|p| p.group == group).collect()
}

#[cfg(test)]
mod tests {
    use super::{in_group, point, Group, POINTS};
    use std::collections::BTreeSet;

    #[test]
    fn identifiers_are_unique() {
        let ids: BTreeSet<&str> = POINTS.iter().map(|p| p.id).collect();
        assert_eq!(
            ids.len(),
            POINTS.len(),
            "two measurement points share an id, so one would overwrite the other \
             in the baseline document"
        );
    }

    #[test]
    fn every_identifier_is_prefixed_by_its_group() {
        for p in POINTS {
            let prefix = format!("{}/", p.group.slug());
            assert!(
                p.id.starts_with(&prefix),
                "{} is in group {:?} but is not prefixed {prefix}",
                p.id,
                p.group
            );
        }
    }

    #[test]
    fn every_group_is_measured() {
        // The roadmap slot names three areas. A group that quietly lost all
        // its points would still render a heading, with nothing under it.
        for group in Group::ALL {
            assert!(
                !in_group(group).is_empty(),
                "{group:?} has no measurement points"
            );
        }
    }

    #[test]
    fn every_point_says_what_it_times() {
        for p in POINTS {
            assert!(
                !p.what.trim().is_empty(),
                "{} does not say what is inside the timed region",
                p.id
            );
        }
    }

    #[test]
    fn lookup_finds_a_known_point_and_misses_an_unknown_one() {
        assert_eq!(
            point("result/serialize_10k").map(|p| p.id),
            Some("result/serialize_10k")
        );
        assert_eq!(point("result/no_such_thing"), None);
    }

    #[test]
    fn in_group_preserves_catalogue_order() {
        let startup: Vec<&str> = in_group(Group::Startup).iter().map(|p| p.id).collect();
        assert_eq!(
            startup,
            vec![
                "startup/config_paths",
                "startup/connections_open_20",
                "startup/annotations_open_20",
            ]
        );
    }
}
