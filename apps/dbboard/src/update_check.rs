//! Startup check for a newer published release (ADR-0040).
//!
//! Queries the GitHub Releases API for the latest release of the public
//! repo and compares its tag against this binary's own
//! `CARGO_PKG_VERSION`. When a newer version exists the Help menu grows a
//! notice plus the release notes; updating stays entirely manual — no
//! download, no restart, no forced upgrade. The check only *informs*.
//!
//! Privacy: this is the only network request dbboard makes on its own
//! behalf (every other call targets a database the user configured). It is
//! best-effort — any failure (offline, rate-limited, malformed JSON) is
//! logged and swallowed, never surfaced as an error — and it can be turned
//! off entirely by setting `DBBOARD_NO_UPDATE_CHECK` to any non-empty
//! value. See ADR-0040.

use std::sync::{Arc, Mutex};

/// GitHub Releases API endpoint for the *latest* (non-draft, non-pre)
/// release. GitHub excludes drafts and pre-releases from this route, so a
/// successful response is always a real, published version.
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/meta-taro/dbboard/releases/latest";

/// Env var that disables the check outright. Any non-empty value opts out;
/// no request is made and the state stays [`UpdateState::Idle`].
const OPT_OUT_ENV: &str = "DBBOARD_NO_UPDATE_CHECK";

/// Shared, UI-readable outcome of the update check. The spawned task holds
/// one clone and writes the terminal state exactly once; the Help menu
/// reads a snapshot every frame it is open.
pub type SharedUpdateState = Arc<Mutex<UpdateState>>;

/// Lifecycle of the one-shot update check.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
    /// Check disabled (opt-out) or not yet started. The Help menu shows
    /// nothing extra.
    #[default]
    Idle,
    /// Request in flight.
    Checking,
    /// Running the newest published release.
    UpToDate,
    /// A newer release exists — the only state the Help menu surfaces.
    Available(ReleaseInfo),
    /// The check could not complete (offline, HTTP error, bad JSON). The
    /// UI stays silent; the reason is logged to stderr.
    Failed,
}

/// The pieces of a newer release the Help menu shows: the version to name,
/// the notes to read, and the page to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Normalised version of the latest release, e.g. `0.2.0` (no leading
    /// `v`).
    pub version: String,
    /// Release notes (the GitHub release body) shown as the changelog.
    pub notes: String,
    /// Canonical web page for the release, opened from the Help menu.
    pub url: String,
}

/// Spawn the one-shot check onto the shared runtime and hand back the
/// state the Help menu reads. Returns immediately; the network call runs
/// in the background and requests a repaint when it resolves so an already
/// open Help menu updates without a manual interaction.
///
/// Honours the [`OPT_OUT_ENV`] opt-out: when set, no request is made and
/// the returned state stays [`UpdateState::Idle`].
pub fn spawn(rt: &tokio::runtime::Handle, ctx: egui::Context) -> SharedUpdateState {
    let state: SharedUpdateState = Arc::new(Mutex::new(UpdateState::Idle));

    if opt_out(std::env::var(OPT_OUT_ENV).ok().as_deref()) {
        eprintln!("dbboard: update check disabled via {OPT_OUT_ENV}");
        return state;
    }

    set_state(&state, UpdateState::Checking);
    let out = Arc::clone(&state);
    rt.spawn(async move {
        let next = run_check(env!("CARGO_PKG_VERSION")).await;
        set_state(&out, next);
        // Wake the UI so an open Help menu reflects the result this frame
        // rather than on the next unrelated repaint.
        ctx.request_repaint();
    });

    state
}

/// Write a terminal state, tolerating a poisoned lock (a panicked reader
/// leaves the enum valid, so recover and keep going rather than abort).
fn set_state(state: &SharedUpdateState, next: UpdateState) {
    let mut guard = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = next;
}

/// True when the opt-out env var is present and non-empty. Split out from
/// [`spawn`] so the policy is unit-testable without mutating process env.
fn opt_out(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty())
}

/// Drive the fetch + classify pipeline, mapping every failure to
/// [`UpdateState::Failed`] with a logged reason. Never returns an error:
/// an update check must not be able to break startup.
async fn run_check(current: &str) -> UpdateState {
    match fetch_latest().await {
        Ok(release) => classify(current, release),
        Err(reason) => {
            eprintln!("dbboard: update check failed (non-fatal): {reason}");
            UpdateState::Failed
        }
    }
}

/// The subset of the GitHub release JSON we consume. `#[serde(default)]`
/// keeps a release with an empty body or a missing URL from failing the
/// whole parse — only `tag_name` is required to decide newness.
#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
}

/// GET the latest release. GitHub rejects requests without a `User-Agent`,
/// so we send a descriptive one. A non-2xx status is an error, as is any
/// transport or JSON failure — all fold into `Err(String)` for logging.
async fn fetch_latest() -> Result<GithubRelease, String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("client init: {e}"))?;
    let response = client
        .get(LATEST_RELEASE_URL)
        .header(
            reqwest::header::USER_AGENT,
            concat!("dbboard/", env!("CARGO_PKG_VERSION")),
        )
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("GitHub returned {}", response.status()));
    }
    response
        .json::<GithubRelease>()
        .await
        .map_err(|e| format!("decode: {e}"))
}

/// Decide, from the current version and a fetched release, whether to
/// surface an update. Pure so the "is this newer, and what do we show"
/// decision is unit-testable without a network round-trip.
fn classify(current: &str, release: GithubRelease) -> UpdateState {
    if is_newer(current, &release.tag_name) {
        UpdateState::Available(ReleaseInfo {
            version: normalise(&release.tag_name),
            notes: release.body,
            url: release.html_url,
        })
    } else {
        UpdateState::UpToDate
    }
}

/// A `major.minor.patch` version. Pre-release and build metadata are
/// deliberately dropped before comparison (see [`parse_version`]) — an
/// internal build never ships a pre-release tag, and treating `0.2.0` and
/// `0.2.0-rc1` as equal is the safe "don't nag" choice.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

/// Parse a tag like `v0.2.0`, `0.2`, or `1` into a [`Version`]. Tolerates a
/// leading `v`/`V`, fills missing minor/patch with `0`, and drops any
/// `-pre` / `+build` suffix. Returns `None` when the numeric core does not
/// parse, so an unrecognised tag is treated as "no update" rather than a
/// spurious one.
fn parse_version(raw: &str) -> Option<Version> {
    let core = normalise(raw);
    // Strip SemVer pre-release / build metadata before splitting on `.`.
    let core = core.split(['-', '+']).next().unwrap_or(&core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some(Version(major, minor, patch))
}

/// Strip a single leading `v`/`V` and surrounding whitespace from a tag so
/// `v0.2.0` and `0.2.0` normalise identically. Used both for comparison
/// and for the version string shown in the notice.
fn normalise(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix(['v', 'V'])
        .unwrap_or(trimmed)
        .to_string()
}

/// True when `latest` is strictly greater than `current`. When either side
/// fails to parse we return `false`: a malformed tag must never nag the
/// user with a phantom update.
fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(cur), Some(new)) => new > cur,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            body: String::new(),
            html_url: String::new(),
        }
    }

    #[test]
    fn parse_version_accepts_v_prefix_and_missing_components() {
        assert_eq!(parse_version("v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(parse_version("0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(parse_version("1.4"), Some(Version(1, 4, 0)));
        assert_eq!(parse_version("2"), Some(Version(2, 0, 0)));
        assert_eq!(parse_version(" V3.1.5 "), Some(Version(3, 1, 5)));
    }

    #[test]
    fn parse_version_drops_prerelease_and_build_metadata() {
        assert_eq!(parse_version("0.2.0-rc1"), Some(Version(0, 2, 0)));
        assert_eq!(parse_version("1.0.0+build.7"), Some(Version(1, 0, 0)));
    }

    #[test]
    fn parse_version_rejects_non_numeric() {
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("v"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn is_newer_compares_across_all_components() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(is_newer("0.2.0", "v0.2.1"));
    }

    #[test]
    fn is_newer_is_false_for_equal_or_older() {
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn is_newer_is_false_when_either_side_is_unparseable() {
        // A malformed tag must not manufacture a phantom update.
        assert!(!is_newer("0.1.0", "not-a-version"));
        assert!(!is_newer("garbage", "9.9.9"));
    }

    #[test]
    fn classify_flags_a_newer_release_and_carries_notes_and_url() {
        let mut rel = release("v0.3.0");
        rel.body = "- fixed the thing\n- added another".to_string();
        rel.html_url = "https://example.test/releases/v0.3.0".to_string();

        let state = classify("0.2.0", rel);

        assert_eq!(
            state,
            UpdateState::Available(ReleaseInfo {
                version: "0.3.0".to_string(),
                notes: "- fixed the thing\n- added another".to_string(),
                url: "https://example.test/releases/v0.3.0".to_string(),
            })
        );
    }

    #[test]
    fn classify_reports_up_to_date_when_not_newer() {
        assert_eq!(classify("0.2.0", release("v0.2.0")), UpdateState::UpToDate);
        assert_eq!(classify("0.2.0", release("v0.1.0")), UpdateState::UpToDate);
    }

    #[test]
    fn classify_is_up_to_date_when_tag_is_unparseable() {
        // An unrecognised tag is treated as "nothing to offer", never a
        // spurious Available.
        assert_eq!(classify("0.2.0", release("nightly")), UpdateState::UpToDate);
    }

    #[test]
    fn opt_out_only_triggers_on_a_non_empty_value() {
        assert!(opt_out(Some("1")));
        assert!(opt_out(Some("anything")));
        assert!(!opt_out(Some("")));
        assert!(!opt_out(None));
    }
}
