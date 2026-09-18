//! The dependency audit runs nightly, and only the dependency audit does.
//!
//! `cargo deny check` is the one job in `ci.yml` that goes red **without a
//! commit**: a crate nobody here names is yanked upstream, the advisory
//! database learns about it, and every branch fails at once. It happened twice
//! in eight days — `chacha20 0.10.1` during v0.14.0 and `wnaf 0.14.0` the week
//! after — and both times it was found because somebody happened to open a
//! pull request, not because anything watched.
//!
//! A schedule is the cheap half of the answer: the failure arrives as a
//! notification within a day instead of waiting for the next push. It does not
//! fix the lockfile — a person still does that — but it decides *when* the
//! repo finds out.
//!
//! Only `deps` runs on the schedule. A nightly that also compiled the
//! workspace and the frontend would spend runner minutes re-answering a
//! question no code change had asked, and the noise is what makes a nightly
//! stop being read. So this test pins both halves: that the schedule exists,
//! and that every other job is excluded from it.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the workspace root")
        .to_path_buf()
}

fn ci_yaml() -> String {
    let path = workspace_root().join(".github/workflows/ci.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The job names declared at the top level of `jobs:`, in file order.
///
/// Job keys are the only lines indented exactly two spaces and ending in a
/// colon, which is enough structure to read without a YAML parser — this
/// crate has no dependency on one, and adding it to assert five job names
/// would cost more than the assertion is worth.
fn job_names(yaml: &str) -> Vec<String> {
    let body = yaml
        .split("\njobs:\n")
        .nth(1)
        .expect("ci.yml declares jobs");
    body.lines()
        .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
        .filter_map(|l| l.trim_end().strip_suffix(':'))
        .map(|n| n.trim().to_string())
        .collect()
}

/// Everything under one job key, up to the next job key.
fn job_block(yaml: &str, name: &str) -> String {
    let body = yaml
        .split("\njobs:\n")
        .nth(1)
        .expect("ci.yml declares jobs");
    let start = body
        .find(&format!("  {name}:\n"))
        .unwrap_or_else(|| panic!("ci.yml has no `{name}` job"));
    let rest = &body[start..];
    let mut out = String::new();
    for (i, line) in rest.lines().enumerate() {
        let is_next_job = i > 0
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':');
        if is_next_job {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn ci_runs_on_a_schedule() {
    let yaml = ci_yaml();
    assert!(
        yaml.contains("  schedule:\n    - cron:"),
        "ci.yml has no schedule trigger — the deps job would only ever run when \
         somebody pushed, which is how two upstream yanks went unnoticed"
    );
}

#[test]
fn the_deps_job_is_the_one_the_schedule_is_for() {
    let yaml = ci_yaml();
    let deps = job_block(&yaml, "deps");
    assert!(
        !deps.contains("event_name != 'schedule'"),
        "the deps job excludes itself from the schedule, which leaves the \
         nightly run with nothing to do"
    );
}

#[test]
fn every_other_job_sits_the_nightly_out() {
    let yaml = ci_yaml();
    for name in job_names(&yaml) {
        if name == "deps" {
            continue;
        }
        let block = job_block(&yaml, &name);
        assert!(
            block.contains("event_name != 'schedule'"),
            "job `{name}` would also run nightly. Only the dependency audit \
             answers a question that changes without a commit; the rest would \
             re-run a build nothing had touched, and a noisy nightly is an \
             unread one."
        );
    }
}
