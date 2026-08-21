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

for pkg in $serialised; do
    cargo test --all-features "$@" -p "$pkg" -- --test-threads=1
done
