//! The toolchain the mandatory commands run under is pinned, and stays pinned.
//!
//! `cargo clippy -- -D warnings` treats every new lint as a build failure, so
//! the set of lints is part of the build. Without a pin that set is whatever
//! the GitHub runner image happens to ship: on 2026-08-24 the image moved to
//! Rust 1.98, `clippy::unused_async_trait_impl` arrived with it, and `develop`
//! went red on a commit that had not touched the offending file since June.
//! Nobody changed anything, and the branch broke anyway.
//!
//! Pinning does not avoid the work — the same lint still has to be answered —
//! it decides *when*. A version bump is a commit someone chose to make, with
//! the lint fixes in the same diff, instead of an unrelated push discovering
//! them.
//!
//! So the pin has to be an exact version. `channel = "stable"` is a file that
//! looks like a pin and floats anyway, which is worse than not having one: it
//! answers the question for the next reader without being true.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // crates/dbboard-config -> crates -> <root>
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the workspace root")
        .to_path_buf()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The bare value of `key = "..."`, from the first line that assigns it.
fn string_value(toml: &str, key: &str) -> Option<String> {
    toml.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim_start().strip_prefix('='))
        .map(|rest| rest.trim().trim_matches('"').to_string())
}

fn version_parts(v: &str) -> Vec<u32> {
    v.split('.').filter_map(|p| p.parse().ok()).collect()
}

#[test]
fn the_toolchain_is_pinned_to_an_exact_version() {
    let path = workspace_root().join("rust-toolchain.toml");
    assert!(
        path.is_file(),
        "rust-toolchain.toml is missing: the mandatory `-D warnings` commands \
         would run under whatever version the machine happens to have"
    );

    let channel =
        string_value(&read(&path), "channel").expect("rust-toolchain.toml declares a `channel`");

    // Three components, all numeric. "stable" / "1.98" / "nightly-2026-08-01"
    // all fail here, and each of them floats in a different way.
    let parts = version_parts(&channel);
    assert_eq!(
        parts.len(),
        3,
        "channel must be an exact x.y.z version, got {channel:?}"
    );
}

#[test]
fn the_pin_carries_the_components_the_mandatory_commands_need() {
    let toolchain = read(&workspace_root().join("rust-toolchain.toml"));

    // A pinned channel installs rustc and cargo only. `cargo fmt --check` and
    // `cargo clippy` are two of the four commands every commit runs, and on a
    // fresh machine they are not there unless this file asks for them.
    for component in ["clippy", "rustfmt"] {
        assert!(
            toolchain.contains(&format!("\"{component}\"")),
            "rust-toolchain.toml must list the {component} component"
        );
    }
}

#[test]
fn the_pin_is_not_below_the_declared_minimum() {
    let root = workspace_root();
    let channel = string_value(&read(&root.join("rust-toolchain.toml")), "channel")
        .expect("rust-toolchain.toml declares a `channel`");
    let msrv = string_value(&read(&root.join("Cargo.toml")), "rust-version")
        .expect("the workspace declares a `rust-version`");

    // Compared on major.minor: `rust-version` carries no patch level.
    let pin = version_parts(&channel);
    let min = version_parts(&msrv);
    assert!(
        (pin[0], pin[1]) >= (min[0], min[1]),
        "pinned {channel} is below the declared minimum {msrv}; one of the two is wrong"
    );
}
