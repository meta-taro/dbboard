// Download page logic (ADR-0047). Kept as a same-origin file (not inline) so
// the page's CSP can be script-src 'self' — an injected inline script cannot
// run.
//
// Public repo → the Releases API needs no auth. Unauthenticated calls are
// rate-limited (~60/hr per IP); on any failure we fall back to a direct link
// to the Releases page rather than showing a broken state.
const REPO = "meta-taro/dbboard";
const RELEASES_URL = `https://github.com/${REPO}/releases`;

// Classify an asset by filename into a platform bucket. Names are set by our
// own release CI (ADR-0044); the Tauri bundles all carry the product name
// `dbboard-desktop`: dbboard-desktop_<v>_x64-setup.exe,
// dbboard-desktop_<v>_universal.dmg, plus SHA256SUMS.txt.
//
// Matching that prefix rather than the extension is load-bearing. Releases up
// to v0.4.0 also carry the retired egui client (ADR-0089) —
// dbboard-windows-x86_64.exe, dbboard-<v>-x86_64.msi,
// dbboard-macos-universal-<v>.dmg — and keying on the extension alone made
// which build a card offered depend on the order the Releases API returned
// assets in (#135). Anything that is not a recognised Tauri bundle, including
// the updater's own `.app.tar.gz` / `.sig` / latest.json, is not a download.
export function bucketFor(name) {
  const n = name.toLowerCase();
  if (n === "sha256sums.txt") return "sums";
  // Two spellings, because the client was renamed in v0.15.0 and this page
  // lists whichever release is being looked at. `dbboard_` keeps its
  // underscore on purpose: the retired egui client was called `dbboard` too
  // and separated its version with a hyphen, so the underscore is the only
  // thing standing between "the current client" and "the one that no longer
  // ships".
  if (!n.startsWith("dbboard-desktop") && !n.startsWith("dbboard_")) return null;
  if (n.endsWith("-setup.exe")) return "win-setup";
  if (n.endsWith(".dmg")) return "mac-dmg";
  return null;
}

// Pick the release the front of the page offers: the newest that is neither a
// draft nor a prerelease — the same rule `/releases/latest` applies, applied
// here so the page can get the archive from the same single API call.
//
// Returns null rather than falling back to "whatever is first". A prerelease
// offered as though it were the current build is worse than no card at all.
export function latestOf(releases) {
  return (releases || []).find((r) => r && !r.draft && !r.prerelease) || null;
}

// The older releases worth listing, in the order GitHub gave them.
//
// **The order is not recomputed.** GitHub returns newest-first, and sorting
// tags here would mean comparing version strings — where "0.9.0" sorts after
// "0.10.0" as text. The API already knows the answer to a question this page
// would get wrong.
//
// A release with no recognised bundle is left out: v0.4.0 and earlier carry
// only the retired egui client (ADR-0089), and listing a version whose only
// asset is `SHA256SUMS.txt` would offer a download that is not one.
export function archiveOf(releases, latest) {
  return (releases || []).filter((r) => {
    if (!r || r.draft || r.prerelease) return false;
    if (latest && r.tag_name === latest.tag_name) return false;
    return (r.assets || []).some((a) => {
      const b = bucketFor((a && a.name) || "");
      return b === "win-setup" || b === "mac-dmg";
    });
  });
}

// Only accept a download URL served by GitHub for this repo, so a surprising
// API payload can never turn into an off-site link.
export function safeUrl(u) {
  try {
    const url = new URL(u);
    return url.protocol === "https:" &&
      (url.host === "github.com" || url.host === "objects.githubusercontent.com")
      ? url.href : null;
  } catch { return null; }
}

// A download is a button, not a panel. These used to be cards with a heading
// and a subtitle each, which is a section's worth of height for two links —
// and on a laptop it pushed the buttons below the fold, so the one thing the
// page is for could not be reached without scrolling.
function dlButton(label, url, primary) {
  return dlLink(label, url, !primary);
}

function dlLink(label, url, secondary) {
  const a = document.createElement("a");
  a.className = "dl" + (secondary ? " secondary" : "");
  a.textContent = label;
  const safe = url && safeUrl(url);
  if (safe) { a.href = safe; } else { a.setAttribute("aria-disabled", "true"); a.textContent = label + " (unavailable)"; }
  return a;
}

function fail() {
  const status = document.getElementById("status");
  status.textContent = "";
  const a = document.createElement("a");
  a.href = RELEASES_URL;
  a.textContent = "Open the latest release on GitHub →";
  status.append("Couldn't load the release list here. ", a);
}

// Draw the "previous versions" table.
//
// Every row is a version someone can install on purpose. The reason this
// exists is the rollback: a release does not work, and the person needs the
// one that did — not the newest.
function renderArchive(releases) {
  const section = document.getElementById("archive");
  const body = document.getElementById("archive-rows");
  if (!section || !body || !releases.length) return;

  for (const rel of releases) {
    const assets = {};
    for (const a of (rel.assets || [])) {
      const b = bucketFor(a.name || "");
      if (b) assets[b] = a.browser_download_url;
    }
    const tr = document.createElement("tr");

    const v = document.createElement("td");
    v.textContent = rel.tag_name || "";
    tr.append(v);

    const win = document.createElement("td");
    if (assets["win-setup"]) win.append(dlLink("Windows .exe", assets["win-setup"], true));
    tr.append(win);

    const mac = document.createElement("td");
    if (assets["mac-dmg"]) mac.append(dlLink("macOS .dmg", assets["mac-dmg"], true));
    tr.append(mac);

    body.append(tr);
  }
  section.hidden = false;
}

// Guarded so `app.js` can be imported by `node --test site/app.test.mjs` for
// the pure helpers above without rendering a page that isn't there.
if (typeof document !== "undefined") boot();

async function boot() {
  try {
    // One call, not two: this list carries the current release and the older
    // ones, and an unauthenticated caller gets ~60 an hour per IP.
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=30`, {
      headers: { "Accept": "application/vnd.github+json" }
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const all = await res.json();
    const rel = latestOf(all);
    if (!rel) { fail(); return; }
    const assets = {};
    for (const a of (rel.assets || [])) {
      const b = bucketFor(a.name || "");
      if (b) assets[b] = a.browser_download_url;
    }

    document.getElementById("version").textContent =
      rel.tag_name ? `— ${rel.tag_name}` : "";

    const cards = document.getElementById("cards");
    if (assets["win-setup"]) {
      cards.append(dlButton("Windows (.exe)", assets["win-setup"], true));
    }
    if (assets["mac-dmg"]) {
      // Both platforms are equally the way in; neither is the fallback. The
      // secondary style exists for a second link *within* one platform, and
      // there is none.
      cards.append(dlButton("macOS (.dmg)", assets["mac-dmg"], true));
    }

    if (!cards.children.length) { fail(); return; }
    document.getElementById("status").hidden = true;
    cards.hidden = false;

    if (assets["sums"] && safeUrl(assets["sums"])) {
      const p = document.getElementById("checksums-link");
      const a = document.createElement("a");
      a.href = safeUrl(assets["sums"]);
      a.textContent = "SHA256SUMS.txt for this release";
      p.append("→ ", a);
    }
    const allLink = document.getElementById("all-releases");
    if (rel.html_url && safeUrl(rel.html_url)) allLink.href = safeUrl(rel.html_url).replace(/\/tag\/.*/, "");

    renderArchive(archiveOf(all, rel));
  } catch (e) {
    fail();
  }
}
