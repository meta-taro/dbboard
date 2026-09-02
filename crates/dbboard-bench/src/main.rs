//! Runs every measurement and renders `docs/performance-baseline.md`.
//!
//! ```text
//! cargo run --release -p dbboard-bench                 # print to stdout
//! cargo run --release -p dbboard-bench -- --write      # rewrite the document
//! cargo run --release -p dbboard-bench -- --hardware "Mac mini (M1, 2020), 16 GB"
//! ```
//!
//! `--release` is not optional advice. A debug build measures the debug
//! build, and nobody ships one.

use std::path::{Path, PathBuf};

use dbboard_bench::harness::{render_markdown, today, Machine};
use dbboard_bench::measure::{run_all, BenchResult, SAMPLES};

/// Where the rendered document lives, relative to the workspace root.
const BASELINE: &str = "docs/performance-baseline.md";

fn workspace_root() -> PathBuf {
    // crates/dbboard-bench -> crates -> <root>
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the workspace root")
        .to_path_buf()
}

/// The exact compiler `rust-toolchain.toml` pins (ADR-0139).
///
/// Read from the file rather than from a build script's `rustc -V`, so the
/// document names the pin. A run under a different compiler than the pinned
/// one is a run whose numbers should not be filed here at all.
fn pinned_toolchain(root: &Path) -> String {
    let path = root.join("rust-toolchain.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return "unknown".to_owned();
    };
    text.parse::<toml::Value>()
        .ok()
        .and_then(|v| {
            v.get("toolchain")?
                .get("channel")?
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn describe_hardware(override_text: Option<String>) -> String {
    override_text.unwrap_or_else(|| {
        let cores = std::thread::available_parallelism()
            .map_or_else(|_| "unknown".to_owned(), |n| format!("{n} logical cores"));
        format!("{cores} (pass --hardware for the model)")
    })
}

#[tokio::main]
async fn main() -> BenchResult<()> {
    let mut write = false;
    let mut hardware = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--write" => write = true,
            "--hardware" => hardware = args.next(),
            other => {
                return Err(format!(
                    "unknown argument {other:?}; expected --write or --hardware <text>"
                )
                .into())
            }
        }
    }

    let root = workspace_root();
    let machine = Machine {
        hardware: describe_hardware(hardware),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        toolchain: pinned_toolchain(&root),
        measured_on: today(),
        samples: SAMPLES,
    };

    eprintln!("measuring ({SAMPLES} timed samples per point)…");
    let readings = run_all().await?;
    let document = render_markdown(&machine, &readings);

    if write {
        let path = root.join(BASELINE);
        std::fs::write(&path, &document)?;
        eprintln!("wrote {}", path.display());
    } else {
        print!("{document}");
    }
    Ok(())
}
