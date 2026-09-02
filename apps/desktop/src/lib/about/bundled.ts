// The changelog the running build was cut from, inlined at build time.
//
// Reading it off disk at runtime would be wrong twice over: an installed copy
// has no repository beside it, and a file read after release would describe
// whatever a working tree happens to say rather than what this build shipped.
// `?raw` freezes the text into the bundle, so the notes and the binary are cut
// together and cannot drift apart (ADR-0137).
import text from '../../../../../CHANGELOG.md?raw';
import { parseChangelog, type Release } from './changelog';

let cache: Release[] | null = null;

/** Every version the shipped CHANGELOG.md describes, newest first. */
export function bundledReleases(): Release[] {
  cache ??= parseChangelog(text);
  return cache;
}
