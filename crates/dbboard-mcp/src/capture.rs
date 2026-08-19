//! Screen capture of the running dbboard window (ADR-0108).
//!
//! The one tool here reaches neither a database nor dbboard's config: it
//! photographs whatever the app is currently drawing. It exists because
//! verifying the *interface* — that a locale renders, that a grid is not
//! full of tofu, that an error is legible — is something an agent can only
//! do if it can see the screen. Without it every visual check costs a human
//! a screenshot and a paste.
//!
//! The window is found by application name rather than by window title,
//! because titles are not ours: a terminal tab called "dbboard" enumerates
//! with exactly that title, and capturing it instead would look like a
//! success. `app_name` comes from the executable / bundle name, which is
//! `productName` in tauri.conf.json.
//!
//! Selection and scaling are pure and tested; only [`capture_window`] calls
//! the platform, and it is a thin wrapper over the two.

use std::io::Cursor;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Serialize;
use xcap::image::{imageops::FilterType, ImageFormat, RgbaImage};

/// Application names that count as "the dbboard app". `dbboard-desktop` is
/// `productName`; the bare name is kept because a platform that reports the
/// window class rather than the binary would use it. Matching is on the
/// whole name, never a substring — a substring match is what lets an
/// unrelated window with dbboard in its name win.
const APP_NAMES: [&str; 2] = ["dbboard-desktop", "dbboard"];

/// Default long-edge limit for a returned capture. The app's own window is
/// around 1280 wide, so this is "full size unless someone maximised it":
/// enough to read a menu label, small enough that the image does not
/// dominate the agent's context.
pub const DEFAULT_MAX_EDGE: u32 = 1400;

/// What went wrong instead of a picture.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// No dbboard window is on screen. A human has to start the app; there
    /// is nothing the caller can rephrase.
    #[error("dbboard is not running (no window belonging to `dbboard-desktop` was found)")]
    NotRunning,
    /// The app is running but every one of its windows is minimised. Also a
    /// human's job — restoring a window is not something MCP can do.
    #[error("the dbboard window is minimised; restore it before capturing")]
    Minimized,
    /// The platform refused to enumerate or to grab the pixels.
    #[error("screen capture failed: {0}")]
    Backend(String),
}

/// The facts about one on-screen window that selection needs. Deliberately
/// owns its strings: it is built from a platform handle that we do not want
/// to keep alive across the decision.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub app_name: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub minimized: bool,
}

/// A capture, ready to hand to an agent.
#[derive(Debug, Serialize)]
pub struct CaptureShot {
    /// PNG bytes, base64-encoded for the MCP image content block.
    #[serde(skip)]
    pub png_base64: String,
    /// The window title at the moment of capture.
    pub title: String,
    /// Size of the returned image, after any scaling.
    pub width: u32,
    pub height: u32,
    /// Size the window actually is, before scaling. Equal to the above when
    /// nothing was scaled.
    pub source_width: u32,
    pub source_height: u32,
}

/// Pick the dbboard window out of everything currently on screen.
///
/// Returns an index into `windows`. Minimised windows are skipped rather
/// than failed on, so a stray minimised instance cannot hide a visible one;
/// only when *every* candidate is minimised does that become the error.
/// Among several visible windows the largest wins — a Tauri app can hold
/// invisible helper windows, and the one a human is looking at is the big
/// one.
///
/// # Errors
///
/// [`CaptureError::NotRunning`] when no window belongs to the app, and
/// [`CaptureError::Minimized`] when they all do but none is on screen.
pub fn select_window(windows: &[WindowInfo]) -> Result<usize, CaptureError> {
    let mut candidates = windows
        .iter()
        .enumerate()
        .filter(|(_, w)| APP_NAMES.iter().any(|n| w.app_name.eq_ignore_ascii_case(n)))
        .peekable();

    if candidates.peek().is_none() {
        return Err(CaptureError::NotRunning);
    }

    candidates
        .filter(|(_, w)| !w.minimized)
        .max_by_key(|(_, w)| u64::from(w.width) * u64::from(w.height))
        .map(|(i, _)| i)
        .ok_or(CaptureError::Minimized)
}

/// Size an image down so its long edge is at most `max_edge`, keeping the
/// aspect ratio. Never enlarges: a window smaller than the limit is worth
/// less, not more, blown up.
///
/// Integer arithmetic throughout, in `u64` so the multiplication cannot
/// overflow — a float ratio would round the long edge itself off by a pixel
/// on some sizes, which is how a "max 1400" turns into 1401.
#[must_use]
pub fn scale_to_fit(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let long_edge = width.max(height);
    if max_edge == 0 || long_edge <= max_edge {
        return (width, height);
    }
    let (long_edge, max_edge) = (u64::from(long_edge), u64::from(max_edge));
    // Round half up, then floor at one: a very wide window scaled hard would
    // otherwise round its short edge to zero, which no encoder accepts.
    let scale = |v: u32| {
        u32::try_from((u64::from(v) * max_edge + long_edge / 2) / long_edge)
            .unwrap_or(u32::MAX)
            .max(1)
    };
    (scale(width), scale(height))
}

/// Photograph the dbboard window.
///
/// Blocking: it enumerates windows and copies pixels. Callers on an async
/// runtime should hand it to a blocking task.
///
/// # Errors
///
/// Whatever [`select_window`] decides, plus [`CaptureError::Backend`] when
/// the platform refuses to enumerate, to copy the pixels, or to encode them.
pub fn capture_window(max_edge: u32) -> Result<CaptureShot, CaptureError> {
    let windows = xcap::Window::all().map_err(|e| CaptureError::Backend(e.to_string()))?;

    // `xcap` returns each attribute as a `Result`; a window that vanished
    // between enumeration and inspection answers with an error rather than
    // stale values. Treating that as "not a candidate" is right — it is
    // gone — so those windows describe themselves as unusable and drop out
    // of selection on their own.
    let infos: Vec<WindowInfo> = windows
        .iter()
        .map(|w| WindowInfo {
            app_name: w.app_name().unwrap_or_default(),
            title: w.title().unwrap_or_default(),
            width: w.width().unwrap_or(0),
            height: w.height().unwrap_or(0),
            minimized: w.is_minimized().unwrap_or(true),
        })
        .collect();

    let index = select_window(&infos)?;
    let image: RgbaImage = windows[index]
        .capture_image()
        .map_err(|e| CaptureError::Backend(e.to_string()))?;

    let (source_width, source_height) = (image.width(), image.height());
    let (width, height) = scale_to_fit(source_width, source_height, max_edge);
    let image = if (width, height) == (source_width, source_height) {
        image
    } else {
        // Lanczos3 over the cheaper filters: the thing being judged in these
        // captures is usually text, and a nearest/triangle downscale of CJK
        // glyphs turns legible characters into the very mush the capture is
        // meant to detect.
        xcap::image::imageops::resize(&image, width, height, FilterType::Lanczos3)
    };

    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| CaptureError::Backend(format!("could not encode PNG: {e}")))?;

    Ok(CaptureShot {
        png_base64: STANDARD.encode(&png),
        title: infos[index].title.clone(),
        width,
        height,
        source_width,
        source_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(app_name: &str, width: u32, height: u32) -> WindowInfo {
        WindowInfo {
            app_name: app_name.to_string(),
            title: "dbboard".to_string(),
            width,
            height,
            minimized: false,
        }
    }

    #[test]
    fn picks_the_dbboard_app_window() {
        let windows = vec![
            window("Some Browser", 1920, 1080),
            window("dbboard-desktop", 1282, 802),
        ];

        assert_eq!(select_window(&windows).unwrap(), 1);
    }

    // The case that made app-name matching non-negotiable: a terminal whose
    // tab is called "dbboard" enumerates with exactly that title, and a
    // title-matching selector photographs the terminal and calls it dbboard.
    #[test]
    fn ignores_a_window_merely_titled_dbboard() {
        let windows = vec![WindowInfo {
            app_name: "Windows Terminal Host".to_string(),
            title: "dbboard".to_string(),
            width: 960,
            height: 516,
            minimized: false,
        }];

        assert!(matches!(
            select_window(&windows),
            Err(CaptureError::NotRunning)
        ));
    }

    #[test]
    fn matches_the_app_name_case_insensitively() {
        let windows = vec![window("dbboard-Desktop", 800, 600)];

        assert_eq!(select_window(&windows).unwrap(), 0);
    }

    #[test]
    fn rejects_an_app_name_that_merely_contains_dbboard() {
        let windows = vec![window("not-dbboard-desktop-either", 800, 600)];

        assert!(matches!(
            select_window(&windows),
            Err(CaptureError::NotRunning)
        ));
    }

    #[test]
    fn no_dbboard_window_at_all_is_not_running() {
        let windows = vec![window("Some Browser", 1920, 1080)];

        assert!(matches!(
            select_window(&windows),
            Err(CaptureError::NotRunning)
        ));
    }

    // A Tauri app can hold windows a human never sees. The one being looked
    // at is the big one.
    #[test]
    fn prefers_the_largest_visible_window() {
        let windows = vec![
            window("dbboard-desktop", 1, 1),
            window("dbboard-desktop", 1282, 802),
        ];

        assert_eq!(select_window(&windows).unwrap(), 1);
    }

    #[test]
    fn skips_a_minimized_window_in_favour_of_a_visible_one() {
        let windows = vec![
            WindowInfo {
                minimized: true,
                ..window("dbboard-desktop", 1920, 1080)
            },
            window("dbboard-desktop", 800, 600),
        ];

        assert_eq!(select_window(&windows).unwrap(), 1);
    }

    #[test]
    fn all_windows_minimized_is_its_own_error() {
        let windows = vec![WindowInfo {
            minimized: true,
            ..window("dbboard-desktop", 1282, 802)
        }];

        assert!(matches!(
            select_window(&windows),
            Err(CaptureError::Minimized)
        ));
    }

    #[test]
    fn leaves_an_image_under_the_limit_alone() {
        assert_eq!(scale_to_fit(1282, 802, 1400), (1282, 802));
    }

    #[test]
    fn never_enlarges() {
        assert_eq!(scale_to_fit(400, 300, 4000), (400, 300));
    }

    #[test]
    fn scales_the_long_edge_down_keeping_the_ratio() {
        assert_eq!(scale_to_fit(2000, 1000, 1000), (1000, 500));
    }

    #[test]
    fn scales_by_height_when_the_window_is_tall() {
        assert_eq!(scale_to_fit(1000, 2000, 1000), (500, 1000));
    }

    // Rounding a very wide image down must not produce a zero-pixel edge:
    // an image encoder rejects that, so the capture would fail rather than
    // return something small.
    #[test]
    fn keeps_at_least_one_pixel_on_the_short_edge() {
        let (width, height) = scale_to_fit(10_000, 3, 100);

        assert_eq!(width, 100);
        assert!(height >= 1);
    }
}
