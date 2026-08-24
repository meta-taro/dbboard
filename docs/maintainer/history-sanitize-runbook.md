# Runbook — purge sanitized strings from git history

**Status:** executed on 2026-08-21. The rewrite described below has been
carried out and force-pushed; `develop`, `main`, the nine other branches and
all eleven tags now carry clean history. This file is kept as the procedure
of record — it has been corrected against what the run actually required, so
the steps here are the ones that worked, not the ones that were planned.

> **This rewrites every commit hash from the first affected commit onward.**
> It breaks existing clones and invalidates every hash referenced in `docs/`,
> `.claude/`, issues and PR bodies. Do it on a mirror, and have a **human**
> run the force-push (baseline §6).

## Why the mapping is not in this file

The whole point is to keep the real strings out of the repo. Listing them
here — even as the left-hand side of a replacement — would put them right
back into a tracked, public file. **The real→placeholder mapping lives in
the maintainer's private notes only** (outside this repository). Build the
`replacements.txt` below from that private mapping on your local machine and
never commit it.

Generate it without ever echoing a real name, and **sort by source length,
descending** — filter-repo applies rules in file order, so a longer name that
contains a shorter one must be consumed first:

```sh
awk -F'|' '/^\| `/ { gsub(/[` ]/,"",$2); gsub(/[` ]/,"",$3);
  if ($2!="" && $3!="" && $2!="Realname") print length($2)"\t"$2"\t"$3 }' "$PRIVATE_MAP" \
  | sort -rn | awk -F'\t' '{print "literal:"$2"==>"$3}' > replacements.txt
```

## Prerequisites

- `git filter-repo` (`python -m pip install git-filter-repo`). `filter-repo
  --version` prints a commit hash rather than a semver; that is normal.
- A **fresh mirror clone** to operate on — never filter-repo your checkout:

  ```sh
  git clone --mirror git@github.com:meta-taro/dbboard.git dbboard-mirror.git
  cd dbboard-mirror.git
  ```

- A second, untouched mirror kept as insurance (`pristine.git`), plus a copy
  of the working checkout's `.git` and `git bundle create all-refs.bundle --all`.
  **These backups contain the strings you are removing — never push them.**
  `pristine.git` becomes irreplaceable the moment the force-push lands: it is
  the only remaining source of the original→new hash mapping.

## The rewrite is three passes, not one

This is the most important correction to the original runbook. Each
filter-repo option covers a different part of the object graph, and running
only the first leaves real names in the repository:

| Pass | Option | Covers | Does **not** cover |
|---|---|---|---|
| 1 | `--replace-text replacements.txt` | **blob contents only** | commit messages, tag messages, author fields |
| 2 | `--mailmap mailmap` | author/committer identity (ADR-0084) | anything textual |
| 3 | `--replace-message replacements.txt` | **commit and tag messages** | blobs |

```sh
git filter-repo --replace-text replacements.txt
git filter-repo --mailmap mailmap --force
git filter-repo --replace-message replacements.txt --force
```

Pass 3 was discovered only during verification: a 2026-07 commit message
still named two stores after pass 1 reported success. `--replace-text` is
documented as operating on file contents, and it does exactly that.

The `mailmap` file names the address you are removing, so it must never be
committed. Build it without printing the address:

```sh
OLD=$(git log --all --format='%ae%n%ce' | grep -i 'gmail[.]com$' | sort -u | head -1)
NEW=$(git log --all --format='%ae%n%ce' | grep -i 'noreply[.]github[.]com$' | sort -u | head -1)
printf 'metataro <%s> <%s>\n'          "$NEW" "$OLD" >  mailmap
printf 'metataro <%s> metataro <%s>\n' "$NEW" "$OLD" >> mailmap
```

## Verify — scan every object, not every ref

Ref-walking misses unreachable leftovers. One pass over the whole object
database is both more thorough and faster:

```sh
git cat-file --batch-all-objects --batch --buffer \
  | grep -a -c -i -e '<old-substring-1>' -e '<old-substring-2>'
```

For the identity, both of these must print `0`:

```sh
git log --all --format='%ae%n%ce' | grep -c -v '@users[.]noreply[.]github[.]com'
git rev-list --count --all --author='<the-old-personal-address>'
```

**Do not grep for the mail provider's domain as a substring.**
`scripts/pii-scan.sh` carries its own self-test fixture around line 337
(`for bad in ...`), so that pattern reports hits on a perfectly clean repo.
Extract the literal old address from the backup and search for that; the
honest count is zero.

`scripts/pii-scan.sh --identity <range>` checks the same invariant and is what
CI runs, but it deliberately scans only new commits — for post-rewrite
verification use the commands above, which cover every object.

## `refs/pull/` — the residual you cannot remove

A mirror clone of a GitHub repo pulls down the server's PR snapshots. Two
things about them, both of which cost time on the real run:

1. **The glob does not match them.** PR refs are `refs/pull/N/head`, three
   levels deep, so `for-each-ref 'refs/pull/*'` silently matches nothing.
   Use the prefix form `'refs/pull/'` — that matched all 197 here.
2. **GitHub rejects writes to them**, which means `git push --force --mirror`
   fails outright. Push explicit refspecs instead (below).

Deleting them locally does not delete them on GitHub. After the force-push,
`refs/heads` + `refs/tags` are clean, but `git rev-list --all` on a fresh
mirror still reaches the old commits through PR refs. Confirmed here:
644 commits via heads and tags, 1320 via `--all`.

**Only GitHub Support can purge PR refs and unreachable objects.** That
request has to come from the account owner. Until it is done, the old strings
remain reachable by anyone who deliberately enumerates `refs/pull/`, though
not by cloning, `git log`, blame, the web UI, tags, or releases.

This residual is the reason delete-and-recreate keeps getting proposed. It
was considered and **rejected**: recreating the repo would destroy 214 pull
requests and the issue history that ADRs cite by number, to close a hole
reachable only by deliberate enumeration on a repo with 0 forks, 0 stars and
0 watchers.

## Ordering — push everything first, or the rewrite undoes itself

**Do the whole rewrite only after every local branch you still intend to push
has been pushed.** A rewrite of `origin` does not touch your checkout. If you
force-push while local commits are still unpushed, those commits still carry
the old strings and the old identity, and the next ordinary `git push` puts
them straight back — silently, because to git they are just new commits.

`git pull --rebase` does not save you: it rebases the un-rewritten commits
onto the clean history and keeps their metadata.

For stale local branches you do not intend to push, **parking beats deleting**:

```sh
git for-each-ref --format='%(refname) %(objectname)' refs/heads \
  | grep -v ' refs/heads/develop$' \
  | while read -r ref sha; do
      git update-ref "refs/pre-rewrite/${ref#refs/heads/}" "$sha" && git update-ref -d "$ref"
    done
```

They stay reachable, `git push --all` cannot republish them, and no branch is
deleted — so this does not hit the §30 approval gate. 48 branches were parked
this way here.

## Force-push (human)

Check first whether there is anything to re-protect. On this repo there was
not — `gh api repos/meta-taro/dbboard/branches/develop/protection` returns 404
and the rulesets list is empty, so the force-push could not bounce. (That a
public repo's default branch is unprotected is a separate problem worth its
own decision; it is not part of this runbook.)

Confirm the local and remote ref sets match, so no deletion is attempted, then
push explicit refspecs — **not** `--mirror`, which fails on `refs/pull/`:

```sh
git push --force origin 'refs/heads/*:refs/heads/*' 'refs/tags/*:refs/tags/*'
```

Then:

- **Deal with every open PR.** `gh pr list --state open` before you start; a
  rewrite orphans each open PR's head commits.
- Tell anyone with an existing clone to re-clone — their old clones still
  contain the strings and will re-introduce them on push.
- Open the GitHub Support request for PR refs / unreachable objects.

## After the push — repoint hash references in the repo

The rewrite invalidates every commit hash written down in `docs/`, `.claude/`
and elsewhere. Here that was **427 references to 217 unique 7-character
hashes across 21 files**. Rebuild the mapping from `pristine.git` by re-running
all three passes on a copy and keeping each `filter-repo/commit-map`.

Two traps, both of which produced a confidently wrong answer on the first
attempt:

- **filter-repo composes `commit-map` across successive runs.** After pass 3,
  its `commit-map` already maps *original* → *final*. Chaining the maps from
  passes 1 and 2 by hand yields intermediate hashes that exist in no
  repository. Use the map from the **last** pass only.
- **`git rev-parse --short=7 <40-hex>` does not check that the object
  exists.** Any syntactically valid hash yields a plausible short form. The
  first run produced 217 abbreviations, all of them fabricated, and nothing
  complained. Validate each one:

  ```sh
  git cat-file -e "${short}^{commit}"   # and confirm rev-parse "$short" == the full hash
  ```

Substitute in a **single pass** (a regex callback, not repeated
search-and-replace) so a new hash can never be rewritten again by a later
rule, and read/write in binary so line endings survive. Prove the result:
re-apply the substitution to the `HEAD` blob of each file and compare with
the working tree — it should match byte for byte, modulo the CRLF that
checkout adds on Windows.

Python writing these intermediate map files in text mode appends CRLF on
Windows, which silently breaks every later `git rev-parse`. Pipe them through
`tr -d '\r'` before use.

## Residual-risk note

The repository was public while the strings were present, so treat them as
potentially already copied or indexed. History rewrite reduces
discoverability but cannot guarantee erasure from third parties. The strings
are business *names*, not secrets — no token, password or key was ever
exposed (those live only in the OS keychain).
