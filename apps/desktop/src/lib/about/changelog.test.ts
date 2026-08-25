import { describe, it, expect } from 'vitest';
import { flatten, parseChangelog, findRelease, releaseHistory } from './changelog';
import { bundledReleases } from './bundled';

describe('flatten', () => {
  it('removes the markup a dialog has no renderer for', () => {
    expect(flatten('see [ADR-0136](docs/decisions.md)')).toBe('see ADR-0136');
    expect(flatten('**bold** and *thin* and `code`')).toBe('bold and thin and code');
  });

  it('folds a wrapped paragraph back into one line', () => {
    expect(flatten('one\n  two\n\nthree')).toBe('one two three');
  });
});

describe('parseChangelog', () => {
  it('reads the version, the date and the slot headline off the heading', () => {
    const [release] = parseChangelog('## [0.12.0] — 2026-08-25 — A connection list you can steer');
    expect(release.version).toBe('0.12.0');
    expect(release.date).toBe('2026-08-25');
    expect(release.headline).toBe('A connection list you can steer');
  });

  it('leaves the headline null when the heading is only a date', () => {
    const [release] = parseChangelog('## [0.10.0] — 2026-08-20');
    expect(release.date).toBe('2026-08-20');
    expect(release.headline).toBeNull();
  });

  it('leaves the date null for Unreleased, headline and all', () => {
    const [release] = parseChangelog('## [Unreleased] — Knowing what changed');
    expect(release.version).toBe('Unreleased');
    expect(release.date).toBeNull();
    expect(release.headline).toBe('Knowing what changed');
  });

  it('groups bullets under the heading they follow', () => {
    const [release] = parseChangelog(
      ['## [0.2.0] — 2026-07-17', '', '### Added', '', '- one', '', '### Fixed', '', '- two'].join(
        '\n',
      ),
    );
    expect(release.groups.map((g) => g.heading)).toEqual(['Added', 'Fixed']);
    expect(release.groups[0].changes[0].body).toBe('one');
    expect(release.groups[1].changes[0].body).toBe('two');
  });

  it("splits a bullet's bolded opening off as its title", () => {
    const [release] = parseChangelog(
      ['## [0.2.0] — 2026-07-17', '', '### Added', '', '- **A short title** — the prose.'].join(
        '\n',
      ),
    );
    const change = release.groups[0].changes[0];
    expect(change.title).toBe('A short title');
    expect(change.body).toBe('the prose.');
  });

  it('leaves a bullet without a bolded opening as body alone', () => {
    const [release] = parseChangelog(
      ['## [0.2.0] — 2026-07-17', '', '### Added', '', '- just prose, `code` and all.'].join('\n'),
    );
    const change = release.groups[0].changes[0];
    expect(change.title).toBeNull();
    expect(change.body).toBe('just prose, code and all.');
  });

  it('keeps a wrapped bullet whole rather than splitting it in two', () => {
    const [release] = parseChangelog(
      [
        '## [0.2.0] — 2026-07-17',
        '',
        '### Added',
        '',
        '- **Title** — a sentence that ran on',
        '  past the margin and kept going.',
      ].join('\n'),
    );
    expect(release.groups[0].changes).toHaveLength(1);
    expect(release.groups[0].changes[0].body).toBe(
      'a sentence that ran on past the margin and kept going.',
    );
  });

  it('marks an indented bullet as nested instead of promoting it', () => {
    const [release] = parseChangelog(
      ['## [0.2.0] — 2026-07-17', '', '### Added', '', '- top', '  - under it'].join('\n'),
    );
    expect(release.groups[0].changes.map((c) => [c.depth, c.body])).toEqual([
      [0, 'top'],
      [1, 'under it'],
    ]);
  });

  it('keeps the prose that sits above the first heading as the lead', () => {
    const [release] = parseChangelog(
      ['## [0.2.0] — 2026-07-17', '', 'What this one was about.', '', '### Added', '', '- one'].join(
        '\n',
      ),
    );
    expect(release.lead).toBe('What this one was about.');
  });

  it('does not read the link definitions at the foot as the oldest release', () => {
    const [release] = parseChangelog(
      [
        '## [0.1.0] — 2026-05-25',
        '',
        '### Added',
        '',
        '- one',
        '',
        '[0.1.0]: https://example.invalid/tag/v0.1.0',
      ].join('\n'),
    );
    expect(release.groups[0].changes).toHaveLength(1);
    expect(release.groups[0].changes[0].body).toBe('one');
  });
});

describe('findRelease', () => {
  const releases = parseChangelog(
    ['## [10.7.0] — 2026-08-25', '', '- new', '', '## [0.7.0] — 2026-08-11', '', '- old'].join('\n'),
  );

  it('tolerates a leading v on the version asked for', () => {
    expect(findRelease(releases, 'v0.7.0')?.date).toBe('2026-08-11');
  });

  it('matches the whole version, so 0.7.0 is not answered by 10.7.0', () => {
    expect(findRelease(releases, '0.7.0')?.date).toBe('2026-08-11');
  });

  it('answers null for a version the file has never heard of', () => {
    expect(findRelease(releases, '99.0.0')).toBeNull();
  });
});

describe('releaseHistory', () => {
  it('drops Unreleased, which no build is ever running', () => {
    const releases = parseChangelog(
      ['## [Unreleased] — later', '', '- soon', '', '## [0.7.0] — 2026-08-11', '', '- old'].join(
        '\n',
      ),
    );
    expect(releaseHistory(releases).map((r) => r.version)).toEqual(['0.7.0']);
  });
});

// The fixtures above say what the rules are; this says the file we ship still
// obeys them, through the same `?raw` import the dialog uses. A parser green
// on invented input, or an import that resolved to nothing, would leave the
// dialog empty and nothing would have failed.
describe('the CHANGELOG.md this build bundles', () => {
  const releases = bundledReleases();

  it('parses into every version the file has', () => {
    expect(releases.length).toBeGreaterThanOrEqual(14);
    expect(releases[0].version).toBe('Unreleased');
    expect(releaseHistory(releases)[0].version).toBe('0.12.0');
    expect(releaseHistory(releases).at(-1)?.version).toBe('0.1.0');
  });

  it('gives every shipped release something to show', () => {
    for (const release of releaseHistory(releases)) {
      const changes = release.groups.flatMap((g) => g.changes);
      expect(changes.length, `${release.version} has no changes`).toBeGreaterThan(0);
      for (const change of changes) {
        expect(change.title ?? change.body, `${release.version} has an empty bullet`).not.toBe('');
      }
    }
  });

  it('leaves no markdown syntax in what it hands the dialog', () => {
    const text = releases
      .flatMap((r) => [r.lead ?? '', ...r.groups.flatMap((g) => g.changes.map((c) => c.body))])
      .join(' ');
    expect(text).not.toMatch(/\*\*|`|\]\(/);
  });
});
