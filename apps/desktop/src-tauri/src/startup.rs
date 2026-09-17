//! When the window appeared, and when it first showed something.
//!
//! [`docs/startup-measurement.md`] records a median of 318 ms from launch to a
//! window existing, and says plainly what that number is not: it is the moment
//! the window server hands a window over, which is **before** the webview has
//! painted anything. An outside observer cannot see the difference — it can
//! poll for a window, and that is all.
//!
//! So the app says it instead. The shell stamps the clock as early as it can
//! reach, the frontend reports once it has drawn real content, and the gap
//! between them is the part nobody could measure from outside.
//!
//! **A second report is ignored rather than overwriting.** A webview reloads
//! — on a dev-server update, on a navigation — and each reload paints again.
//! The first one is the launch; the rest are not, and taking the last would
//! quietly turn a startup measurement into a reload measurement.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Launch-to-first-paint, as the app itself saw it.
#[derive(Debug)]
pub struct StartupClock {
    began: Instant,
    /// `None` until the frontend reports. Never replaced once set.
    first_paint: Mutex<Option<Duration>>,
}

/// What [`StartupClock::reading`] answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct StartupReading {
    /// Milliseconds from the shell starting to the frontend's first paint.
    /// `None` while the frontend has not reported yet.
    pub first_paint_ms: Option<u64>,
    /// Milliseconds the process has been up. Always answerable, and the thing
    /// to compare a `first_paint_ms` against when judging whether a report
    /// arrived late because painting was slow or because it was sent late.
    pub uptime_ms: u64,
}

impl StartupClock {
    /// Start the clock. Call this as early in `run()` as it can be reached:
    /// everything before it is invisible to the measurement and silently
    /// flatters the number.
    #[must_use]
    pub fn started() -> Self {
        Self {
            began: Instant::now(),
            first_paint: Mutex::new(None),
        }
    }

    /// Record the frontend's first paint. Returns what was stored — the first
    /// report, whether or not this call is it.
    ///
    /// # Panics
    ///
    /// If the lock is poisoned, which needs a panic while holding it; nothing
    /// here can panic between `lock` and the write.
    pub fn paint(&self) -> Duration {
        let mut slot = self.first_paint.lock().expect("startup clock lock");
        *slot.get_or_insert_with(|| self.began.elapsed())
    }

    /// The clock as it stands.
    ///
    /// # Panics
    ///
    /// Same as [`StartupClock::paint`].
    #[must_use]
    pub fn reading(&self) -> StartupReading {
        let slot = self.first_paint.lock().expect("startup clock lock");
        StartupReading {
            first_paint_ms: slot.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            uptime_ms: u64::try_from(self.began.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_reported_before_the_frontend_speaks() {
        let clock = StartupClock::started();
        assert_eq!(clock.reading().first_paint_ms, None);
    }

    #[test]
    fn the_first_paint_is_kept_and_a_later_one_is_ignored() {
        let clock = StartupClock::started();
        let first = clock.paint();
        std::thread::sleep(Duration::from_millis(12));
        let second = clock.paint();

        assert_eq!(first, second, "a reload must not overwrite the launch");
        assert_eq!(
            clock.reading().first_paint_ms,
            Some(first.as_millis() as u64)
        );
    }

    #[test]
    fn uptime_is_answerable_without_a_paint() {
        let clock = StartupClock::started();
        std::thread::sleep(Duration::from_millis(5));
        let reading = clock.reading();
        assert!(reading.uptime_ms >= 5, "uptime was {}", reading.uptime_ms);
        assert_eq!(reading.first_paint_ms, None);
    }

    /// The two numbers answer different questions and must not be conflated:
    /// a paint that lands at 900 ms is a different story when the process has
    /// been up 900 ms than when it has been up 40 seconds.
    #[test]
    fn a_paint_is_not_the_same_number_as_uptime() {
        let clock = StartupClock::started();
        clock.paint();
        std::thread::sleep(Duration::from_millis(20));
        let reading = clock.reading();
        assert!(
            reading.uptime_ms > reading.first_paint_ms.expect("painted"),
            "uptime {} should have moved past the paint {:?}",
            reading.uptime_ms,
            reading.first_paint_ms
        );
    }
}

/// The frontend says it has drawn real content. Answers the reading, so the
/// caller can log it without a second round trip.
///
/// Called once from the frontend after its first meaningful frame. A later
/// call — a webview reload paints again — is accepted and ignored.
#[tauri::command]
pub(crate) fn report_first_paint(state: tauri::State<'_, crate::AppState>) -> StartupReading {
    state.startup.paint();
    state.startup.reading()
}

/// The clock as it stands, without reporting anything.
#[tauri::command]
pub(crate) fn startup_timing(state: tauri::State<'_, crate::AppState>) -> StartupReading {
    state.startup.reading()
}
