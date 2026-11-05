// The Windows installer carries the MCP server (ADR-0169).
//
// Until v0.22.1 the server was only a separate release asset, copied by hand
// to %LOCALAPPDATA%\dbboard\dbboard-mcp.exe. The installer never touched it,
// so every upgrade left the agent talking to whatever build was copied last:
// on 0.20.1 two machines ran the app at 0.20.1 and the server at 0.10.0.
//
// The per-user NSIS installer installs to %LOCALAPPDATA%\<productName>, and an
// `externalBin` lands next to the app under its name without the target
// triple, which is the path the setup instructions already register. These
// tests hold the pieces of that together: lose one and the installer either
// stops shipping the server or stops building.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const json = (p) => JSON.parse(read(p));

const TAURI = "apps/desktop/src-tauri";
const OVERLAY = `${TAURI}/tauri.bundle-mcp.conf.json`;
const HOOKS = `${TAURI}/windows/mcp-installer-hooks.nsh`;

test("the installer lands where the setup instructions register the server", () => {
  // %LOCALAPPDATA%\<productName>\<externalBin name>.exe
  assert.equal(json(`${TAURI}/tauri.conf.json`).productName, "dbboard");
  assert.deepEqual(json(OVERLAY).bundle.externalBin, ["binaries/dbboard-mcp"]);
  assert.ok(read("README.md").includes("$env:LOCALAPPDATA\\dbboard\\dbboard-mcp.exe"));
});

test("the base config does not name the server, so builds without it still work", () => {
  // tauri-build fails when an externalBin is missing, and only the release
  // job builds the server first. clippy, tests and `tauri dev` must not need it.
  const base = json(`${TAURI}/tauri.conf.json`);
  assert.equal(base.bundle.externalBin, undefined);
  assert.equal(base.bundle.windows?.nsis?.installerHooks, undefined);
});

test("the Windows release build stages the server and applies the overlay", () => {
  const yml = read(".github/workflows/release.yml");
  const job = yml.slice(yml.indexOf("  build-tauri-windows:"), yml.indexOf("  build-tauri-macos:"));
  assert.match(job, /cargo build --release -p dbboard-mcp/);
  assert.match(job, /src-tauri\/binaries\/dbboard-mcp-\$triple\.exe/);
  assert.match(job, /pnpm tauri build --config src-tauri\/tauri\.bundle-mcp\.conf\.json/);
  const staged = job.indexOf("cargo build --release -p dbboard-mcp");
  const bundled = job.indexOf("pnpm tauri build");
  assert.ok(staged < bundled, "the server must exist before the bundler looks for it");
});

test("the staged server is not committed", () => {
  assert.match(read(".gitignore"), /^\/apps\/desktop\/src-tauri\/binaries\/$/m);
});

test("a running server is moved aside rather than blocking the install", () => {
  // Windows refuses to overwrite a running executable but lets it be renamed.
  // An agent holding the server open would otherwise fail the update.
  const overlay = json(OVERLAY);
  assert.equal(overlay.bundle.windows.nsis.installerHooks, "windows/mcp-installer-hooks.nsh");
  const nsh = read(HOOKS);
  assert.match(nsh, /!macro NSIS_HOOK_PREINSTALL/);
  assert.match(nsh, /Rename "\$INSTDIR\\dbboard-mcp\.exe"/);
  // The rename comes after the app-running check, so cancelling that prompt
  // never leaves the install without a server.
  assert.ok(nsh.indexOf("CheckIfAppIsRunning") < nsh.indexOf("Rename"));
});
