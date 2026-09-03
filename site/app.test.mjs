// Unit tests for the download page's asset classification. Run with the
// Node built-in runner — no test framework, no install step:
//
//   node --test site/app.test.mjs
//
// Only the pure helpers are imported; `app.js` skips its DOM bootstrap when
// there is no `document`, which is what makes this importable at all.
import { test } from "node:test";
import assert from "node:assert/strict";

import { bucketFor, safeUrl } from "./app.js";

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
