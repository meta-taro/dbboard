# Runbook — purge sanitized strings from git history

**Status:** current-tree sanitization is already merged (connection ids
and sample row data are neutral placeholders — `store-a` / `store-b` /
`store-c`, `Alpha` / `Beta`). This runbook covers the remaining,
**destructive** step: removing the old strings from **past commits** as
well, so `git log -S` can no longer surface them.

> **This rewrites every commit hash from the first affected commit
> onward.** It breaks existing clones, open PRs, and forks, and requires
> a force-push to `develop` (and `main`). Do it deliberately, on a mirror,
> with the force-push done by a human. If in doubt, the current-tree
> sanitization already stops all *future* exposure — history rewrite is
> defense-in-depth, not a correctness requirement.

## Why the mapping is not in this file

The whole point is to keep the real strings out of the repo. Listing them
here — even as the left-hand side of a replacement — would put them right
back into a tracked, public file. **The real→placeholder mapping lives in
the maintainer's private notes only** (outside this repository). Build the
`replacements.txt` below from that private mapping on your local machine
and never commit it.

## Prerequisites

- `git filter-repo` installed (`pipx install git-filter-repo`, or see
  <https://github.com/newren/git-filter-repo>). The built-in
  `git filter-branch` is not a supported alternative here — it is slow and
  error-prone.
- A **fresh mirror clone** to operate on (never run filter-repo against
  your working checkout):

  ```sh
  git clone --mirror git@github.com:meta-taro/dbboard.git dbboard-mirror.git
  cd dbboard-mirror.git
  ```

## Step 1 — build the local replacements file

Create `replacements.txt` (in the mirror dir, untracked) with one line per
string, using the private real→placeholder mapping. Format
(`git filter-repo --replace-text`):

```
# One line per string. Left side = the exact old string (from private
# notes); right side = the placeholder already used in the current tree.
literal:<old-d1-id>==>store-a
literal:<old-aurora-id>==>store-b
literal:<old-supabase-id>==>store-c
literal:<old-display-name>==>Store C
literal:<old-sample-row-1>==>Alpha
literal:<old-sample-row-2>==>Beta
```

Because the ids appear as substrings of the keyring references
(`dbboard.<id>.token` etc.), replacing the id string also fixes those
references in one pass — no separate lines needed for them.

## Step 2 — rewrite history

```sh
git filter-repo --replace-text replacements.txt
```

## Step 3 — verify zero hits remain

```sh
# Should print nothing:
git grep -I -i -e '<old-substring-1>' -e '<old-substring-2>' $(git rev-list --all)
```

Also spot-check a few historical commits that originally introduced the
strings (see the private notes for the commit list) to confirm the tree at
those revisions is clean.

## Step 4 — force-push (human)

Only after Step 3 is clean:

```sh
git push --force --mirror git@github.com:meta-taro/dbboard.git
```

Then:

- Re-protect `develop` / `main` branch rules if the mirror push reset them.
- Tell anyone with an existing clone to re-clone (their old clones still
  contain the strings and will re-introduce them on push).
- Note that GitHub may retain old commit objects reachable by SHA for a
  while, and existing forks are independent copies — contact GitHub
  Support if a hard guarantee is required.

## Residual-risk note

The repository was public while the strings were present, so treat them as
potentially already copied/indexed. History rewrite reduces discoverability
but cannot guarantee erasure from third parties. The strings are business
*names*, not secrets — no token, password, or key was ever exposed (those
live only in the OS keychain).
