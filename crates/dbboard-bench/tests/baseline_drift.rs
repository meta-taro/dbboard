//! The baseline document lists exactly the points the harness measures.
//!
//! No number in `docs/performance-baseline.md` is asserted anywhere, and that
//! is deliberate (ADR-0141): a threshold on a timing is a test that fails on a
//! busy CI runner rather than on a regression, and this project has already
//! spent an ADR on one rare intermittent failure (ADR-0125).
//!
//! What is asserted is the *set of points*. Without this, deleting a
//! measurement would look exactly like the thing it measured getting faster —
//! the row simply stops being there, and the file still renders.

use dbboard_bench::harness::ids_in_markdown;
use dbboard_bench::points::POINTS;

const BASELINE: &str = "docs/performance-baseline.md";
const REGENERATE: &str = "cargo run --release -p dbboard-bench -- --write";

fn baseline_text() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("this crate sits two directories below the workspace root")
        .to_path_buf();
    let path = root.join(BASELINE);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e}\n\
             The baseline document is generated, not written by hand. \
             Regenerate it with:\n  {REGENERATE}",
            path.display()
        )
    })
}

#[test]
fn the_baseline_lists_every_measurement_point_in_catalogue_order() {
    let listed = ids_in_markdown(&baseline_text());
    let expected: Vec<String> = POINTS.iter().map(|p| p.id.to_owned()).collect();

    assert_eq!(
        listed, expected,
        "\n{BASELINE} and the point catalogue disagree.\n\
         A point was added, removed or reordered without the document being \
         regenerated. Run:\n  {REGENERATE}\n"
    );
}

#[test]
fn the_baseline_still_says_its_numbers_are_machine_specific() {
    // The sentence that stops the file being read as a cross-machine
    // comparison. It is generated, so losing it means the generator changed.
    let text = baseline_text();
    assert!(
        text.contains("These numbers describe one machine."),
        "{BASELINE} no longer warns that its numbers are machine-specific"
    );
}
