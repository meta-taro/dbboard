# PII / secret leak scanning (operator guide)

dbboard is developed against real, business-identifying databases but shipped
as a public repository. This is the **preventive** guard that keeps real store
names, credentials, and maintainer PII out of the public repo — on every
commit, on every commit message, and once a day in CI. It is the companion to
the one-time [`history-sanitize-runbook.md`](./history-sanitize-runbook.md),
which removes names that already landed in history.

See [ADR-0055](../decisions.md) for the rationale.

## What runs where

| Trigger | Command | Blocks? |
|---|---|---|
| pre-commit hook | `pii-scan.sh --staged --reveal` | yes — staged content |
| commit-msg hook | `pii-scan.sh --message <file> --reveal` | yes — the message text |
| CI push/PR/daily | `--selftest`, `--tree`, `--range origin/main..HEAD` | yes — tracked files + new commit messages |

The hooks are installed by cargo-husky from `.cargo-husky/hooks/` on the next
`cargo test` after this lands. `--reveal` is passed locally (private terminal)
but **never** in CI (public Actions log).

## Two severities

The scanner splits rules by how much a database client's own test suite trips
them — it is full of synthetic connection strings and example emails.

- **BLOCKING** (fails the commit / CI):
  - **denylist literals** — the real names/PII from `.pii-denylist` (below).
    This is the primary mechanism; matched exactly and **redacted** in output.
  - **private-key** — PEM `BEGIN … PRIVATE KEY` blocks.
  - **aws-access-key-id** — a real-looking `AKIA…` key id.
- **ADVISORY** (printed in the daily `--tree`/`--range` scan, never fails):
  - **passworded-db-url**, **personal-email**, **windows-home-path**.
  By project invariant real secrets live only in the OS keyring, never in a
  tracked file, so a passworded URL in the tree is a fixture — worth a glance,
  not a build break. A genuinely new personal email still surfaces here. To
  make a specific known value blocking, add it to the denylist.

## The denylist (the real strings)

The real store names, the maintainer's personal email / full name / OS
username, and any production hostnames go in a **denylist that is never
committed**:

- **Locally:** copy [`.pii-denylist.example`](../../.pii-denylist.example) to
  `.pii-denylist` at the repo root (gitignored) and fill it in, one literal per
  line. Fixed strings, case-insensitive.
- **In CI:** put the same lines in the `PII_DENYLIST` repo secret (Settings →
  Secrets and variables → Actions). The workflow materializes it into
  `.pii-denylist` for the run and shreds it afterwards.

Keep the two in sync. Without a denylist the scan degrades to generic rules
only — it does not fail; it just loses literal-name detection.

## False positives — the allowlist

`scripts/pii-scan.allow` holds narrow EREs for known-safe shapes (placeholder
emails, example connection strings, `C:\Users\<placeholder>` docs paths). When
an advisory shape rule hits a genuine fixture, add a **narrow** regex there.
Never add a real name or credential to the allowlist — the denylist takes
precedence and cannot be allowlisted anyway.

## Running it by hand

```sh
sh scripts/pii-scan.sh --selftest              # prove the rules fire
sh scripts/pii-scan.sh --tree                  # scan tracked files at HEAD
sh scripts/pii-scan.sh --tree --reveal         # ... showing generic matches
sh scripts/pii-scan.sh --range origin/main..HEAD   # scan new commit messages
sh scripts/pii-scan.sh --message .git/COMMIT_EDITMSG
```

Exit status: `0` clean, `1` a blocking leak, `2` usage error.

## When a commit is blocked

1. Read the finding. Denylist hits print `[denylist#<sha8>] file:line (match
   redacted)` — the `<sha8>` identifies which denylist entry without printing
   it. Generic blocking hits (`private-key`, `aws-access-key-id`) print the
   rule and location; run with `--reveal` locally to see the text.
2. If it is a **real leak**: remove it. Real store names belong only in your
   private notes and the untracked `.pii-denylist` — never in a tracked file, a
   commit message, or a PR body.
3. If it is a **false positive**: add a narrow regex to `scripts/pii-scan.allow`.
4. The **only** sanctioned `--no-verify` bypass in this repo is the Windows
   libSQL teardown segfault — never use it to skip a PII finding.

## Scope note: history

CI scans HEAD and *new* commit messages, not full history. History still holds
un-remediated real names pending the destructive one-time rewrite in
`history-sanitize-runbook.md`; scanning all of it would be permanently red and
bury the live signal. Remediate history via that runbook, not this scanner.
