// Turn "a release is due" into the four edits that make one.
//
// Cutting a release here has always been five files done by hand: the
// CHANGELOG heading, the workspace version, two manifests that repeat it, and
// Cargo.lock as a consequence. Nothing about that is hard, which is exactly
// why it was never written down — and a ritual that lives only in whoever did
// it last is a reason to put releasing off. dbboard is used while it is being
// built, so a release deferred is an improvement withheld.
//
// This does the four text edits and stops. It does not commit, does not tag,
// and does not push: deciding to release and pushing the tag stay human
// (ADR-0121), and the point of the script is to make the mechanical half not
// worth postponing, not to take the decision away.
//
//   node scripts/release-cut.mjs            # version from the trigger
//   node scripts/release-cut.mjs 0.12.0     # or say which
//
// See scripts/release-cut.test.mjs.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { releaseDue, plannedFor } from "./release-due.mjs";

/** `## [Unreleased]`, with the headline that follows it when there is one. */
const UNRELEASED = /^## \[Unreleased\][ \t]*(?:[—-][ \t]*(.+?))?[ \t]*$/m;

/**
 * Move `## [Unreleased]` down to `version`, keeping its headline and its
 * entries together, and leave a bare `## [Unreleased]` above for what comes
 * next.
 *
 * Refuses when there is nothing under the heading. Cutting an empty section
 * would produce a version whose changelog says nothing shipped, and the most
 * likely way to reach that state is running this twice.
 */
export function cutHeading(changelog, version, date) {
  const found = UNRELEASED.exec(changelog);
  if (!found) throw new Error("CHANGELOG.md has no `## [Unreleased]` heading");

  const after = changelog.slice(found.index + found[0].length);
  const body = after.split(/^## \[/m)[0];
  if (body.trim() === "") {
    throw new Error(`nothing unreleased to cut into ${version}`);
  }

  // Whatever this file already uses. A release must not restate every line of
  // the changelog as a change to itself.
  const eol = changelog.includes("\r\n") ? "\r\n" : "\n";
  const headline = found[1] ? ` — ${found[1]}` : "";
  const replacement =
    `## [Unreleased]${eol}${eol}## [${version}] — ${date}${headline}`;

  return (
    changelog.slice(0, found.index) +
    replacement +
    changelog.slice(found.index + found[0].length)
  );
}

/**
 * The version under `[workspace.package]` — the one every crate inherits.
 *
 * Scoped to that section on purpose: `[workspace.dependencies]` below it is
 * full of `version = "1"`, and a bump that caught one of those would be a
 * dependency change wearing a release's commit message.
 */
export function bumpCargoToml(toml, version) {
  const section = /^\[workspace\.package\][ \t]*$/m.exec(toml);
  if (!section) throw new Error("Cargo.toml has no [workspace.package] section");

  const from = section.index + section[0].length;
  const rest = toml.slice(from);
  const end = /^\[/m.exec(rest);
  const within = rest.slice(0, end ? end.index : rest.length);

  const line = /^version = "[^"]*"$/m.exec(within);
  if (!line) throw new Error("[workspace.package] has no version line");

  return (
    toml.slice(0, from + line.index) +
    `version = "${version}"` +
    toml.slice(from + line.index + line[0].length)
  );
}

/**
 * The `"version"` field of a manifest, edited as text rather than reparsed.
 *
 * `JSON.parse` then `JSON.stringify` would reformat both manifests and lose
 * their key order, turning a one-line release into a whole-file diff.
 */
export function bumpJson(json, version) {
  const field = /^([ \t]*"version"[ \t]*:[ \t]*")[^"]*(")/m.exec(json);
  if (!field) throw new Error('the manifest has no "version" field');
  return (
    json.slice(0, field.index) +
    field[1] +
    version +
    field[2] +
    json.slice(field.index + field[0].length)
  );
}

/** Files a release touches, and how each one is edited. */
const TARGETS = [
  ["Cargo.toml", bumpCargoToml],
  ["apps/desktop/package.json", bumpJson],
  ["apps/desktop/src-tauri/tauri.conf.json", bumpJson],
];

const invokedDirectly =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedDirectly) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const read = (p) => readFileSync(join(root, p), "utf8");

  const changelog = read("CHANGELOG.md");
  const state = releaseDue(changelog);
  const version = process.argv[2] ?? state.next;

  if (!version) {
    console.error("[release] nothing unreleased — there is no version to cut");
    process.exit(1);
  }

  // Today, in the repository's own format. Taken from the clock rather than
  // asked for: a release dated anything but the day it was cut is a mistake
  // nobody notices until the changelog is read as history.
  const date = new Date().toISOString().slice(0, 10);

  try {
    writeFileSync(join(root, "CHANGELOG.md"), cutHeading(changelog, version, date));
    for (const [path, edit] of TARGETS) {
      writeFileSync(join(root, path), edit(read(path), version));
    }
  } catch (err) {
    console.error(`[release] ${err.message}`);
    process.exit(1);
  }

  let roadmap = null;
  try {
    roadmap = read("docs/roadmap.md");
  } catch {
    // Optional: without it the reminder below loses the headline, not the cut.
  }
  const slot = roadmap ? plannedFor(version, roadmap) : null;

  console.log(`[release] v${version} — ${date}${slot ? ` — ${slot}` : ""}`);
  console.log("[release] edited CHANGELOG.md, Cargo.toml, and both manifests");
  console.log("[release] left to do:");
  console.log("[release]   cargo check --workspace   # Cargo.lock follows");
  console.log(`[release]   git commit -am "chore: release v${version}"`);
  console.log(`[release]   git tag v${version}       # and push it — that is the release`);
}
