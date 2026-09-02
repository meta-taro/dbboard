//! Summary statistics over a set of timings.
//!
//! Deliberately small. The questions this release slot asks are coarse — is
//! startup 20 ms or 2 s, does a 10,000-row result serialise in 5 ms or 500 ms
//! — and a median with a p95 answers them. A benchmark framework would answer
//! them too, at the cost of a dependency tree that `cargo clippy
//! --all-targets` recompiles on every push (ADR-0141).

use std::time::Duration;

/// The shape of one measurement point's timings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// How many timings this summarises. Recorded because a median over
    /// three samples and one over three hundred are not the same claim.
    pub samples: usize,
    pub min: Duration,
    pub median: Duration,
    pub p95: Duration,
    pub max: Duration,
}

impl Stats {
    /// Summarise `samples`, or `None` when there is nothing to summarise.
    ///
    /// Both percentiles use the *nearest-rank* definition: the value at
    /// position `ceil(q * n)` in the sorted set, 1-indexed. One rule serves
    /// both, no interpolation invents a duration that was never measured,
    /// and every result is a timing that actually happened.
    #[must_use]
    pub fn from_durations(samples: &[Duration]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Some(Self {
            samples: sorted.len(),
            min: sorted[0],
            median: nearest_rank(&sorted, 50),
            p95: nearest_rank(&sorted, 95),
            max: sorted[sorted.len() - 1],
        })
    }
}

/// The nearest-rank percentile of an already-sorted, non-empty slice.
///
/// `numerator` / 100 is the quantile: 50 for the median, 95 for the p95.
/// Integer ceiling division keeps the rank exact — a float `ceil(0.95 * 20)`
/// can land either side of 19 depending on the rounding of `0.95`.
fn nearest_rank(sorted: &[Duration], numerator: usize) -> Duration {
    let n = sorted.len();
    // Clamped rather than asserted: a quantile of 0 would rank 0, and a
    // quantile of 100 would rank n. Both are off the end by one.
    let rank = (numerator * n).div_ceil(100).clamp(1, n);
    sorted[rank - 1]
}

/// Render a duration at a scale a reader can compare at a glance.
///
/// Fixed units per magnitude rather than a single unit throughout: a table
/// holding both a 40 ns clone and a 1.2 s connect is unreadable in either
/// one of them.
///
/// Integer arithmetic throughout, because the obvious float version is wrong
/// in a way that shows. `Duration::as_secs_f64` builds its value as
/// `secs + nanos / 1e9`, and for 2345 ms that lands on 2.3449999999999998 —
/// a different double from the one the literal `2.345` parses to, and one
/// that renders as "2.34 s". These numbers go into a document people diff
/// between releases, so the last digit has to be the same digit every time,
/// on every platform.
#[must_use]
pub fn format_duration(d: Duration) -> String {
    // Each arm rounds to its own last displayed digit by adding half a step
    // before the truncating divide.
    match d.as_nanos() {
        n @ 0..=999 => format!("{n} ns"),
        n @ 1_000..=999_999 => {
            let tenths = (n + 50) / 100;
            format!("{}.{} \u{b5}s", tenths / 10, tenths % 10)
        }
        n @ 1_000_000..=999_999_999 => {
            let tenths = (n + 50_000) / 100_000;
            format!("{}.{} ms", tenths / 10, tenths % 10)
        }
        n => {
            let hundredths = (n + 5_000_000) / 10_000_000;
            format!("{}.{:02} s", hundredths / 100, hundredths % 100)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_duration, nearest_rank, Stats};
    use std::time::Duration;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn no_samples_summarise_to_nothing() {
        assert_eq!(Stats::from_durations(&[]), None);
    }

    #[test]
    fn a_single_sample_is_every_statistic() {
        let s = Stats::from_durations(&[ms(7)]).expect("one sample summarises");
        assert_eq!(s.samples, 1);
        assert_eq!(s.min, ms(7));
        assert_eq!(s.median, ms(7));
        assert_eq!(s.p95, ms(7));
        assert_eq!(s.max, ms(7));
    }

    #[test]
    fn min_and_max_are_the_extremes_regardless_of_input_order() {
        let s = Stats::from_durations(&[ms(9), ms(1), ms(5)]).expect("three samples summarise");
        assert_eq!(s.min, ms(1));
        assert_eq!(s.max, ms(9));
    }

    #[test]
    fn median_of_an_odd_count_is_the_middle_value() {
        let s = Stats::from_durations(&[ms(30), ms(10), ms(20)]).expect("three samples");
        assert_eq!(s.median, ms(20));
    }

    #[test]
    fn median_of_an_even_count_is_the_lower_of_the_two_middles() {
        // Nearest-rank, not interpolation: ceil(0.5 * 4) = 2, so the second
        // smallest. The alternative would report 25ms, a duration nothing
        // ever took.
        let s = Stats::from_durations(&[ms(10), ms(20), ms(30), ms(40)]).expect("four samples");
        assert_eq!(s.median, ms(20));
    }

    #[test]
    fn p95_of_a_hundred_samples_is_the_ninety_fifth() {
        let samples: Vec<Duration> = (1..=100).map(ms).collect();
        let s = Stats::from_durations(&samples).expect("a hundred samples");
        assert_eq!(s.p95, ms(95));
    }

    #[test]
    fn p95_of_twenty_samples_rounds_up_to_the_nineteenth() {
        // ceil(0.95 * 20) = 19. Computed with integers because the float
        // form of 0.95 is slightly under 0.95 and would rank 19th or 20th
        // depending on the platform's rounding.
        let samples: Vec<Duration> = (1..=20).map(ms).collect();
        let s = Stats::from_durations(&samples).expect("twenty samples");
        assert_eq!(s.p95, ms(19));
    }

    #[test]
    fn the_input_slice_is_not_required_to_be_sorted() {
        let ascending: Vec<Duration> = (1..=10).map(ms).collect();
        let descending: Vec<Duration> = (1..=10).rev().map(ms).collect();
        assert_eq!(
            Stats::from_durations(&ascending),
            Stats::from_durations(&descending)
        );
    }

    #[test]
    fn nearest_rank_never_indexes_past_the_end() {
        let sorted = [ms(1), ms(2), ms(3)];
        // ceil(1.00 * 3) = 3, the last element, not one past it.
        assert_eq!(nearest_rank(&sorted, 100), ms(3));
    }

    #[test]
    fn nearest_rank_of_a_low_quantile_is_still_a_real_sample() {
        let sorted = [ms(1), ms(2), ms(3)];
        // ceil(0.01 * 3) = 1, clamped up from 0 so it names an element.
        assert_eq!(nearest_rank(&sorted, 1), ms(1));
    }

    #[test]
    fn durations_below_a_microsecond_are_shown_in_nanoseconds() {
        assert_eq!(format_duration(Duration::from_nanos(940)), "940 ns");
    }

    #[test]
    fn durations_below_a_millisecond_are_shown_in_microseconds() {
        assert_eq!(format_duration(Duration::from_nanos(1_500)), "1.5 µs");
        assert_eq!(format_duration(Duration::from_nanos(999_400)), "999.4 µs");
    }

    #[test]
    fn durations_below_a_second_are_shown_in_milliseconds() {
        assert_eq!(format_duration(Duration::from_micros(1_500)), "1.5 ms");
        assert_eq!(format_duration(Duration::from_micros(41_230)), "41.2 ms");
    }

    #[test]
    fn durations_of_a_second_and_over_are_shown_in_seconds() {
        assert_eq!(format_duration(Duration::from_millis(1_000)), "1.00 s");
        assert_eq!(format_duration(Duration::from_millis(2_345)), "2.35 s");
    }

    #[test]
    fn a_zero_duration_still_renders() {
        assert_eq!(format_duration(Duration::ZERO), "0 ns");
    }
}
