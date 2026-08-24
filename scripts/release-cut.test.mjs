import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { cutHeading, bumpCargoToml, bumpJson } from "./release-cut.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const changelog = (unreleased) =>
  ["# Changelog", "", unreleased, "", "### Added", "", "- a thing", "",
   "## [0.10.0] — 2026-08-20 — Something earlier", ""].join("\n");

test("the headline moves down with the version it described", () => {
  const out = cutHeading(
    changelog("## [Unreleased] — Connection repair and duplication"),
    "0.11.0",
    "2026-08-22",
  );
  assert.match(out, /^## \[Unreleased\]$/m);
  assert.match(out, /^## \[0\.11\.0\] — 2026-08-22 — Connection repair and duplication$/m);
});

test("the entries stay under the version, not under Unreleased", () => {
  const out = cutHeading(
    changelog("## [Unreleased] — Connection repair and duplication"),
    "0.11.0",
    "2026-08-22",
  );
  const version = out.indexOf("## [0.11.0]");
  const entry = out.indexOf("- a thing");
  assert.ok(version < entry, "the released heading must come before its entries");
  assert.ok(out.indexOf("## [Unreleased]") < version);
});

test("a version without a headline still gets its date", () => {
  const out = cutHeading(changelog("## [Unreleased]"), "0.10.1", "2026-08-22");
  assert.match(out, /^## \[0\.10\.1\] — 2026-08-22$/m);
});

test("cutting twice is refused rather than nesting two headings", () => {
  const once = cutHeading(changelog("## [Unreleased] — A"), "0.11.0", "2026-08-22");
  assert.throws(() => cutHeading(once, "0.12.0", "2026-08-23"), /nothing unreleased/i);
});

test("a changelog with no Unreleased heading is refused", () => {
  assert.throws(
    () => cutHeading("# Changelog\n\n## [0.10.0] — 2026-08-20\n", "0.11.0", "2026-08-22"),
    /\[Unreleased\]/,
  );
});

test("CRLF survives the cut", () => {
  const out = cutHeading(
    changelog("## [Unreleased] — A headline").replaceAll("\n", "\r\n"),
    "0.11.0",
    "2026-08-22",
  );
  assert.ok(out.includes("\r\n"));
  assert.ok(!/[^\r]\n/.test(out), "no bare LF may be introduced");
});

test("only the workspace version moves, not a dependency's", () => {
  const toml = [
    "[workspace]",
    'members = ["crates/a"]',
    "",
    "[workspace.package]",
    'version = "0.10.0"',
    'edition = "2021"',
    "",
    "[workspace.dependencies]",
    'tokio = { version = "1", features = ["macros"] }',
    "",
  ].join("\n");
  const out = bumpCargoToml(toml, "0.11.0");
  assert.match(out, /^version = "0\.11\.0"$/m);
  assert.match(out, /tokio = \{ version = "1"/);
});

test("a Cargo.toml with no workspace version is refused", () => {
  assert.throws(() => bumpCargoToml('[package]\nversion = "0.1.0"\n', "0.2.0"), /workspace\.package/);
});

test("the json version moves and the rest of the file is untouched", () => {
  const json = '{\n  "name": "x",\n  "version": "0.10.0",\n  "private": true\n}\n';
  assert.equal(
    bumpJson(json, "0.11.0"),
    '{\n  "name": "x",\n  "version": "0.11.0",\n  "private": true\n}\n',
  );
});

test("a json with no version field is refused", () => {
  assert.throws(() => bumpJson('{\n  "name": "x"\n}\n', "0.11.0"), /"version"/);
});

test("the four files on disk are all shaped the way the cut expects", () => {
  const read = (p) => readFileSync(join(root, p), "utf8");
  // Between a release and the next entry there is nothing to cut, and
  // refusing is the correct answer then — so either outcome passes, and a
  // changelog the cut cannot parse at all fails.
  try {
    cutHeading(read("CHANGELOG.md"), "9.9.9", "2026-01-01");
  } catch (err) {
    assert.match(err.message, /nothing unreleased/i);
  }
  assert.doesNotThrow(() => bumpCargoToml(read("Cargo.toml"), "9.9.9"));
  assert.doesNotThrow(() => bumpJson(read("apps/desktop/package.json"), "9.9.9"));
  assert.doesNotThrow(() =>
    bumpJson(read("apps/desktop/src-tauri/tauri.conf.json"), "9.9.9"),
  );
});
