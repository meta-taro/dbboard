//! Every workspace crate that can reach the libSQL adapter must run its tests
//! one at a time.
//!
//! On Windows, tearing down two in-memory libSQL databases at the same instant
//! crashes the whole test binary with `STATUS_ACCESS_VIOLATION` (0xc0000005)
//! *after* every assertion has already passed. The verification recipe works
//! around it by serialising the affected crates, and for a long time it
//! serialised exactly one: `dbboard-turso`, the crate the crash was first seen
//! in. That is the wrong boundary. The hazard is not a crate name, it is
//! "opens libSQL in a test", which any crate inherits the moment it takes
//! `dbboard-turso` as a dependency — directly or through `dbboard-connect`.
//!
//! Four crates had inherited it without being serialised, and one of them
//! (`dbboard-mcp`, 17 in-memory databases) eventually failed a push on a green
//! branch. Nothing announced the gap, because the crash is rare enough to hide
//! for months: 0 failures in 30 deliberate reproduction runs here.
//!
//! Serialising is what makes it rare; it is not what makes it impossible. A
//! serialised `dbboard-mcp` run crashed on 2026-08-22 with every assertion
//! passed, so `scripts/cargo-test-serialised.sh` retries that one signature
//! once (ADR-0125). Being on the list is the reason a single retry is enough,
//! which is why the list still has to be right.
//!
//! So the list is checked rather than remembered. This test derives the set
//! from the dependency graph and fails when the recipe's list disagrees with
//! it, which is what happens the next time someone adds the adapter to a crate
//! that is not on the list yet.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// The crate whose teardown is the hazard. Reaching it — at any depth, through
/// normal or dev dependencies — is what puts a crate on the list.
const HAZARD: &str = "dbboard-turso";

/// The single source of truth. Kept as data rather than duplicated into each
/// caller, so there is one place to change and one place this test can check.
const LIST: &str = "scripts/libsql-serialised-crates.txt";

/// The one thing that reads the list and turns it into cargo invocations. The
/// hooks call this rather than carrying their own copy of the loop.
const RUNNER: &str = "scripts/cargo-test-serialised.sh";

/// Dev and build dependencies count: the crash happens in a *test* binary, and
/// `dbboard-mcp` reaches the adapter only through `[dev-dependencies]`.
const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn workspace_root() -> PathBuf {
    // crates/dbboard-turso -> crates -> <root>
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the workspace root")
        .to_path_buf()
}

fn read_manifest(path: &Path) -> toml::Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.parse::<toml::Value>()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Package name -> the names it depends on, for every workspace member.
///
/// Dependency *keys* are used as names. A renamed dependency
/// (`foo = { package = "bar" }`) would be recorded under `foo`; the workspace
/// renames nothing today, and a rename that hid the adapter would show up here
/// as a crate that stopped being on the list.
fn dependency_graph(root: &Path) -> HashMap<String, BTreeSet<String>> {
    let workspace = read_manifest(&root.join("Cargo.toml"));
    let members = workspace["workspace"]["members"]
        .as_array()
        .expect("[workspace] members is an array");

    let mut graph = HashMap::new();
    for member in members {
        let dir = member.as_str().expect("a member path is a string");
        let manifest = read_manifest(&root.join(dir).join("Cargo.toml"));
        let name = manifest["package"]["name"]
            .as_str()
            .expect("a member declares package.name")
            .to_owned();

        let mut deps = BTreeSet::new();
        for table in DEP_TABLES {
            if let Some(table) = manifest.get(table).and_then(toml::Value::as_table) {
                deps.extend(table.keys().cloned());
            }
        }
        graph.insert(name, deps);
    }
    graph
}

/// Members that can reach the adapter, including the adapter itself.
fn crates_linking_libsql(graph: &HashMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    graph
        .keys()
        .filter(|start| reaches(graph, start, HAZARD))
        .cloned()
        .collect()
}

fn reaches(graph: &HashMap<String, BTreeSet<String>>, start: &str, target: &str) -> bool {
    if start == target {
        return true;
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = VecDeque::from([start]);
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name) {
            continue;
        }
        let Some(deps) = graph.get(name) else {
            // A third-party dependency: it cannot reach a path-only crate.
            continue;
        };
        for dep in deps {
            if dep == target {
                return true;
            }
            queue.push_back(dep);
        }
    }
    false
}

fn declared_list(root: &Path) -> BTreeSet<String> {
    let path = root.join(LIST);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e}\n\
             This file is the list the pre-commit and pre-push hooks read to \
             decide which crates run with --test-threads=1.",
            path.display()
        )
    });
    text.lines()
        // The hooks strip CR too: the working tree is CRLF on Windows, and a
        // trailing CR would be passed to cargo as part of the package name.
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\r'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_crate_reaching_libsql_is_serialised() {
    let root = workspace_root();
    let expected = crates_linking_libsql(&dependency_graph(&root));
    let declared = declared_list(&root);

    let missing: Vec<_> = expected.difference(&declared).cloned().collect();
    let extra: Vec<_> = declared.difference(&expected).cloned().collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "{LIST} disagrees with the dependency graph.\n\
         reaches {HAZARD} but is not listed (add it): {missing:?}\n\
         listed but no longer reaches {HAZARD} (drop it): {extra:?}\n\
         Every listed crate runs with --test-threads=1; see this file's module \
         comment for why."
    );
}

/// Shell comments do not run. A hook that only *mentions* the runner in a
/// comment -- which is what the first version of this test accepted -- still
/// tests the workspace in parallel.
fn code_only(text: &str) -> String {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_list_is_wired_into_the_hooks() {
    // A list nothing reads is worse than no list: it looks like a control.
    // `cargo deny` sat red in this repo for months for exactly that reason
    // (ADR-0117), so the wiring is asserted rather than assumed.
    let root = workspace_root();

    let runner = root.join(RUNNER);
    let text = std::fs::read_to_string(&runner)
        .unwrap_or_else(|e| panic!("read {}: {e}", runner.display()));
    assert!(
        code_only(&text).contains(LIST),
        "{RUNNER} does not read {LIST}, so the list has stopped controlling \r
         anything it claims to control"
    );

    for hook in [
        ".cargo-husky/hooks/pre-commit",
        ".cargo-husky/hooks/pre-push",
    ] {
        let path = root.join(hook);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        assert!(
            code_only(&text).contains(RUNNER),
            "{hook} does not call {RUNNER}, so it runs the libSQL crates in \r
             parallel and will crash on Windows"
        );
    }
}
