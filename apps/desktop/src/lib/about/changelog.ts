// CHANGELOG.md, read as structure so the About dialog can say what the
// running build brought (ADR-0137).
//
// Pure text in, structure out: no Tauri and no file access, so the parser is
// testable on its own and the bundling decision lives next door in
// `bundled.ts`.
//
// The formatting rules deliberately mirror `scripts/release-notes.mjs`, which
// produces the one-line summary the *update* dialog shows. Two readers of one
// file should not disagree about what a bullet says, and the notes a person
// reads before updating should be recognisable as the notes they read after.
// What differs is only how much survives: the script flattens a release to a
// sentence for a notification, this keeps the groups and the bullets because
// a dialog someone opened on purpose has room for them.

/** One bullet under a `### Added`-style heading. */
export interface ReleaseChange {
  /** 0 for a top-level bullet, 1 for one nested under it. */
  depth: number;
  /** The bolded opening of `- **Short title** — prose`, or null without one. */
  title: string | null;
  /** The rest of the bullet, formatting removed. May be empty. */
  body: string;
}

/** A `### Added` / `### Fixed` group and the bullets under it. */
export interface ReleaseGroup {
  heading: string;
  changes: ReleaseChange[];
}

/** One `## [version]` section. */
export interface Release {
  version: string;
  /** `null` for `[Unreleased]`, which has no date yet. */
  date: string | null;
  /** The slot's headline, where the heading carries one. */
  headline: string | null;
  /** Prose between the heading and the first `###`, formatting removed. */
  lead: string | null;
  groups: ReleaseGroup[];
}

const VERSION_HEADING = /^## \[([^\]]+)\](.*)$/;
const GROUP_HEADING = /^### +(.+?)\s*$/;
const BULLET = /^(\s*)[-*] +(.*)$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
// A link-reference definition — `[0.5.0]: https://…` — sits at the foot of
// the file, inside the oldest version's section. It is machinery, not a
// change, and would otherwise be read as that release's closing prose.
const LINK_DEFINITION = /^\[[^\]]+\]:\s/;

/**
 * Strip the inline markdown that has no meaning without a renderer.
 *
 * Same rules, same order as `flatten` in `scripts/release-notes.mjs`.
 */
export function flatten(markdown: string): string {
  return markdown
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1') // [text](url) -> text
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/\*([^*]+)\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\s+/g, ' ')
    .trim();
}

/** The bolded opening of a bullet and what follows it. */
function splitBullet(text: string): { title: string | null; body: string } {
  const bold = /^\*\*(.+?)\*\*\s*(.*)$/s.exec(text);
  if (!bold) return { title: null, body: flatten(text) };
  // `- **Title** — prose` and `- **Title**: prose` both read as a title with
  // prose after it; the separator belongs to neither half.
  const rest = bold[2].replace(/^\s*[—–:-]\s*/, '');
  return { title: flatten(bold[1]), body: flatten(rest) };
}

/** Every `## [version]` section, newest first, in the order the file has them. */
export function parseChangelog(text: string): Release[] {
  const releases: Release[] = [];
  let release: Release | null = null;
  let group: ReleaseGroup | null = null;
  let lead: string[] = [];
  // A bullet wraps over as many lines as it needs; its continuation is
  // indented and belongs to the bullet above rather than to the group.
  let open: { change: ReleaseChange; raw: string[] } | null = null;

  const closeBullet = () => {
    if (!open) return;
    const { title, body } = splitBullet(open.raw.join('\n'));
    open.change.title = title;
    open.change.body = body;
    open = null;
  };
  const closeRelease = () => {
    closeBullet();
    if (!release) return;
    const prose = flatten(lead.join('\n'));
    release.lead = prose === '' ? null : prose;
    releases.push(release);
    release = null;
    group = null;
    lead = [];
  };

  for (const line of text.split(/\r?\n/)) {
    const heading = VERSION_HEADING.exec(line);
    if (heading) {
      closeRelease();
      const parts = heading[2]
        .split('—')
        .map((p) => p.trim())
        .filter((p) => p !== '');
      const date = parts.length > 0 && DATE.test(parts[0]) ? parts.shift()! : null;
      release = {
        version: heading[1],
        date,
        headline: parts.length > 0 ? parts.join(' — ') : null,
        lead: null,
        groups: [],
      };
      continue;
    }
    if (!release) continue;
    if (LINK_DEFINITION.test(line)) {
      closeBullet();
      continue;
    }

    const groupHeading = GROUP_HEADING.exec(line);
    if (groupHeading) {
      closeBullet();
      group = { heading: groupHeading[1], changes: [] };
      release.groups.push(group);
      continue;
    }

    const bullet = BULLET.exec(line);
    if (bullet) {
      closeBullet();
      // A bullet before any `###` still has to land somewhere: give it an
      // unnamed group rather than dropping it. Losing a change silently is
      // worse than showing one without its heading.
      if (!group) {
        group = { heading: '', changes: [] };
        release.groups.push(group);
      }
      const change: ReleaseChange = { depth: bullet[1].length > 0 ? 1 : 0, title: null, body: '' };
      group.changes.push(change);
      open = { change, raw: [bullet[2]] };
      continue;
    }

    if (line.trim() === '') {
      closeBullet();
      continue;
    }
    if (open) open.raw.push(line);
    else if (!group) lead.push(line);
  }
  closeRelease();
  return releases;
}

/**
 * The release matching `version`, or null.
 *
 * A leading `v` is tolerated on either side, and the match is on the whole
 * version so `0.7.0` never answers for `10.7.0`.
 */
export function findRelease(releases: Release[], version: string): Release | null {
  const wanted = version.trim().replace(/^[vV]/, '');
  return releases.find((r) => r.version.replace(/^[vV]/, '') === wanted) ?? null;
}

/**
 * The releases that actually shipped, newest first.
 *
 * `[Unreleased]` is excluded by the only thing that distinguishes it: it has
 * no date. A build is never running it, and offering it in a history of what
 * changed would describe work the person does not have.
 */
export function releaseHistory(releases: Release[]): Release[] {
  return releases.filter((r) => r.date !== null);
}
