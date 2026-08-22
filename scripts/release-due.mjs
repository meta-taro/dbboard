// How much unreleased content CHANGELOG.md is holding, and what the roadmap's
// release trigger says about it.
//
// docs/roadmap.md ("Releases are not planned here", ADR-0110) does not schedule
// releases: it derives them from content. One unreleased entry means a release
// *may* be cut; three mean one is *due*. That rule was only ever reachable by
// running an awk one-liner out of the roadmap, so in practice nobody saw the
// counter move and the next version looked like it was never coming. This
// prints the same rule where the work already happens — the pre-push hook.
//
// Run directly to print one line and exit 0. It must never fail a push: a
// release counter that can block work would be worse than no counter.
//
// See scripts/release-due.test.mjs.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const VERSION_HEADING = /^## \[([^\]]+)\]/;
const SECTION_HEADING = /^### (.+?)\s*$/;
/** A top-level entry. Indented bullets are continuations, not entries. */
const ENTRY = /^- /;

/** Sections whose presence makes the next release a minor rather than a patch. */
const ADDITIVE = new Set(["Added", "Changed"]);

/**
 * Read the unreleased state of `changelog`.
 *
 * Returns `{ count, verdict, current, bump, next }`:
 * - `count`   — top-level entries under `## [Unreleased]`
 * - `verdict` — `"none"` | `"may"` | `"due"`, the roadmap's thresholds
 * - `current` — the newest released version heading, or `null`
 * - `bump`    — `"minor"` if anything was added, else `"patch"`
 * - `next`    — `current` with `bump` applied, or `null` when `current` is not
 *               three numbers (a heading this cannot parse is left alone
 *               rather than guessed at)
 */
export function releaseDue(changelog) {
  const lines = changelog.split(/\r?\n/);

  let inUnreleased = false;
  let current = null;
  let section = null;
  let count = 0;
  let additive = 0;

  for (const line of lines) {
    const heading = VERSION_HEADING.exec(line);
    if (heading) {
      if (heading[1] === "Unreleased") {
        inUnreleased = true;
        section = null;
      } else {
        inUnreleased = false;
        current ??= heading[1];
      }
      continue;
    }
    if (!inUnreleased) continue;

    const sub = SECTION_HEADING.exec(line);
    if (sub) {
      section = sub[1];
      continue;
    }
    if (!ENTRY.test(line)) continue;

    count += 1;
    if (section !== null && ADDITIVE.has(section)) additive += 1;
  }

  const bump = additive > 0 ? "minor" : "patch";
  return {
    count,
    verdict: count === 0 ? "none" : count < 3 ? "may" : "due",
    current,
    bump,
    next: nextVersion(current, bump),
  };
}

/** `current` with `bump` applied, or null when it is not `major.minor.patch`. */
function nextVersion(current, bump) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(current ?? "");
  if (!m) return null;
  const [major, minor, patch] = m.slice(1).map(Number);
  return bump === "minor"
    ? `${major}.${minor + 1}.0`
    : `${major}.${minor}.${patch + 1}`;
}

/** The one line the hook prints. */
/** A row of the roadmap's slot table: `| **v0.11** | Headline | … |`. */
const SLOT = /^\|\s*\*\*v(\d+)\.(\d+)\*\*\s*\|\s*([^|]+?)\s*\|/;

/**
 * The headline reserved for `version` in `roadmap`, or `null`.
 *
 * Slots are written `major.minor` because a patch ships whatever its minor was
 * about; `0.11.3` therefore answers with `0.11`'s headline. A version with no
 * row returns null rather than a guess — an invented headline on a release is
 * worse than none, because it reads as a promise somebody made.
 */
export function plannedFor(version, roadmap) {
  if (!roadmap || !version) return null;
  const parts = /^(\d+)\.(\d+)/.exec(version);
  if (!parts) return null;
  for (const line of roadmap.split(/\r?\n/)) {
    const slot = SLOT.exec(line);
    if (slot && slot[1] === parts[1] && slot[2] === parts[2]) return slot[3];
  }
  return null;
}

export function summary(state, roadmap) {
  const { count, verdict, current, next } = state;
  if (verdict === "none") return "[changelog] nothing unreleased yet";
  const entries = `${count} unreleased ${count === 1 ? "entry" : "entries"}`;
  const verb = verdict === "due" ? "a release is due" : "a release may be cut";
  const slot = plannedFor(next, roadmap);
  const headline = slot ? `: ${slot}` : "";
  const target = current && next ? ` (${current} -> ${next}${headline})` : "";
  return `[changelog] ${entries} — ${verb}${target}`;
}

const invokedDirectly =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedDirectly) {
  // Any failure here is silence, not an error: this runs inside a git hook.
  try {
    const here = dirname(fileURLToPath(import.meta.url));
    const path = process.argv[2] ?? join(here, "..", "CHANGELOG.md");
    // The roadmap is optional: without it the line loses the headline, not
    // the count, so a renamed or missing roadmap degrades instead of going
    // silent.
    let roadmap = null;
    try {
      roadmap = readFileSync(join(here, "..", "docs", "roadmap.md"), "utf8");
    } catch {
      /* no plan on file, no headline */
    }
    console.log(summary(releaseDue(readFileSync(path, "utf8")), roadmap));
  } catch {
    /* no changelog, no counter, no complaint */
  }
}
