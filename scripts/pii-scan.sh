#!/usr/bin/env sh
#
# dbboard PII / secret leak scanner (ADR-0055).
#
# Prevents business-identifying names and credential material from entering
# the public repository — on every commit, on every commit message, and
# once a day in CI. It is the *preventive* companion to the one-time
# history rewrite in docs/maintainer/history-sanitize-runbook.md.
#
# Two severities, because this is a database client whose tests are full of
# SYNTHETIC connection strings and example emails. Gating commits on those
# shapes would be a false-positive wall, so shape rules that fixtures also
# match are advisory, not blocking.
#
#   BLOCKING (exit 1) — a real leak, blocks the commit / fails CI:
#     * denylist literals: the real store names, the maintainer's real
#       username / name / personal email. Supplied out-of-band (see below),
#       matched exactly, and REDACTED in output so a public log never
#       echoes the very string we hide. This is the primary mechanism —
#       the documented incident was real store names in test fixtures.
#     * private-key: PEM private-key blocks.
#     * aws-access-key-id: a real-looking AWS key id.
#
#   ADVISORY (never fails, printed in the daily CI tree/range scan for a
#   human to eyeball) — high-value shapes that fixtures also trip:
#     * passworded-db-url, personal-email, windows-home-path.
#   By project invariant real secrets live only in the OS keyring, never in
#   a tracked file, so a passworded URL in the tree is a fixture — worth a
#   glance, not a build break. A genuinely new personal email still shows up
#   here; add known real PII to the denylist to make it blocking.
#
# The denylist itself is NOT committed (that would put the real strings back
# into a tracked file). Default path: .pii-denylist at the repo root, or
# $PII_DENYLIST_FILE. In CI it is materialized from the PII_DENYLIST secret.
#
# Modes:
#   --staged            Scan the git index (pre-commit hook). Blocking only.
#   --message <file>    Scan a commit-message file (commit-msg hook). Blocking.
#   --tree              Scan tracked files at HEAD (daily CI). Blocking+advisory.
#   --range <A..B>      Scan commit messages in a range (CI). Blocking+advisory.
#   --identity <A..B>   Scan author/committer identity in a range (CI). Blocking.
#   --selftest          Run built-in fixtures and verify the rules fire.
#
# Flags:
#   --reveal            Print matched text for BLOCKING generic hits instead
#                       of redacting. Local hooks pass it; CI does not.
#                       Denylist hits are redacted regardless.
#
# Exit status: 0 = clean, 1 = a blocking leak was found, 2 = usage error.

set -eu

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
ALLOW_FILE="$REPO_ROOT/scripts/pii-scan.allow"
DENYLIST_FILE="${PII_DENYLIST_FILE:-$REPO_ROOT/.pii-denylist}"

REVEAL=0
MODE=""
ARG=""

# Paths never worth scanning (the scanner's own files carry every pattern
# as a definition and would self-flag; binaries and lockfiles are noise).
exclude_pathspecs() {
    printf '%s\n' \
        ":(exclude)scripts/pii-scan.sh" \
        ":(exclude)scripts/pii-scan.allow" \
        ":(exclude).pii-denylist.example" \
        ":(exclude)docs/maintainer/pii-scanning.md" \
        ":(exclude)Cargo.lock" \
        ":(exclude)*.png" ":(exclude)*.jpg" ":(exclude)*.ico" ":(exclude)*.icns"
}

# "id<TAB>extended-regex", one per line.
block_rules() {
    cat <<'RULES'
private-key	-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----
aws-access-key-id	AKIA[0-9A-Z]{16}
RULES
}

advisory_rules() {
    cat <<'RULES'
passworded-db-url	(postgres|postgresql|mysql|libsql|mongodb)://[^ :@/"']+:[^ :@/"']+@
personal-email	[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}
windows-home-path	[Cc]:\\+[Uu]sers\\+[A-Za-z0-9]
RULES
}

# Allowlist regexes, joined into one ERE per TIER. Blank and comment lines
# are stripped first — a blank line fed to `grep -f` is an empty pattern that
# matches EVERYTHING, which would silently allowlist every finding.
#
# Tier scoping matters: the broad advisory entries (example domains, `host`
# placeholders) must NEVER be able to drop a BLOCKING finding. A real AWS key
# on the same line as `admin@example.com` would otherwise be silently
# allowlisted. So block-tier findings consult ONLY the block section of the
# allow file (marked `# === BLOCKING`); advisory findings consult everything.
BLOCK_ALLOW_RE=""
ADVISORY_ALLOW_RE=""
if [ -f "$ALLOW_FILE" ]; then
    ADVISORY_ALLOW_RE=$(grep -vE '^[[:space:]]*(#|$)' "$ALLOW_FILE" 2>/dev/null | paste -sd'|' -)
    BLOCK_ALLOW_RE=$(awk '
        /^#[[:space:]]*===[[:space:]]*BLOCKING/ { sec="block"; next }
        /^#[[:space:]]*===/                     { sec="other"; next }
        /^[[:space:]]*(#|$)/                    { next }
        sec=="block"                            { print }
    ' "$ALLOW_FILE" 2>/dev/null | paste -sd'|' -)
fi
# is_allowlisted <block|advisory> <text>
is_allowlisted() {
    if [ "$1" = block ]; then re=$BLOCK_ALLOW_RE; else re=$ADVISORY_ALLOW_RE; fi
    [ -n "$re" ] || return 1
    printf '%s' "$2" | grep -qE "$re" 2>/dev/null
}

# --- generic scan ---------------------------------------------------------
# scan_generic <block|advisory> <git-grep source args...>
# Prints blocking hits as "  [rule] where" and advisory hits as
# "  (advisory) [rule] where". run_and_check keys failure off the former.
scan_generic() {
    tier=$1; shift
    if [ "$tier" = block ]; then rules=block_rules; prefix='  '; else rules=advisory_rules; prefix='  (advisory) '; fi
    $rules | while IFS='	' read -r rule regex; do
        [ -n "$rule" ] || continue
        git grep -nIE -e "$regex" "$@" -- $(exclude_pathspecs) 2>/dev/null \
        | while IFS= read -r line; do
            is_allowlisted "$tier" "$line" && continue
            where=$(printf '%s' "$line" | cut -d: -f1-2)
            text=$(printf '%s' "$line" | cut -d: -f3-)
            if [ "$tier" = block ] && [ "$REVEAL" = 1 ]; then
                printf '%s[%s] %s\n      %s\n' "$prefix" "$rule" "$where" "$text" >&2
            else
                printf '%s[%s] %s  (match redacted)\n' "$prefix" "$rule" "$where" >&2
            fi
        done
    done
}

denylist_id() {
    if command -v sha1sum >/dev/null 2>&1; then printf '%s' "$1" | sha1sum | cut -c1-8
    else printf '%s' "$1" | shasum | cut -c1-8; fi
}

# Blocking. Missing file => generic-only (a fresh clone without the private
# mapping is not blocked, it just loses literal detection).
scan_denylist() {
    if [ ! -f "$DENYLIST_FILE" ]; then
        printf '  note: no denylist file (%s) — literal name detection off\n' "$DENYLIST_FILE" >&2
        return 0
    fi
    while IFS= read -r entry || [ -n "$entry" ]; do
        case "$entry" in ''|'#'*) continue ;; esac
        id="denylist#$(denylist_id "$entry")"
        git grep -nIiF -e "$entry" "$@" -- $(exclude_pathspecs) 2>/dev/null \
        | while IFS= read -r line; do
            where=$(printf '%s' "$line" | cut -d: -f1-2)
            printf '  [%s] %s  (match redacted)\n' "$id" "$where" >&2
        done
    done < "$DENYLIST_FILE"
}

# --- commit identity (ADR-0084) -------------------------------------------
# A leaked string in a *file* is fixable with another commit. An address in
# the author/committer field is not: it is part of the commit object, so
# removing it means rewriting every descendant hash and force-pushing. That
# asymmetry is why identity is checked separately, and why it is checked
# before the commit exists (--staged) as well as after (--identity <range>).
#
# GitHub's noreply forms are the only publishable addresses. Override with
# OSS_IDENTITY_ALLOW_RE if this repo ever moves off GitHub.
IDENTITY_ALLOW_RE="${OSS_IDENTITY_ALLOW_RE:-^([0-9]+\+)?[A-Za-z0-9-]+@users\.noreply\.github\.com$}"

# identity_allowed <email> — 0 when the address is safe to publish.
identity_allowed() {
    [ -n "${1:-}" ] || return 1
    printf '%s' "$1" | grep -qE "$IDENTITY_ALLOW_RE"
}

# scan_identity (--config | <range>)
# Output is deliberately value-free: naming the address in a public Actions
# log would republish the very thing the check exists to keep out.
scan_identity() {
    if [ "$1" = "--config" ]; then
        e=$(git config user.email 2>/dev/null || true)
        if [ -z "$e" ]; then
            printf '  [identity] git config user.email is unset\n' >&2
        elif ! identity_allowed "$e"; then
            printf '  [identity] git config user.email is not a GitHub noreply address  (value redacted)\n' >&2
        fi
        return 0
    fi

    git log --format='%H%x09%ae%x09%ce' "$1" 2>/dev/null \
    | while IFS='	' read -r h ae ce; do
        [ -n "$h" ] || continue
        identity_allowed "$ae" || printf '  [identity] %s author email is not a GitHub noreply address  (value redacted)\n' "$h" >&2
        identity_allowed "$ce" || printf '  [identity] %s committer email is not a GitHub noreply address  (value redacted)\n' "$h" >&2
    done

    # Display names are free-form, so there is no shape to test against; the
    # denylist is the only thing that knows the maintainer's real name.
    [ -f "$DENYLIST_FILE" ] || return 0
    names=$(git log --format='%H%x09%an%x09%cn' "$1" 2>/dev/null || true)
    while IFS= read -r entry || [ -n "$entry" ]; do
        case "$entry" in ''|'#'*) continue ;; esac
        id="denylist#$(denylist_id "$entry")"
        printf '%s' "$names" | grep -iF -e "$entry" 2>/dev/null \
        | while IFS='	' read -r h _rest; do
            printf '  [%s] %s commit identity  (match redacted)\n' "$id" "$h" >&2
        done
    done < "$DENYLIST_FILE"
}

# --- text-stream scan (commit messages / logs, not tracked files) ---------
# scan_text_stream <label> <block|both>  — reads text on stdin.
scan_text_stream() {
    label=$1; scope=$2
    input=$(cat)
    _stream_rules() { if [ "$scope" = both ]; then block_rules; advisory_rules; else block_rules; fi; }
    _stream_rules | while IFS='	' read -r rule regex; do
        [ -n "$rule" ] || continue
        # advisory rules print non-blocking; block rules print blocking.
        adv=$(block_rules | grep -qE "^$rule	" && echo 0 || echo 1)
        if [ "$adv" = 1 ]; then atier=advisory; else atier=block; fi
        printf '%s' "$input" | grep -nIE -e "$regex" 2>/dev/null \
        | while IFS= read -r line; do
            is_allowlisted "$atier" "$label:$line" && continue
            n=$(printf '%s' "$line" | cut -d: -f1)
            if [ "$adv" = 1 ]; then
                printf '  (advisory) [%s] %s:%s  (match redacted)\n' "$rule" "$label" "$n" >&2
            else
                printf '  [%s] %s:%s  (match redacted)\n' "$rule" "$label" "$n" >&2
            fi
        done
    done
    if [ -f "$DENYLIST_FILE" ]; then
        while IFS= read -r entry || [ -n "$entry" ]; do
            case "$entry" in ''|'#'*) continue ;; esac
            id="denylist#$(denylist_id "$entry")"
            printf '%s' "$input" | grep -nIiF -e "$entry" 2>/dev/null \
            | while IFS= read -r line; do
                n=$(printf '%s' "$line" | cut -d: -f1)
                printf '  [%s] %s:%s  (match redacted)\n' "$id" "$label" "$n" >&2
            done
        done < "$DENYLIST_FILE"
    fi
}

# Run a scanner, echo its output, and FAIL only on a blocking line — one
# that starts with two spaces then '[' (advisory lines start with '  (').
run_and_check() {
    out=$("$@" 2>&1 || true)
    [ -n "$out" ] && printf '%s\n' "$out" >&2
    printf '%s' "$out" | grep -qE '^  \[' && return 1
    return 0
}

usage() {
    printf 'usage: pii-scan.sh (--staged | --tree | --message <f> | --range <A..B> | --selftest) [--reveal]\n' >&2
    exit 2
}

# --- selftest -------------------------------------------------------------
selftest() {
    tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT; rc=0

    # Blocking positives: a private key line and an AWS key must fire.
    cat > "$tmp/block.txt" <<'FIX'
-----BEGIN RSA PRIVATE KEY-----
aws_key = AKIAIOSFODNN7EXAMPLE
FIX
    bf=0
    block_rules | while IFS='	' read -r rule regex; do
        [ -n "$rule" ] || continue
        grep -qE -e "$regex" "$tmp/block.txt" 2>/dev/null && printf 'x'
    done > "$tmp/bf"; bf=$(wc -c < "$tmp/bf" | tr -d ' ')
    if [ "${bf:-0}" -lt 2 ]; then printf 'selftest FAIL: block rules fired %s/2\n' "$bf" >&2; rc=1; fi

    # Advisory positives: url / email / home path.
    cat > "$tmp/adv.txt" <<'FIX'
db = postgres://admin:s3cr3tpw@db.internal:5432/app
contact: real.person@somecorp.co.jp
home C:\Users\johndoe\dbboard
FIX
    af=0
    advisory_rules | while IFS='	' read -r rule regex; do
        [ -n "$rule" ] || continue
        grep -qE -e "$regex" "$tmp/adv.txt" 2>/dev/null && printf 'x'
    done > "$tmp/af"; af=$(wc -c < "$tmp/af" | tr -d ' ')
    if [ "${af:-0}" -lt 3 ]; then printf 'selftest FAIL: advisory rules fired %s/3\n' "$af" >&2; rc=1; fi

    # Negatives: placeholders / allowlisted shapes must NOT trip any rule.
    cat > "$tmp/clean.txt" <<'FIX'
connection id: store-a
sample rows: Alpha, Beta
noreply@anthropic.com
path C:\path\to\dbboard-mcp.exe
home C:\Users\alice\dbboard
url postgres://user:pass@db.example.supabase.co/db
FIX
    { block_rules; advisory_rules; } | while IFS='	' read -r rule regex; do
        [ -n "$rule" ] || continue
        grep -nE -e "$regex" "$tmp/clean.txt" 2>/dev/null | while IFS= read -r l; do
            is_allowlisted advisory "clean.txt:$l" && continue
            printf 'HIT %s: %s\n' "$rule" "$l"
        done
    done > "$tmp/clean.out" || true
    if [ -s "$tmp/clean.out" ]; then
        printf 'selftest FAIL: clean fixture tripped a rule:\n' >&2; cat "$tmp/clean.out" >&2; rc=1
    fi

    # Exit-code convention: run_and_check must FAIL on a blocking line
    # ('  [') and PASS on an advisory line ('  (advisory) ['). Guards against
    # a print-format refactor silently turning every finding non-blocking.
    _emit_block() { printf '  [aws-access-key-id] HEAD:x.txt\n' >&2; }
    _emit_adv()   { printf '  (advisory) [personal-email] HEAD:x.txt\n' >&2; }
    if run_and_check _emit_block 2>/dev/null; then printf 'selftest FAIL: blocking line did not fail run_and_check\n' >&2; rc=1; fi
    if run_and_check _emit_adv 2>/dev/null; then :; else printf 'selftest FAIL: advisory line wrongly failed run_and_check\n' >&2; rc=1; fi

    # Commit identity (ADR-0084). GitHub's two noreply forms are the only
    # publishable author addresses; everything else — personal mail, work
    # mail, the fake `user@hostname` git invents when unconfigured — is a
    # leak that survives in commit metadata forever.
    for ok in '3032390+meta-taro@users.noreply.github.com' \
              'meta-taro@users.noreply.github.com'; do
        identity_allowed "$ok" || { printf 'selftest FAIL: identity rejected a noreply address\n' >&2; rc=1; }
    done
    for bad in 'someone@gmail.com' \
               'dev@corp.example.co.jp' \
               'runner@fv-az1234-567.(none)' \
               'attacker@evil.users.noreply.github.com.example.com' \
               ''; do
        if identity_allowed "$bad"; then
            printf 'selftest FAIL: identity accepted a publishable-unsafe address\n' >&2; rc=1
        fi
    done

    # An identity finding must be BLOCKING (run_and_check convention) and must
    # never echo the address itself — CI logs are public.
    _emit_ident() { printf '  [identity] deadbeef author email is not a GitHub noreply address  (value redacted)\n' >&2; }
    if run_and_check _emit_ident 2>/dev/null; then
        printf 'selftest FAIL: identity line did not fail run_and_check\n' >&2; rc=1
    fi
    if _emit_ident 2>&1 | grep -q '@'; then
        printf 'selftest FAIL: identity output leaked an address\n' >&2; rc=1
    fi

    # Denylist literal must match, case-insensitively.
    printf 'AcmeMegaStore\n' > "$tmp/deny"; printf 'welcome to acmemegastore\n' > "$tmp/src.txt"
    d=""
    while IFS= read -r e || [ -n "$e" ]; do
        case "$e" in ''|'#'*) continue ;; esac
        grep -iF -e "$e" "$tmp/src.txt" >/dev/null 2>&1 && d=MATCH
    done < "$tmp/deny"
    if [ "$d" != MATCH ]; then printf 'selftest FAIL: denylist literal did not match\n' >&2; rc=1; fi

    [ "$rc" = 0 ] && printf 'pii-scan selftest: ok\n' >&2
    return $rc
}

# --- arg parse ------------------------------------------------------------
while [ $# -gt 0 ]; do
    case "$1" in
        --staged)   MODE=staged ;;
        --tree)     MODE=tree ;;
        --message)  MODE=message; shift; ARG=${1:-} ;;
        --range)    MODE=range; shift; ARG=${1:-} ;;
        --identity) MODE=identity; shift; ARG=${1:-} ;;
        --selftest) MODE=selftest ;;
        --reveal)   REVEAL=1 ;;
        -h|--help)  usage ;;
        *)          printf 'unknown arg: %s\n' "$1" >&2; usage ;;
    esac
    shift
done
[ -n "$MODE" ] || usage

rc=0
case "$MODE" in
    selftest)
        selftest || rc=$?; exit $rc ;;
    staged)
        printf '[pii-scan] scanning staged changes (blocking rules)...\n' >&2
        run_and_check scan_generic block --cached || rc=1
        run_and_check scan_denylist --cached || rc=1
        # Cheapest possible moment to catch a bad identity: before the commit
        # object that would carry it forever is written.
        run_and_check scan_identity --config || rc=1 ;;
    tree)
        printf '[pii-scan] scanning tracked files at HEAD...\n' >&2
        run_and_check scan_generic block HEAD || rc=1
        run_and_check scan_denylist HEAD || rc=1
        # Advisory: print for review, never change rc.
        scan_generic advisory HEAD 2>&1 | sed '/^$/d' >&2 || true ;;
    message)
        [ -n "$ARG" ] && [ -f "$ARG" ] || { printf 'no message file\n' >&2; exit 2; }
        printf '[pii-scan] scanning commit message (blocking rules)...\n' >&2
        out=$(scan_text_stream "commit-msg" block < "$ARG" 2>&1 || true)
        [ -n "$out" ] && printf '%s\n' "$out" >&2
        printf '%s' "$out" | grep -qE '^  \[' && rc=1 ;;
    identity)
        [ -n "$ARG" ] || { printf 'no range\n' >&2; exit 2; }
        printf '[pii-scan] scanning commit identity in %s...\n' "$ARG" >&2
        run_and_check scan_identity "$ARG" || rc=1 ;;
    range)
        [ -n "$ARG" ] || { printf 'no range\n' >&2; exit 2; }
        printf '[pii-scan] scanning commit messages in %s...\n' "$ARG" >&2
        out=$(git log --format='%H%n%B' "$ARG" 2>/dev/null | scan_text_stream "commit-log" both 2>&1 || true)
        [ -n "$out" ] && printf '%s\n' "$out" >&2
        printf '%s' "$out" | grep -qE '^  \[' && rc=1 ;;
    # A mode the parser accepts but this dispatch forgets would otherwise
    # report "clean" without scanning anything — the worst failure mode a
    # leak scanner has. Fail loudly instead.
    *)  printf 'internal error: mode "%s" has no dispatch branch\n' "$MODE" >&2; exit 2 ;;
esac

if [ "$rc" != 0 ] && [ "$MODE" = identity ]; then
    cat >&2 <<'MSG'

[pii-scan] PUBLISHABLE-UNSAFE COMMIT IDENTITY.
  An author/committer address on a public repo must be a GitHub noreply
  address — a personal one cannot be removed by a later commit, only by
  rewriting history and force-pushing. Fix the identity for THIS repo:

    git config user.email "<id>+<login>@users.noreply.github.com"

  (the exact address is on GitHub → Settings → Emails). For commits that
  already carry it, see docs/maintainer/history-sanitize-runbook.md.
MSG
elif [ "$rc" != 0 ]; then
    cat >&2 <<'MSG'

[pii-scan] BLOCKING LEAK — commit/push blocked.
  A denylisted business name or credential material was found. Remove it,
  or if it is a false positive add a narrow regex to scripts/pii-scan.allow.
  Real store names belong only in your private notes and the untracked
  .pii-denylist — never in a tracked file, a commit message, or a PR body.
  See docs/maintainer/pii-scanning.md.
MSG
else
    printf '[pii-scan] clean\n' >&2
fi
exit $rc
