import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { planDrift } from "./release-due.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const read = (...p) => readFileSync(join(here, "..", ...p), "utf8");

const ROADMAP = [
  "### Near slots — one release each",
  "",
  "| Version | Headline | What it carries |",
  "|---|---|---|",
  "| **v0.11** | Connection repair | Duplicate and Repair |",
  "| **v0.12** | A list you can steer | Order and search |",
].join("\n");

const changelog = (unreleased, released = "## [0.10.0] — 2026-08-20") =>
  ["# Changelog", "", unreleased, "", "- something", "", released].join("\n");

test("a plan that matches the changelog has drifted nowhere", () => {
  const c = changelog("## [Unreleased] — Connection repair");
  assert.deepEqual(planDrift(ROADMAP, c), []);
});

test("a slot for a version already released is stale", () => {
  const c = changelog("## [Unreleased] — Connection repair", "## [0.11.0] — 2026-08-22");
  const found = planDrift(ROADMAP, c);
  assert.equal(found.length, 1);
  assert.match(found[0], /v0\.11/);
  assert.match(found[0], /released/);
});

test("unreleased content with no headline is drift", () => {
  const found = planDrift(ROADMAP, changelog("## [Unreleased]"));
  assert.equal(found.length, 1);
  assert.match(found[0], /Connection repair/);
});

test("a headline that no longer matches its slot is drift", () => {
  const found = planDrift(ROADMAP, changelog("## [Unreleased] — Something else"));
  assert.equal(found.length, 1);
  assert.match(found[0], /Something else/);
});

test("an empty Unreleased needs no headline", () => {
  const c = ["# Changelog", "", "## [Unreleased]", "", "## [0.10.0] — 2026-08-20"].join("\n");
  assert.deepEqual(planDrift(ROADMAP, c), []);
});

test("a roadmap with no slot table is not a drift report", () => {
  assert.deepEqual(planDrift("# Roadmap\n\nNothing planned.\n", changelog("## [Unreleased]")), []);
});

test("the roadmap and the changelog on file agree", () => {
  assert.deepEqual(planDrift(read("docs", "roadmap.md"), read("CHANGELOG.md")), []);
});
