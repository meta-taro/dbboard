// Unit tests for the download page's asset classification. Run with the
// Node built-in runner — no test framework, no install step:
//
//   node --test site/app.test.mjs
//
// Only the pure helpers are imported; `app.js` skips its DOM bootstrap when
// there is no `document`, which is what makes this importable at all.
import { test } from "node:test";
import assert from "node:assert/strict";

import { archiveOf, bucketFor, latestOf, safeUrl } from "./app.js";

// The exact asset names v0.4.0 published — the last release that carried both
// clients. The page must pick the Tauri build out of this list unambiguously
// (issue #135: keying on the extension alone made the answer depend on the
// order the Releases API happened to return assets in).
const V0_4_0 = [
  "dbboard-0.4.0-x86_64.msi",
  "dbboard-desktop.app.tar.gz",
  "dbboard-desktop.app.tar.gz.sig",
  "dbboard-desktop_0.4.0_universal.dmg",
  "dbboard-desktop_0.4.0_x64-setup.exe",
  "dbboard-desktop_0.4.0_x64-setup.exe.sig",
  "dbboard-macos-universal-0.4.0.dmg",
  "dbboard-windows-x86_64.exe",
  "latest.json",
  "SHA256SUMS.txt",
];

test("the Tauri bundles are the ones offered", () => {
  assert.equal(bucketFor("dbboard-desktop_0.4.0_x64-setup.exe"), "win-setup");
  assert.equal(bucketFor("dbboard-desktop_0.4.0_universal.dmg"), "mac-dmg");
  assert.equal(bucketFor("SHA256SUMS.txt"), "sums");
});

test("the renamed bundles are offered, and the old ones keep working", () => {
  // From v0.15.0 the product is called `dbboard`, so Tauri names its bundles
  // `dbboard_<version>_…`. Releases v0.5.0 through v0.14.0 shipped the same
  // client as `dbboard-desktop_…`, and the page lists whichever release a
  // visitor is looking at, so both spellings have to be recognised.
  assert.equal(bucketFor("dbboard_0.15.0_x64-setup.exe"), "win-setup");
  assert.equal(bucketFor("dbboard_0.15.0_universal.dmg"), "mac-dmg");
  assert.equal(bucketFor("dbboard-desktop_0.14.0_universal.dmg"), "mac-dmg");
});

test("the underscore is what separates the new name from the retired one", () => {
  // The new prefix cannot simply be `dbboard`: the egui client the project
  // retired in ADR-0089 was called exactly that, and its assets are still
  // attached to the releases up to v0.4.0. Tauri puts an underscore before
  // the version, the egui build used a hyphen, and that is the whole
  // distinction — so it is asserted rather than left to be noticed.
  assert.equal(bucketFor("dbboard-macos-universal-0.4.0.dmg"), null);
  assert.equal(bucketFor("dbboard_0.15.0_universal.dmg"), "mac-dmg");
});

test("the retired egui assets are ignored", () => {
  // A release from before ADR-0089 still carries these. Offering one would
  // hand a visitor the client that no longer ships.
  assert.equal(bucketFor("dbboard-windows-x86_64.exe"), null);
  assert.equal(bucketFor("dbboard-0.4.0-x86_64.msi"), null);
  assert.equal(bucketFor("dbboard-macos-universal-0.4.0.dmg"), null);
});

test("the MCP server binaries are not desktop downloads", () => {
  // Published from the same tag but a different product (ADR-0046). The
  // Windows one ends in `.exe`, so an extension-keyed classifier would offer
  // a headless stdio server to someone clicking "Download for Windows".
  assert.equal(bucketFor("dbboard-mcp-windows-x86_64.exe"), null);
  assert.equal(bucketFor("dbboard-mcp-macos-universal"), null);
});

test("the updater's own artifacts are not downloads", () => {
  // `.app.tar.gz` and the signatures exist for tauri-plugin-updater, not for
  // a human clicking a button.
  assert.equal(bucketFor("dbboard-desktop.app.tar.gz"), null);
  assert.equal(bucketFor("dbboard-desktop.app.tar.gz.sig"), null);
  assert.equal(bucketFor("dbboard-desktop_0.4.0_x64-setup.exe.sig"), null);
  assert.equal(bucketFor("latest.json"), null);
});

test("a whole release resolves to exactly one asset per bucket", () => {
  // The regression guard for #135: run the real v0.4.0 list through the
  // classifier and assert no bucket is claimed twice, in any asset order.
  for (const order of [V0_4_0, [...V0_4_0].reverse()]) {
    const seen = new Map();
    for (const name of order) {
      const b = bucketFor(name);
      if (!b) continue;
      assert.equal(seen.has(b), false, `bucket ${b} claimed twice by ${name}`);
      seen.set(b, name);
    }
    assert.deepEqual([...seen.keys()].sort(), ["mac-dmg", "sums", "win-setup"]);
  }
});

test("only GitHub-served download URLs are accepted", () => {
  const ok = "https://github.com/meta-taro/dbboard/releases/download/v0.4.0/x.exe";
  assert.equal(safeUrl(ok), ok);
  assert.equal(safeUrl("https://objects.githubusercontent.com/x"), "https://objects.githubusercontent.com/x");
  assert.equal(safeUrl("http://github.com/x"), null);
  assert.equal(safeUrl("https://evil.example/x"), null);
  assert.equal(safeUrl("not a url"), null);
});

// --- Installing a specific version (ADR-0155) -------------------------------
//
// The page used to call `/releases/latest` and offer exactly one version. That
// is the wrong shape for the case this is actually for: a release does not
// work, and the person wants the one that did. Blender's download page has
// carried a "previous versions" archive for years for the same reason.
//
// Switching to `/releases` keeps it to **one** API call rather than two — the
// list carries the latest release as well as the old ones. Unauthenticated
// callers get ~60 requests an hour per IP, and this page must work for someone
// behind a shared address.

test("the latest release is the newest that is neither a draft nor a prerelease", () => {
  const picked = latestOf([
    { tag_name: "v0.18.0", draft: true, prerelease: false },
    { tag_name: "v0.18.0-rc1", draft: false, prerelease: true },
    { tag_name: "v0.17.0", draft: false, prerelease: false },
    { tag_name: "v0.16.1", draft: false, prerelease: false },
  ]);
  assert.equal(picked.tag_name, "v0.17.0");
});

test("no usable release is null rather than a guess", () => {
  assert.equal(latestOf([]), null);
  assert.equal(latestOf([{ tag_name: "v1.0.0-rc1", draft: false, prerelease: true }]), null);
});

// GitHub returns releases newest-first. The page keeps that order rather than
// sorting the tags itself: comparing version strings is a trap ("0.9.0" sorts
// after "0.10.0" as text) and the API already knows the answer.
test("the archive keeps the order the API gave, minus the one already offered", () => {
  const releases = [
    { tag_name: "v0.17.0", draft: false, prerelease: false, assets: [{ name: "dbboard-desktop_0.17.0_x64-setup.exe" }] },
    { tag_name: "v0.16.1", draft: false, prerelease: false, assets: [{ name: "dbboard-desktop_0.16.1_x64-setup.exe" }] },
    { tag_name: "v0.16.0", draft: false, prerelease: false, assets: [{ name: "dbboard-desktop_0.16.0_universal.dmg" }] },
  ];
  const rows = archiveOf(releases, latestOf(releases));
  assert.deepEqual(rows.map((r) => r.tag_name), ["v0.16.1", "v0.16.0"]);
});

test("a release with nothing installable is not listed as installable", () => {
  const releases = [
    { tag_name: "v0.17.0", draft: false, prerelease: false, assets: [{ name: "dbboard-desktop_0.17.0_x64-setup.exe" }] },
    // A release carrying only the retired egui client (ADR-0089), which
    // `bucketFor` refuses. Releases up to v0.4.0 carry those names *alongside*
    // the Tauri bundles, so the real v0.4.0 is listed — it is the asset that
    // decides, not the version.
    { tag_name: "v0.4.0", draft: false, prerelease: false, assets: [{ name: "dbboard-windows-x86_64.exe" }] },
    // A release whose only asset is the checksum file offers no download.
    { tag_name: "v0.3.0", draft: false, prerelease: false, assets: [{ name: "SHA256SUMS.txt" }] },
  ];
  assert.deepEqual(archiveOf(releases, latestOf(releases)).map((r) => r.tag_name), []);
});

test("drafts and prereleases stay out of the archive too", () => {
  const releases = [
    { tag_name: "v0.17.0", draft: false, prerelease: false, assets: [{ name: "dbboard-desktop_0.17.0_x64-setup.exe" }] },
    { tag_name: "v0.17.1", draft: true, prerelease: false, assets: [{ name: "dbboard-desktop_0.17.1_x64-setup.exe" }] },
    { tag_name: "v0.18.0-rc1", draft: false, prerelease: true, assets: [{ name: "dbboard-desktop_0.18.0_x64-setup.exe" }] },
  ];
  assert.deepEqual(archiveOf(releases, latestOf(releases)).map((r) => r.tag_name), []);
});
