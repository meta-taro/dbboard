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

function card(title, sub, primary, secondary) {
  const el = document.createElement("div");
  el.className = "card";
  const h = document.createElement("h3"); h.textContent = title; el.append(h);
  const s = document.createElement("p"); s.className = "sub"; s.textContent = sub; el.append(s);
  el.append(dlLink(primary.label, primary.url, false));
  if (secondary) el.append(dlLink(secondary.label, secondary.url, true));
  return el;
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

// Guarded so `app.js` can be imported by `node --test site/app.test.mjs` for
// the pure helpers above without rendering a page that isn't there.
if (typeof document !== "undefined") boot();

async function boot() {
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers: { "Accept": "application/vnd.github+json" }
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const rel = await res.json();
    const assets = {};
    for (const a of (rel.assets || [])) {
      const b = bucketFor(a.name || "");
      if (b) assets[b] = a.browser_download_url;
    }

    document.getElementById("version").textContent =
      rel.tag_name ? `— ${rel.tag_name}` : "";

    const cards = document.getElementById("cards");
    if (assets["win-setup"]) {
      cards.append(card(
        "Windows", "64-bit (x86_64)",
        { label: "Download installer (.exe)", url: assets["win-setup"] }, null
      ));
    }
    if (assets["mac-dmg"]) {
      cards.append(card(
        "macOS", "Universal (.dmg)",
        { label: "Download .dmg", url: assets["mac-dmg"] }, null
      ));
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
    const all = document.getElementById("all-releases");
    if (rel.html_url && safeUrl(rel.html_url)) all.href = safeUrl(rel.html_url).replace(/\/tag\/.*/, "");
  } catch (e) {
    fail();
  }
}
