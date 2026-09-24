#!/usr/bin/env sh
#
# The whole startup, including the half the app cannot see.
#
# `About → First paint` measures from the first line of `run()`, which is
# after the kernel has exec'd the binary and dyld has paged it in — exactly
# the work a warm launch skips. On 2026-09-24 a genuine first-launch-after-
# reboot reported 451 ms and the next launch 431 ms: a 4% gap, where a cold
# disk should have been obvious. The number was never wrong; it was answering
# a narrower question than `startup-measurement.md` item 3 asks.
#
# So this stopwatch starts before the process exists, and the app publishes
# the wall-clock instant its own clock started (`started_at_unix_ms`). The
# difference between them is the load — the part no code inside can reach.
#
#     total   = paint finished - stopwatch started   (what a person waits)
#     inside  = first_paint_ms                        (what About shows)
#     loading = total - inside                        (exec + dyld + runtime)
#
# A COLD sample is the FIRST launch after a reboot and there is exactly one
# per boot. Run this before opening the app any other way, or the number is a
# warm one wearing a cold label.
#
# **Launch a freshly built binary once BEFORE the reboot.** macOS verifies an
# app it has not seen, and that verification lands in `loading` where it looks
# exactly like a cold disk. Measured 2026-09-24, same machine, same build:
#
#     first launch of a new build   loading  3108 ms   <- Gatekeeper
#     every launch after that       loading    15-17 ms
#
# Three seconds of signature checking reported as a cold start would be a
# worse number than having none.
#
# The binary is launched directly rather than through `open`, because
# LaunchServices does not pass the environment through and the app needs
# DBBOARD_STARTUP_REPORT to know where to leave its reading.

set -eu

REPO_ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP="$REPO_ROOT/target/release/bundle/macos/dbboard.app/Contents/MacOS/dbboard-desktop"
[ "${1:-}" = "" ] || APP="$1"

if [ ! -x "$APP" ]; then
    printf 'no runnable app at %s\n' "$APP" >&2
    printf 'build one first: cd apps/desktop && pnpm tauri build\n' >&2
    printf '(the pre-push hook writes a non-runnable shell to the same path)\n' >&2
    exit 1
fi

REPORT="${TMPDIR:-/tmp}/dbboard-cold-start-$$.json"
rm -f "$REPORT" "$REPORT.partial"

now_ms() { python3 -c 'import time; print(int(time.time() * 1000))'; }

STARTED=$(now_ms)
DBBOARD_STARTUP_REPORT="$REPORT" "$APP" >/dev/null 2>&1 &
APP_PID=$!

# The frontend reports after its first meaningful frame, so this normally
# lands in well under a second. Thirty is for a cold machine still settling.
WAITED=0
while [ ! -f "$REPORT" ]; do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
        printf 'the app exited before it painted\n' >&2
        exit 1
    fi
    if [ "$WAITED" -ge 3000 ]; then
        printf 'no report after 30s — is this build older than the report path?\n' >&2
        kill "$APP_PID" 2>/dev/null || true
        exit 1
    fi
    sleep 0.01
    WAITED=$((WAITED + 1))
done

python3 - "$REPORT" "$STARTED" <<'PY'
import json, sys

report, started = sys.argv[1], int(sys.argv[2])
with open(report, encoding="utf-8") as f:
    reading = json.load(f)

inside = reading["first_paint_ms"]
if inside is None:
    sys.exit("the app wrote a report before it painted")

painted_at = reading["started_at_unix_ms"] + inside
total = painted_at - started
loading = total - inside

print(f"  total    {total:5d} ms   launch -> painted (what a person waits)")
print(f"  inside   {inside:5d} ms   what About calls First paint")
print(f"  loading  {loading:5d} ms   exec + dyld + runtime, before the clock starts")
PY

printf '\nthe app is still running as pid %s — close it when you are done\n' "$APP_PID"
rm -f "$REPORT"
