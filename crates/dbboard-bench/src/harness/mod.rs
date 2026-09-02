//! Turning a pile of timings into a table a person can read.

mod report;
mod stats;

pub use report::{ids_in_markdown, render_markdown, today, Machine, Reading};
pub use stats::{format_duration, Stats};
