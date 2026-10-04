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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch. Saturates rather than failing: a clock
/// set before 1970 is a broken machine, not a case worth an error path here.
fn unix_millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Launch-to-first-paint, as the app itself saw it.
#[derive(Debug)]
pub struct StartupClock {
    began: Instant,
    /// The same moment as `began`, on the wall clock, so a stopwatch outside
    /// this process can line its own reading up with this one. `Instant` is
    /// deliberately opaque and cannot be shared across processes.
    began_at: SystemTime,
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
    /// When the clock started, in milliseconds since the Unix epoch.
    ///
    /// This exists because `first_paint_ms` **cannot see a cold start**. The
    /// clock starts on the first line of `run()`, by which point the kernel
    /// has exec'd the binary and dyld has paged it in — and that is precisely
    /// the work a warm launch skips. Measured on 2026-09-24: the first launch
    /// after a reboot reported 451 ms and the next one 431 ms, a 4% gap where
    /// the disk had every chance to show itself.
    ///
    /// Publishing the start on a clock other processes share lets a stopwatch
    /// that began *before this process existed* subtract, and recover the part
    /// no code inside can reach. `scripts/measure-cold-start.sh` does that.
    pub started_at_unix_ms: u64,
}

impl StartupClock {
    /// Start the clock. Call this as early in `run()` as it can be reached:
    /// everything before it is invisible to the measurement and silently
    /// flatters the number.
    #[must_use]
    pub fn started() -> Self {
        Self {
            began: Instant::now(),
            began_at: SystemTime::now(),
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
            started_at_unix_ms: unix_millis(self.began_at),
        }
    }
}

/// Where to leave the reading, if anyone asked. Set by the measurement
/// harness; unset in every ordinary launch.
pub const REPORT_PATH_VAR: &str = "DBBOARD_STARTUP_REPORT";

/// The path the harness named, or `None` when nobody asked.
#[must_use]
pub fn report_destination() -> Option<std::path::PathBuf> {
    match std::env::var_os(REPORT_PATH_VAR) {
        Some(raw) if !raw.is_empty() => Some(std::path::PathBuf::from(raw)),
        _ => None,
    }
}

/// Leave the reading where a stopwatch outside this process can pick it up.
///
/// The harness polls for this file, so it is written whole and renamed into
/// place: a reader that catches a half-written file would parse a truncated
/// number rather than fail, and quietly report a wrong measurement.
///
/// # Errors
///
/// If the file cannot be written or moved into place.
pub fn write_report(path: &std::path::Path, reading: &StartupReading) -> std::io::Result<()> {
    let json = serde_json::to_string(reading).map_err(std::io::Error::other)?;
    let staging = path.with_extension("partial");
    std::fs::write(&staging, json)?;
    std::fs::rename(&staging, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique path under the OS temp dir. Tests in this module run in the
    /// same process, so a shared name would have them clobber each other.
    fn scratch_path(tag: &str) -> std::path::PathBuf {
        let unique = unix_millis(SystemTime::now());
        std::env::temp_dir().join(format!(
            "dbboard-startup-{tag}-{unique}-{:?}.json",
            std::thread::current().id()
        ))
    }

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

    /// The clock cannot see the expensive half of a cold start. `started()`
    /// runs after the kernel has exec'd the binary and dyld has paged 47 MB of
    /// it off disk, and *that* is where a cold launch differs from a warm one:
    /// measured on 2026-09-24, a genuine first-launch-after-reboot reported
    /// 451 ms and the very next launch reported 431 ms. Twenty milliseconds is
    /// not the cost of a cold disk; it is the noise between two warm paints.
    ///
    /// So the clock publishes *when* it started, in wall-clock terms. A
    /// stopwatch outside — one that took its reading before the process
    /// existed — can subtract and get the part nobody inside can see.
    #[test]
    fn the_reading_says_when_the_clock_started_in_wall_clock_terms() {
        let before = unix_millis(SystemTime::now());
        let clock = StartupClock::started();
        let after = unix_millis(SystemTime::now());

        let at = clock.reading().started_at_unix_ms;
        assert!(
            (before..=after).contains(&at),
            "started_at {at} should sit within {before}..={after}"
        );
    }

    /// The two numbers are only useful together: the outside stopwatch knows
    /// when it pressed the button, and this pair says when painting finished
    /// on the same clock. Anything that drifts between them makes the
    /// subtraction meaningless.
    #[test]
    fn the_start_plus_the_paint_is_when_painting_finished() {
        let clock = StartupClock::started();
        std::thread::sleep(Duration::from_millis(15));
        clock.paint();

        let reading = clock.reading();
        let painted_at = reading.started_at_unix_ms + reading.first_paint_ms.expect("painted");
        let now = unix_millis(SystemTime::now());

        assert!(
            painted_at <= now,
            "painting finished at {painted_at}, which is after now ({now})"
        );
        assert!(
            now - painted_at < 1_000,
            "painting finished {} ms ago, which is too long for this test",
            now - painted_at
        );
    }

    /// A stopwatch outside cannot call a Tauri command — those only answer the
    /// frontend. So when asked, the app drops its reading in a file the
    /// stopwatch named, and the stopwatch waits for it to appear.
    #[test]
    fn the_reading_can_be_left_somewhere_a_stopwatch_will_find_it() {
        let path = scratch_path("found");
        let clock = StartupClock::started();
        clock.paint();

        write_report(&path, &clock.reading()).expect("write the report");

        let text = std::fs::read_to_string(&path).expect("read it back");
        let back: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert!(back["started_at_unix_ms"].as_u64().unwrap_or(0) > 0);
        assert!(back["first_paint_ms"].as_u64().is_some(), "{back}");

        std::fs::remove_file(&path).ok();
    }

    /// Nothing is written unless a path was asked for. A measurement harness
    /// is the only caller; an ordinary launch must not start leaving files
    /// around a user's disk.
    #[test]
    fn nothing_is_written_when_no_one_asked() {
        // SAFETY: single-threaded test process; no other thread reads the env.
        unsafe { std::env::remove_var(REPORT_PATH_VAR) };
        assert_eq!(report_destination(), None);
    }

    #[test]
    fn a_named_destination_is_used() {
        let path = scratch_path("asked");
        // SAFETY: as above.
        unsafe { std::env::set_var(REPORT_PATH_VAR, &path) };
        assert_eq!(report_destination().as_deref(), Some(path.as_path()));
        // SAFETY: as above.
        unsafe { std::env::remove_var(REPORT_PATH_VAR) };
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
    let reading = state.startup.reading();

    // Only when a harness asked. A failure here is not the app's problem to
    // raise: the measurement is a side errand, and a launch that cannot write
    // a scratch file should still open.
    if let Some(path) = report_destination() {
        let _ = write_report(&path, &reading);
    }

    reading
}

/// The clock as it stands, without reporting anything.
#[tauri::command]
pub(crate) fn startup_timing(state: tauri::State<'_, crate::AppState>) -> StartupReading {
    state.startup.reading()
}
