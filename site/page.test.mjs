// The download page, checked against the page's own rules.
//
//   node --test site/page.test.mjs
//
// Screenshots are the part of this page most likely to break silently. A broken
// `<img>` is invisible in a diff and invisible in review — it only shows up as a
// blank rectangle to the visitor the page was written for. Worse, the page pins
// `img-src 'self'` in its CSP (ADR-0047), so an image pointed at a CDN does not
// degrade into a slow image, it renders as nothing at all, with the reason only
// in a console the visitor will never open.
//
// So: every image the page asks for must exist next to it, and must be asked
// for in a way the page's own CSP allows.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const html = readFileSync(join(HERE, "index.html"), "utf8");

/** Every `<img>` on the page, as `{ src, alt }`. */
function images() {
  return [...html.matchAll(/<img\b[^>]*>/gi)].map((match) => {
    const tag = match[0];
    const attr = (name) => tag.match(new RegExp(`${name}="([^"]*)"`, "i"))?.[1];
    return { tag, src: attr("src"), alt: attr("alt") };
  });
}

test("every image the page loads exists next to it", () => {
  for (const { src } of images()) {
    assert.ok(src, "an <img> has no src at all");
    assert.ok(
      existsSync(join(HERE, src)),
      `${src} is referenced but not present in site/, so it ships as a blank box`,
    );
  }
});

test("every image is loaded from this origin", () => {
  // `img-src 'self'` means an absolute URL is not a slower image, it is no
  // image. Catching it here rather than in a browser console.
  for (const { src } of images()) {
    assert.doesNotMatch(
      src,
      /^(https?:)?\/\//i,
      `${src} is off-origin and the page's own CSP blocks it`,
    );
  }
});

test("every image says what it shows", () => {
  for (const { src, alt } of images()) {
    assert.ok(alt !== undefined, `${src} has no alt attribute`);
    assert.ok(
      alt.trim().length > 0,
      `${src} has an empty alt; it carries meaning here, so it is not decorative`,
    );
  }
});

test("the page shows the app before asking anyone to install it", () => {
  // The reason this section exists: until it did, the only way to find out what
  // dbboard looks like was to download an unsigned binary and run it. Two shots
  // is the floor -- one window could be a mockup, a second showing a different
  // view is the app.
  const shots = images().filter(({ src }) => src.startsWith("screenshots/"));
  assert.ok(
    shots.length >= 2,
    `only ${shots.length} screenshot(s); the page goes back to asking for a download sight-unseen`,
  );
});

test("the page says the binaries are unsigned, and does not call it pending", () => {
  // This paragraph is the whole of what ADR-0106 promises a downloader: the
  // warning they are about to click through is expected, and it is a decision
  // rather than an oversight. Both halves matter. Dropping the disclosure
  // leaves them guessing whether the file is tampered with; calling signing
  // "planned" or a "follow-up" tells them to wait for a build that is not
  // coming, which is the dishonesty the ADR exists to avoid.
  assert.match(
    html,
    /not code-signed/i,
    "the page no longer tells a downloader the binaries are unsigned",
  );
  for (const claim of [/\byet\b/i, /planned/i, /tracked follow-up/i, /coming soon/i]) {
    const paragraph = html.match(/<h2>Before you run it<\/h2>[\s\S]*?<\/p>/i)?.[0] ?? "";
    assert.doesNotMatch(
      paragraph,
      claim,
      `the "Before you run it" note still describes signing as pending (${claim}); ADR-0106 decided against it`,
    );
  }
});

// --- Previous versions (ADR-0155) -------------------------------------------

// The archive starts hidden and is revealed by `renderArchive` only when it has
// rows. Shipping it visible would show an empty table to anyone whose API call
// is rate-limited — an empty "Previous versions" reads as "there are none",
// which is the opposite of true and exactly the wrong thing to tell someone
// trying to roll back.
test("the previous-versions section is hidden until it has rows", () => {
  const section = html.match(/<section id="archive"[^>]*>/i);
  assert.ok(section, "the archive section must exist");
  assert.match(section[0], /\bhidden\b/, "it must ship hidden");
});

// The script fills these two by id. A rename on either side is silent: the
// table simply never appears, and nothing fails.
test("the ids the script fills are the ids the page has", () => {
  const app = readFileSync(join(HERE, "app.js"), "utf8");
  for (const id of ["archive", "archive-rows"]) {
    assert.ok(
      html.includes(`id="${id}"`),
      `index.html is missing id="${id}", which app.js fills`,
    );
    assert.ok(
      app.includes(`"${id}"`),
      `app.js no longer references ${id}; the markup is then dead`,
    );
  }
});

// The reason the archive exists is the rollback, and a rollback raises one
// question immediately: what happens to my connections? The answer has to be on
// the page, next to the download, not only in a changelog.
test("the page says what a rollback does to the connections file", () => {
  assert.match(
    html,
    /connections\.pre-&lt;version&gt;\.toml/,
    "the page must name the snapshot file a rollback falls back on",
  );
  assert.match(
    html,
    /OS keychain/i,
    "the page must say credentials are not in the file",
  );
});
