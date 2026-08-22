// The serialised runner's behaviour when a test binary dies at teardown.
//
// scripts/cargo-test-serialised.sh runs the libSQL-touching crates one thread
// at a time because two in-memory teardowns at once crash the binary on
// Windows. Serialising made that rare — 0 in 150 measured runs — and the
// recipe was written as though rare meant gone. It is not: a serialised
// dbboard-mcp run crashed on 2026-08-22 with every assertion passed, on a
// branch whose diff was TypeScript.
//
// So the runner retries that one signature, once. What it must never do is
// retry a test that actually failed, which is the whole reason the signature
// is matched rather than the exit code.
//
// These drive the script with a fake `cargo` on PATH, because the behaviour
// under test is "what does it do when cargo dies", and the real cargo cannot
// be asked to die on cue.

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const runner = join(root, "scripts", "cargo-test-serialised.sh");

/** A fake cargo whose behaviour depends on `$MODE` and how often it has run. */
const CARGO = `#!/usr/bin/env sh
# The workspace pass is not what these tests are about; let it through.
for a in "$@"; do
    [ "$a" = "--workspace" ] && exit 0
done

n=$(cat "$STATE" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "$STATE"

if [ "$MODE" = "real-failure" ]; then
    echo "failures:"
    echo "    fake::tests::a_thing_that_is_wrong"
    echo "test result: FAILED. 2 passed; 1 failed; 0 ignored"
    exit 101
fi

if [ "$MODE" = "crash-once" ] && [ "$n" -ge 2 ]; then
    echo "test result: ok. 3 passed; 0 failed; 0 ignored"
    exit 0
fi

# What the teardown crash looks like: no failure reported, process gone.
echo "test fake::tests::something ... error: test failed, to rerun pass -p fake-crate --lib"
echo "  process didn't exit successfully: fake.exe (exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)"
exit 139
`;

/** Run the real script in a throwaway tree, against the fake cargo. */
function run(mode) {
  const dir = mkdtempSync(join(tmpdir(), "dbboard-serialised-"));
  mkdirSync(join(dir, "scripts"));
  mkdirSync(join(dir, "bin"));
  writeFileSync(join(dir, "scripts", "libsql-serialised-crates.txt"), "# a list\nfake-crate\n");

  const cargo = join(dir, "bin", "cargo");
  writeFileSync(cargo, CARGO);
  chmodSync(cargo, 0o755);

  const state = join(dir, "runs");
  const out = spawnSync("sh", [runner], {
    cwd: dir,
    encoding: "utf8",
    env: { ...process.env, PATH: `${join(dir, "bin")}:${process.env.PATH}`, MODE: mode, STATE: state },
  });

  return {
    status: out.status,
    output: `${out.stdout ?? ""}${out.stderr ?? ""}`,
    attempts: Number(readFileSync(state, "utf8").trim()),
  };
}

test("a teardown crash is retried, and a clean re-run is a pass", () => {
  const r = run("crash-once");
  assert.equal(r.attempts, 2, "the crate should have been run twice");
  assert.equal(r.status, 0, `the run should have succeeded:\n${r.output}`);
});

test("the retry says so, rather than swallowing the crash silently", () => {
  // A crash that leaves no trace teaches the next reader that it never
  // happens, which is the belief this whole change exists to correct.
  const r = run("crash-once");
  assert.match(r.output, /0xc0000005|teardown/i);
});

test("a test that actually failed is not retried", () => {
  const r = run("real-failure");
  assert.equal(r.attempts, 1, "a real failure must fail on the spot");
  assert.notEqual(r.status, 0);
});

test("crashing twice in a row fails the run", () => {
  // Once is the known tail of a race. Twice is something else, and passing it
  // off as the same flake would be how a real fault gets retried into silence.
  const r = run("crash-always");
  assert.equal(r.attempts, 2);
  assert.notEqual(r.status, 0);
});
