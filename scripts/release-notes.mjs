// The text the update dialog shows for a release, taken from CHANGELOG.md.
//
// Called by .github/workflows/release.yml while assembling latest.json, the
// manifest tauri-plugin-updater fetches. Prints the notes on stdout and exits
// 0; prints nothing and exits 0 when the version has no usable section, so the
// workflow can fall back to its boilerplate rather than fail a release over a
// missing paragraph. A release must never be blocked by its own description.
//
// See scripts/release-notes.test.mjs for why the output is shaped this way.

const HEADING = /^## \[([^\]]+)\]/;

/**
 * The body of one version's section, without its heading, or null.
 *
 * The version is matched inside the brackets rather than anywhere on the line,
 * so `0.7.0` does not match the heading of `10.7.0`.
 */
export function extractSection(changelog, version) {
  const lines = changelog.split(/\r?\n/);
  let start = -1;
  for (let i = 0; i < lines.length; i += 1) {
    const m = HEADING.exec(lines[i]);
    if (!m) continue;
    if (start >= 0) return lines.slice(start, i).join("\n");
    if (m[1] === version) start = i + 1;
  }
  return start >= 0 ? lines.slice(start).join("\n") : null;
}

/** Markdown that would otherwise be shown literally by `{update.notes}`. */
export function flatten(markdown) {
  return markdown
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1") // [text](url) -> text
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * The dialog text for a section body, or null when it has nothing to say.
 *
 * Prefers the lead paragraph — the prose between the version heading and the
 * first `###` — because it is already written for a person. Falls back to the
 * bolded bullet titles grouped under their headings, which is what a version
 * without a lead has instead.
 */
export function summarise(section) {
  const lead = flatten(section.split(/^### /m)[0]);
  if (lead) return lead;

  const out = [];
  for (const group of section.split(/^### /m).slice(1)) {
    const [heading, ...rest] = group.split(/\r?\n/);
    // Only the bolded opening of a bullet: `- **Short title** — long prose`.
    // A bullet without one is prose, and prose belongs in the release page.
    const titles = [...rest.join("\n").matchAll(/^- \*\*(.+?)\*\*/gm)].map((m) => flatten(m[1]));
    if (titles.length > 0) out.push([flatten(heading), ...titles.map((t) => `• ${t}`)].join("\n"));
  }
  return out.length > 0 ? out.join("\n\n") : null;
}

/** The dialog text for a version, or null. */
export function releaseNotes(changelog, version) {
  const section = extractSection(changelog, version);
  return section === null ? null : summarise(section);
}

// CLI: node scripts/release-notes.mjs <changelog-path> <version>
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const { readFileSync } = await import("node:fs");
  const [path, version] = process.argv.slice(2);
  if (!path || !version) {
    console.error("usage: release-notes.mjs <changelog-path> <version>");
    process.exit(2);
  }
  const notes = releaseNotes(readFileSync(path, "utf8"), version);
  if (notes) process.stdout.write(notes);
}
