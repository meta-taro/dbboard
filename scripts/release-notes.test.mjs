// What the update dialog says a release contains.
//
// The manifest the updater fetches has always carried "dbboard v0.9.0. See the
// release page for the full changelog." — which is true, and tells nobody
// anything. The dialog is the one place a person sees before deciding to
// restart, and it was the only place in the project where the answer to "what
// changed?" was a pointer instead of an answer.
//
// The source is CHANGELOG.md, which is already written for people. The work is
// choosing *which part*: the whole section runs to several thousand characters
// of prose and would fill the screen. Two shapes are pinned here.
//
//   * A lead paragraph, when the version has one. This project writes a short
//     summary under most version headings ("The first release cut from *using*
//     the previous one..."), and that is exactly the dialog's text, already
//     written and already edited.
//   * The bolded bullet titles, when it does not. Every entry is written as
//     `- **Short title** — long prose`, so the titles alone read as a list of
//     what changed. v0.7.0 has no lead paragraph; without this fallback it
//     would have shipped an empty dialog.
//
// And the output is plain text, because `UpdateNotice.svelte` renders it with
// `{update.notes}` — Svelte escapes it, so `**bold**` and `[text](url)` would
// appear literally. Flattening happens here rather than in the client so the
// client keeps no Markdown knowledge for one string.

import assert from "node:assert/strict";
import { test } from "node:test";
import { extractSection, flatten, releaseNotes, summarise } from "./release-notes.mjs";

const CHANGELOG = `# Changelog

## [Unreleased]

## [0.9.0] — 2026-08-19

Seven MCP verbs, and what they are for. Checking a change by hand means
switching the language, typing the SQL, pressing Run, and then looking.

### Added

- **The UI language can be read and set over MCP** — \`get_ui_locale\` and
  \`set_ui_locale\` ([ADR-0107](docs/decisions.md)). The chosen language now
  lives in \`ui-settings.toml\`.

### Fixed

- **The query toolbar had been frozen** since some earlier release.

## [0.7.0] — 2026-08-11

### Added

- **\`dbboard-mongodb\`** — a new adapter crate.
- **MongoDB is selectable in the desktop client** — the kind list grew a
  tenth entry.

## [0.6.0] — 2026-08-10

Nothing here is bolded.

### Added

- a plain bullet with no title.
`;

test("a version's section is found by its exact number", () => {
  const section = extractSection(CHANGELOG, "0.7.0");
  assert.ok(section.includes("dbboard-mongodb"));
  // The next version's heading terminates it; 0.6.0's text must not leak in.
  assert.ok(!section.includes("Nothing here is bolded"));
});

test("a version that is not in the file is reported as absent, not as empty", () => {
  assert.equal(extractSection(CHANGELOG, "9.9.9"), null);
  assert.equal(releaseNotes(CHANGELOG, "9.9.9"), null);
});

// `0.7.0` must not match `10.7.0`, and `0.7` must not match `0.7.0`. The
// heading is `## [x.y.z] — date`, so the bracket is what makes it exact.
test("a version number is not matched as a substring of another", () => {
  const shifted = CHANGELOG.replace("## [0.7.0]", "## [10.7.0]");
  assert.equal(extractSection(shifted, "0.7.0"), null);
  assert.ok(extractSection(shifted, "10.7.0").includes("dbboard-mongodb"));
});

test("the lead paragraph is the notes when the version has one", () => {
  const notes = releaseNotes(CHANGELOG, "0.9.0");
  assert.ok(notes.startsWith("Seven MCP verbs, and what they are for."));
  // The lead stops at the first `###`; the entries are not appended to it.
  assert.ok(!notes.includes("get_ui_locale"));
});

test("a hard-wrapped lead paragraph becomes one line", () => {
  const notes = releaseNotes(CHANGELOG, "0.9.0");
  assert.ok(!notes.includes("\n"), `still wrapped: ${JSON.stringify(notes)}`);
  assert.ok(notes.includes("by hand means switching the language"));
});

test("a version with no lead paragraph falls back to its bullet titles", () => {
  const notes = releaseNotes(CHANGELOG, "0.7.0");
  assert.equal(notes, "Added\n• dbboard-mongodb\n• MongoDB is selectable in the desktop client");
});

test("a section with neither a lead nor a bolded title yields nothing", () => {
  // Better an absent field the workflow can fall back on than a heading with
  // no content under it.
  assert.equal(releaseNotes(CHANGELOG, "0.6.0"), "Nothing here is bolded.");
  assert.equal(summarise("### Added\n\n- a plain bullet.\n"), null);
});

test("Markdown that would show up literally is removed", () => {
  assert.equal(flatten("**bold** and *italic* and `code`"), "bold and italic and code");
  assert.equal(flatten("see [ADR-0107](docs/decisions.md) for why"), "see ADR-0107 for why");
  assert.equal(flatten("a  b\t c"), "a b c");
});

test("an em dash separator inside a bullet title is kept out of the title", () => {
  const notes = releaseNotes(CHANGELOG, "0.9.0".replace("0.9.0", "0.7.0"));
  assert.ok(!notes.includes("—"), `separator leaked: ${JSON.stringify(notes)}`);
});
