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
// own release CI (ADR-0044): dbboard-windows-x86_64.exe,
// dbboard-<v>-x86_64.msi, dbboard-macos-universal-<v>.dmg, SHA256SUMS.txt.
function bucketFor(name) {
  const n = name.toLowerCase();
  if (n.endsWith(".exe")) return "win-exe";
  if (n.endsWith(".msi")) return "win-msi";
  if (n.endsWith(".dmg")) return "mac-dmg";
  if (n === "sha256sums.txt") return "sums";
  return null;
}

// Only accept a download URL served by GitHub for this repo, so a surprising
// API payload can never turn into an off-site link.
function safeUrl(u) {
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

(async () => {
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
    if (assets["win-exe"] || assets["win-msi"]) {
      cards.append(card(
        "Windows", "64-bit (x86_64)",
        { label: "Download .exe", url: assets["win-exe"] },
        assets["win-msi"] ? { label: "Installer (.msi)", url: assets["win-msi"] } : null
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
})();
