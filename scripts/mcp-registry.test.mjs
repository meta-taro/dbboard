// `server.json` is what the MCP Registry lists dbboard-mcp by (ADR-0168).
//
// It carries a version, and nothing but a release ever reads it, so left
// alone it rots: the listing goes on naming an old release, and whoever
// finds it there reads "not maintained". These tests hold it to the app's
// version, and hold the release cut to bumping it.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { TARGETS, bumpJson } from "./release-cut.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const json = (p) => JSON.parse(readFileSync(join(root, p), "utf8"));

test("server.json names dbboard under the owner's GitHub namespace", () => {
  const server = json("server.json");
  assert.equal(server.name, "io.github.meta-taro/dbboard");
  assert.equal(server.repository.url, "https://github.com/meta-taro/dbboard");
  assert.equal(server.repository.source, "github");
});

test("server.json claims the version the app is at", () => {
  const app = json("apps/desktop/src-tauri/tauri.conf.json").version;
  assert.equal(
    json("server.json").version,
    app,
    `server.json must say "version": "${app}" — node scripts/release-cut.mjs keeps them together`,
  );
});

test("the description fits the registry's limit", () => {
  // The registry rejects a description over 100 characters at publish
  // time, which is the tag push: too late to find out.
  const { description } = json("server.json");
  assert.ok(description.length > 0 && description.length <= 100, description);
});

test("cutting a release bumps server.json with the rest", () => {
  const paths = TARGETS.map(([path]) => path);
  assert.ok(paths.includes("server.json"), paths.join(", "));
  const edit = TARGETS.find(([path]) => path === "server.json")[1];
  assert.equal(edit, bumpJson);
});

test("the bump edits the version and leaves the schema URL alone", () => {
  const src = readFileSync(join(root, "server.json"), "utf8");
  const out = JSON.parse(bumpJson(src, "9.9.9"));
  assert.equal(out.version, "9.9.9");
  assert.equal(out.$schema, JSON.parse(src).$schema);
});
