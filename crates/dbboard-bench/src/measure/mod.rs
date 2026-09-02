//! Running the measurements.
//!
//! Every point follows the same shape: discard [`WARMUP`] iterations, then
//! time [`SAMPLES`] of them. The warm-up is not politeness — the first call
//! into a fresh page of allocator arena, or the first parse that faults in a
//! file, is measuring the machine's state rather than the code's cost.

mod browse;
mod results;
mod startup;

use std::time::Duration;

use crate::harness::{Reading, Stats};
use crate::points::{point, Point};

/// What a measurement can fail with. Any of the layers below can, and none of
/// the failures are worth distinguishing here: a benchmark that cannot set up
/// its fixture has nothing to report either way.
pub type BenchResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Iterations run and thrown away before timing starts.
pub const WARMUP: usize = 5;

/// Timed iterations per point.
///
/// Fifty rather than five hundred because the whole run has to stay short
/// enough that it gets run. The p95 of fifty samples is the 48th value, which
/// is a coarse tail — enough to notice a stall, not enough to characterise
/// one.
pub const SAMPLES: usize = 50;

/// Collects timings for one point and decides when there are enough.
pub struct Sampler {
    point: &'static Point,
    warmup_left: usize,
    collected: Vec<Duration>,
}

impl Sampler {
    /// Start sampling the catalogued point `id`.
    ///
    /// # Panics
    ///
    /// Panics when `id` is not in [`crate::points::POINTS`]. That is a typo in
    /// this crate, not a runtime condition: an uncatalogued point would be
    /// measured and then silently dropped by the renderer, which is a worse
    /// outcome than a crash during development.
    #[must_use]
    pub fn new(id: &'static str) -> Self {
        Self {
            point: point(id).unwrap_or_else(|| panic!("{id} is not in the point catalogue")),
            warmup_left: WARMUP,
            collected: Vec::with_capacity(SAMPLES),
        }
    }

    /// Whether the loop should run another iteration.
    #[must_use]
    pub fn wants_more(&self) -> bool {
        self.warmup_left > 0 || self.collected.len() < SAMPLES
    }

    /// Offer one iteration's elapsed time. Warm-up iterations are discarded.
    pub fn record(&mut self, elapsed: Duration) {
        if self.warmup_left > 0 {
            self.warmup_left -= 1;
        } else {
            self.collected.push(elapsed);
        }
    }

    /// Summarise, or `None` when nothing was timed.
    #[must_use]
    pub fn finish(self) -> Option<Reading> {
        Stats::from_durations(&self.collected).map(|stats| Reading {
            point: self.point,
            stats,
        })
    }
}

/// Run every group, in the order the baseline document renders them.
///
/// # Errors
///
/// Returns the first fixture or query failure. A partial run is not reported:
/// a baseline missing the group that failed would be read as a baseline where
/// that group got faster.
pub async fn run_all() -> BenchResult<Vec<Reading>> {
    let mut readings = Vec::new();
    startup::measure(&mut readings)?;
    browse::measure(&mut readings).await?;
    results::measure(&mut readings).await?;
    Ok(readings)
}

#[cfg(test)]
mod tests {
    use super::{Sampler, SAMPLES, WARMUP};
    use std::time::Duration;

    #[test]
    fn warmup_iterations_are_discarded() {
        let mut s = Sampler::new("startup/config_paths");
        // Warm-ups are deliberately slow; if they leaked into the summary the
        // median would be 900ms rather than 1ms.
        for _ in 0..WARMUP {
            s.record(Duration::from_millis(900));
        }
        for _ in 0..SAMPLES {
            s.record(Duration::from_millis(1));
        }
        let reading = s.finish().expect("samples were recorded");
        assert_eq!(reading.stats.samples, SAMPLES);
        assert_eq!(reading.stats.max, Duration::from_millis(1));
    }

    #[test]
    fn the_loop_stops_after_warmup_plus_samples() {
        let mut s = Sampler::new("startup/config_paths");
        let mut iterations = 0;
        while s.wants_more() {
            s.record(Duration::from_millis(1));
            iterations += 1;
            assert!(
                iterations <= WARMUP + SAMPLES + 1,
                "wants_more never went false"
            );
        }
        assert_eq!(iterations, WARMUP + SAMPLES);
    }

    #[test]
    fn a_sampler_that_recorded_nothing_reports_nothing() {
        let s = Sampler::new("startup/config_paths");
        assert!(s.finish().is_none());
    }

    #[test]
    #[should_panic(expected = "is not in the point catalogue")]
    fn an_uncatalogued_point_is_a_bug_not_a_silent_drop() {
        let _ = Sampler::new("startup/invented_on_the_spot");
    }
}
