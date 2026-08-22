#!/usr/bin/env sh
#
# Run the workspace tests, with the libSQL-touching crates one thread at a time.
#
# Those crates each open their own in-memory database, and on Windows a
# parallel run crashes the test binary with STATUS_ACCESS_VIOLATION
# (0xc0000005) after every assertion has already passed: the fault is in
# tearing two databases down at once, not in anything under test. Measured on
# the maintainer's machine, release build: 5 crashes in 150 parallel runs, 0 in
# 150 serialised ones. A coin flip per push is worse than a slow test, because
# it fails a branch that is green and teaches whoever hits it to reach for
# --no-verify.
#
# 0 in 150 is not 0. On 2026-08-22 a serialised dbboard-mcp run crashed exactly
# this way, on a branch whose entire diff was TypeScript, and 35 deliberate
# reproduction runs afterwards came back clean. Serialising removed the common
# case, not the race, so the residual is retried below rather than left to
# fail a green branch once or twice a year (ADR-0125).
#
# Which crates those are is data rather than a name written here, because the
# hazard spreads by dependency: any crate taking dbboard-turso, directly or
# through dbboard-connect, inherits it.
# crates/dbboard-turso/tests/serialised_teardown.rs derives the same set from
# the workspace manifests and fails when the file drifts from it.
#
# Any arguments are passed to every cargo invocation, so this takes --release.

set -e

list="scripts/libsql-serialised-crates.txt"

# Fail CLOSED: running these in parallel is the bug itself, so a missing list
# must stop the run rather than fall back to the fast path.
if [ ! -f "$list" ]; then
    echo "error: $list is missing — refusing to run the libSQL crates in parallel." >&2
    exit 1
fi

# tr -d '\r': the working tree is CRLF on Windows, and a trailing CR would
# reach cargo as part of the package name.
serialised=$(sed 's/#.*//' "$list" | tr -d ' \t\r' | grep -v '^$')

excludes=""
for pkg in $serialised; do
    excludes="$excludes --exclude $pkg"
done

# $excludes is split into separate arguments on purpose.
# shellcheck disable=SC2086
cargo test --all-features "$@" --workspace $excludes

# The crash, as it looks from outside the dead process: an access violation
# and no failure ever reported, because the harness did not survive to print
# one. Matched on the signature rather than on the exit code, so that a crate
# whose tests genuinely failed is never retried — retrying a real failure is
# how a fault gets quietly turned into a pass.
crashed_at_teardown() {
    grep -qE 'STATUS_ACCESS_VIOLATION|0xc0000005|Segmentation fault' "$1" &&
        ! grep -qE '^failures:|^test result: FAILED' "$1"
}

log="${TMPDIR:-/tmp}/dbboard-serialised-$$.log"
rc="${TMPDIR:-/tmp}/dbboard-serialised-$$.rc"
trap 'rm -f "$log" "$rc"' EXIT

for pkg in $serialised; do
    attempt=1
    while :; do
        # The status wanted is cargo's, not tee's, and `sh` is dash on CI so
        # there is no pipefail to ask for it.
        set +e
        { cargo test --all-features "$@" -p "$pkg" -- --test-threads=1; echo $? >"$rc"; } 2>&1 | tee "$log"
        set -e
        status=$(cat "$rc")

        if [ "$status" -eq 0 ]; then
            break
        fi

        # Once. Twice in a row is no longer the tail of a race, and calling it
        # one would be the same mistake as calling it impossible.
        if [ "$attempt" -eq 1 ] && crashed_at_teardown "$log"; then
            echo "[test] $pkg died at teardown (0xc0000005) with no test reported failed." >&2
            echo "[test] that is the known libSQL race, not a result — running $pkg once more." >&2
            attempt=2
            continue
        fi

        exit "$status"
    done
done
