// Checks on `llms.txt`, the entry point for AI agents (issue #142). Run with
// the Node built-in runner — no test framework, no install step:
//
//   node --test site/llms.test.mjs
//
// These are file-content assertions rather than behaviour tests because
// `llms.txt` has no behaviour: it is a static file whose whole job is to hold
// a correct set of links. What can break is that somebody edits it and drops
// the one link the issue's completion condition names, or that a doc gets
// renamed and the link rots silently.
import { test } from "node:test";
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const llms = await readFile(join(here, "llms.txt"), "utf8");

test("llms.txt ships from site/, so Pages serves it at the site root", async () => {
  // The pages workflow rsyncs `site/` and excludes only `*.test.mjs` and
  // `package.json`. Anything else in here is published, which is what makes
  // https://meta-taro.github.io/dbboard/llms.txt resolve.
  await access(join(here, "llms.txt"));
});

test("it links the MCP server section", () => {
  // Issue #142's completion condition, stated literally: the file must
  // contain a URL to the MCP server section.
  assert.ok(
    llms.includes("https://github.com/meta-taro/dbboard#mcp-server"),
    "the README's MCP server anchor is missing",
  );
});

test("the connection JSON is one hop away", () => {
  // The other completion condition. The MCP crate README is the file that
  // holds the ready-to-paste `mcpServers` block, so linking it directly is
  // what makes the block reachable without a second hop through the long
  // top-level README.
  assert.ok(
    llms.includes("crates/dbboard-mcp/README.md"),
    "the MCP crate README — the file holding the mcpServers JSON — is not linked",
  );
});

test("every linked repository file exists", async () => {
  // Guards against a doc being renamed while llms.txt keeps pointing at the
  // old path. Only raw.githubusercontent links are checked: those name a path
  // in this repository, so the path can be resolved locally without network
  // access. Links to github.com pages and to the download page are left
  // alone — they are not files in this tree.
  const rawPrefix =
    "https://raw.githubusercontent.com/meta-taro/dbboard/develop/";
  const paths = [...llms.matchAll(/https:\/\/raw\.githubusercontent\.com\/\S+?(?=\))/g)].map(
    (m) => m[0].slice(rawPrefix.length),
  );

  assert.ok(paths.length > 0, "no raw links found — the regex or the file changed shape");

  for (const p of paths) {
    await access(join(repoRoot, p)).catch(() => {
      assert.fail(`llms.txt links ${p}, which does not exist in this repository`);
    });
  }
});

test("it says dbboard is self-hosted", () => {
  // dbboard is self-hosted only (no maintainer-run SaaS), and an agent that
  // does not read that here will go looking for an account to create.
  //
  // Asserted positively on purpose. The first version of this test searched
  // for "sign up" and rejected the file — for the sentence saying there is
  // nothing to sign up for. Screening prose for an unwanted *claim* with a
  // regex cannot tell a promise from its denial; requiring the wanted claim
  // to be present can.
  assert.match(llms, /self-hosted/i);
});
