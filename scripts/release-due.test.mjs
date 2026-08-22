import test from "node:test";
import assert from "node:assert/strict";

import { releaseDue, plannedFor, summary } from "./release-due.mjs";

/** A changelog with `body` under `## [Unreleased]` and one released section. */
function changelog(body, released = "0.10.0") {
  return [
    "# Changelog",
    "",
    "Some preamble that is not a release entry.",
    "",
    "## [Unreleased]",
    "",
    body,
    `## [${released}] — 2026-08-20`,
    "",
    "### Added",
    "",
    "- An entry that already shipped and must not be counted.",
    "",
  ].join("\n");
}

test("counts the top-level entries under Unreleased", () => {
  const c = changelog(["### Added", "", "- One.", "- Two.", ""].join("\n"));
  assert.equal(releaseDue(c).count, 2);
});

test("a wrapped entry is one entry, not two", () => {
  const c = changelog(
    ["### Added", "", "- One that runs on", "  to a second line.", ""].join("\n"),
  );
  assert.equal(releaseDue(c).count, 1);
});

test("a nested bullet is not an entry of its own", () => {
  const c = changelog(
    ["### Added", "", "- One.", "  - a sub-point", ""].join("\n"),
  );
  assert.equal(releaseDue(c).count, 1);
});

test("entries of the released section below are not counted", () => {
  assert.equal(releaseDue(changelog("")).count, 0);
});

test("the verdict follows the roadmap's thresholds", () => {
  const of = (n) =>
    releaseDue(
      changelog(
        ["### Fixed", "", ...Array.from({ length: n }, (_, i) => `- Fix ${i}.`), ""].join("\n"),
      ),
    ).verdict;
  assert.equal(of(0), "none");
  assert.equal(of(1), "may");
  assert.equal(of(2), "may");
  assert.equal(of(3), "due");
  assert.equal(of(9), "due");
});

test("an addition makes it a minor bump, fixes alone a patch", () => {
  const added = changelog(["### Added", "", "- New thing.", ""].join("\n"));
  assert.equal(releaseDue(added).bump, "minor");
  assert.equal(releaseDue(added).next, "0.11.0");

  const fixed = changelog(["### Fixed", "", "- Old thing.", ""].join("\n"));
  assert.equal(releaseDue(fixed).bump, "patch");
  assert.equal(releaseDue(fixed).next, "0.10.1");
});

test("Changed counts as an addition for the bump, Security does not", () => {
  const changed = changelog(["### Changed", "", "- Reworked.", ""].join("\n"));
  assert.equal(changed.includes("### Changed"), true);
  assert.equal(releaseDue(changed).bump, "minor");

  const security = changelog(["### Security", "", "- Hardened.", ""].join("\n"));
  assert.equal(releaseDue(security).bump, "patch");
});

test("the current version is the newest released heading", () => {
  const c = changelog(["### Added", "", "- New.", ""].join("\n"), "1.2.3");
  assert.equal(releaseDue(c).current, "1.2.3");
  assert.equal(releaseDue(c).next, "1.3.0");
});

test("CRLF input is read the same as LF", () => {
  const body = ["### Added", "", "- One.", "- Two.", ""].join("\n");
  const lf = releaseDue(changelog(body));
  const crlf = releaseDue(changelog(body).replace(/\n/g, "\r\n"));
  assert.deepEqual(crlf, lf);
});

test("no Unreleased section is not an error", () => {
  const c = ["# Changelog", "", "## [0.10.0] — 2026-08-20", "", "- Shipped.", ""].join("\n");
  const r = releaseDue(c);
  assert.equal(r.count, 0);
  assert.equal(r.verdict, "none");
  assert.equal(r.current, "0.10.0");
});

test("a non-numeric released heading leaves next null rather than guessing", () => {
  const c = changelog(["### Added", "", "- New.", ""].join("\n"), "unreleased-draft");
  assert.equal(releaseDue(c).next, null);
});

/** A roadmap holding the slot table, in the shape docs/roadmap.md uses. */
const ROADMAP = [
  "## Release plan",
  "",
  "### Near slots — one release each",
  "",
  "| Version | Headline | What it carries |",
  "|---|---|---|",
  "| **v0.11** | Connection repair and duplication | Duplicate and Repair actions |",
  "| **v0.12** | A connection list you can steer | Order, search, colour marks |",
  "| **v1.0** | The HTTP contract freezes | Not a feature release |",
  "",
].join("\n");

test("a reserved slot gives the version its headline", () => {
  assert.equal(plannedFor("0.11.0", ROADMAP), "Connection repair and duplication");
  assert.equal(plannedFor("0.12.0", ROADMAP), "A connection list you can steer");
});

test("a slot written as two parts matches a three-part version", () => {
  assert.equal(plannedFor("1.0.0", ROADMAP), "The HTTP contract freezes");
});

test("a version with no slot is null, not an invented headline", () => {
  assert.equal(plannedFor("0.99.0", ROADMAP), null);
});

test("a patch release inherits the slot of its minor", () => {
  assert.equal(plannedFor("0.11.3", ROADMAP), "Connection repair and duplication");
});

test("a missing or empty roadmap is not an error", () => {
  assert.equal(plannedFor("0.11.0", ""), null);
  assert.equal(plannedFor("0.11.0", null), null);
});

test("the summary names the slot when one is reserved", () => {
  const state = { count: 5, verdict: "due", current: "0.10.0", bump: "minor", next: "0.11.0" };
  assert.equal(
    summary(state, ROADMAP),
    "[changelog] 5 unreleased entries — a release is due (0.10.0 -> 0.11.0: Connection repair and duplication)",
  );
});

test("the summary is unchanged when no slot is reserved", () => {
  const state = { count: 1, verdict: "may", current: "0.98.0", bump: "patch", next: "0.98.1" };
  assert.equal(summary(state, ROADMAP), "[changelog] 1 unreleased entry — a release may be cut (0.98.0 -> 0.98.1)");
});
