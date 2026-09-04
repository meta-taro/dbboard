# CLAUDE.md — dbboard AI Agent Rules

This file is the integrated rule set for AI coding agents (and humans) working on
**dbboard**. It defines package management, testing, architecture, commit, review,
and documentation policies. Read this file before making any change.

## Project Snapshot

- **What**: High-performance desktop database client for modern serverless and
  distributed databases (Neon, Supabase, Turso/libSQL initially).
- **Where**: Public repo <https://github.com/meta-taro/dbboard>. Builds are
  published on the download page <https://meta-taro.github.io/dbboard/> — quote
  that URL when anyone asks where to get dbboard, including in sibling repos.
- **Stack**: Rust core; the client is a Tauri 2 shell with a SvelteKit
  frontend. Pluggable database adapters, optional AI provider layer.
- **Why**: Learning and reference project for multi-DB integration, local-first
  tooling, and pluggable AI workflows.

## Package Management

- Use **cargo** as the sole package manager.
- Commit `Cargo.lock` for binaries (this is a binary project).
- Prefer well-maintained crates. Avoid abandoned or experimental ones unless
  the trade-off is recorded in `docs/decisions.md`.
- When adding a non-trivial crate, write a short ADR entry in
  `docs/decisions.md`.

## Tech Selection Principles

- Prefer the current Rust stable edition.
- The compiler itself is pinned in `rust-toolchain.toml` to an exact version.
  Because `clippy -D warnings` makes the lint set part of the build, an
  unpinned toolchain lets a runner-image update break a branch nobody touched
  (ADR-0139). Bump it deliberately, with the lint fixes the bump requires in
  the same commit.
- Avoid crates with frequent breaking changes unless the value is clear.
- Confirm the latest stable version of major libraries (tauri, tokio, sqlx,
  libsql, etc.) before pinning.

## Test-First Development (mandatory)

- **Before changing behaviour, add a failing test.** Then make it pass.
- After implementation, update any existing tests affected by the change.
- Unit tests live in `#[cfg(test)] mod tests` inside the source file.
- Integration tests live in `crates/<crate>/tests/`.
- Target coverage: meaningful tests for every public function and every
  non-trivial branch. Hard percentage targets are secondary to coverage of
  behaviour.

## Mandatory Verification Commands

Run before every commit:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
sh scripts/cargo-test-serialised.sh
```

Run before every push (in addition to the above):

```sh
cargo build --release
sh scripts/cargo-test-serialised.sh --release
```

`scripts/cargo-test-serialised.sh` runs `cargo test` over the whole workspace,
except that the crates named in `scripts/libsql-serialised-crates.txt` run one
thread at a time. A parallel run there tears down two in-memory libSQL
databases at once, which crashes the test binary on Windows after every
assertion has passed.

That list is every crate that can reach `dbboard-turso` in the dependency
graph, not just the adapter itself: the hazard travels with the dependency.
Do not edit it by hand — `crates/dbboard-turso/tests/serialised_teardown.rs`
derives the set from the workspace manifests and tells you what it should
contain.

Serialising makes that crash rare, not impossible, so the script re-runs a
crate once when it dies with `0xc0000005` and no test reported a failure
(ADR-0125). A second crash in a row fails the run, and a genuine test failure
is never retried.

These commands are wired into the git hooks (see "Git Hooks" below).

## Code Quality Standards

- Prefer readable code over clever code.
- Keep functions small and single-purpose. Soft limit: 50 lines.
- Keep files focused. Soft limit: 500 lines. Hard limit: 800.
- Avoid nesting deeper than 4 levels; prefer early returns.
- Handle errors explicitly with `Result<T, E>`. Avoid `unwrap()` outside of
  tests and statically infallible paths.
- **Comment the *why*, not the *what*.** Add comments for non-obvious logic,
  hidden constraints, and workarounds. Do not narrate what the code already
  says.
- Mark temporary code with `// TODO(short-reason)` or `// FIXME(reason)`.

## Architecture

Layered separation is enforced via the cargo workspace:

| Layer | Crate / Path | Responsibility |
|---|---|---|
| Domain | `crates/dbboard-core` | Adapter trait, value types (Query, Row, Schema), errors. No I/O. |
| Adapters | `crates/dbboard-turso`, `crates/dbboard-neon`, `crates/dbboard-supabase` | Concrete DB implementations of the core trait. |
| AI (optional) | `crates/dbboard-ai` | Pluggable AI provider trait; no hard dependency on any specific provider. |
| Presentation | `apps/desktop/src` | SvelteKit frontend. Talks to the shell over Tauri IPC; never to a database. |
| App | `apps/desktop/src-tauri` | Tauri shell. Wires concrete adapters and the frontend together. |

Rules:

- **No business logic in UI event handlers.** It belongs in `dbboard-core` or
  a use-case module that lives next to the trait it uses.
- **Adapters depend on `dbboard-core` only.** They never depend on the client.
- **`dbboard-core` depends on nothing in this workspace.** It defines the
  contracts everything else implements.

See `docs/architecture.md` for the trait sketches and dependency diagram.

## Releases

Every version has its contents reserved in advance. `docs/roadmap.md`
("Release plan") holds one slot per version up to 1.0, then bands of several
releases each for the eight phases of the handed-over Database Workspace plan
(`.claude/plans/`). Before starting anything, know which slot it belongs to;
if it belongs to none, the plan is what to change first, not the code.

A slot is a reservation, not a deadline. Unfinished content moves to the next
slot and slots are never renumbered, so no release ever waits on its slowest
initiative (ADR-0110, as amended by ADR-0122).

**When a slot is full, cut it before starting the next one.** Code that is
written but not tagged has reached nobody. What is due is not a judgement call:

```sh
node scripts/release-due.mjs
```

One unreleased entry means a release *may* be cut; three mean one is *due*.
The same line prints on every push. Deciding to release, and pushing the tag,
stay human (ADR-0121).

Every `CHANGELOG.md` version heading carries its date and its slot's headline
(`## [0.11.0] — 2026-08-22 — Connection repair and duplication`), so "what
shipped?" is answerable without reading a diff. `scripts/release-plan.test.mjs`
fails when the plan and the changelog drift apart.

Cutting one is a single command, so that the mechanical half is never the
reason to put a release off:

```sh
node scripts/release-cut.mjs        # or: … release-cut.mjs 0.12.0
```

It rewrites the changelog heading, the workspace version and both manifests,
then prints what is left: `cargo check` to move `Cargo.lock`, the commit, and
the tag. It stops there deliberately — the tag push is the release, and that
is the human's (ADR-0121).

## Git & Commits

- Commits are authored by the agent. **Pushes are done by the human.**
- Commit in small, focused chunks per phase or per logical change.
- Commit messages are written in **English** (this is an OSS project).

### Branching

- **`develop`** is the integration branch and the repo default. Day-to-day
  work merges here.
- **`main`** is reserved for tagged releases. Do not commit directly.
- Feature work happens on `feature/<short-slug>` branched from `develop`.
- Open PRs against `develop`. Release PRs merge `develop` into `main`.

### Sibling Repository

This is the **desktop** implementation. A separate web implementation
lives at <https://github.com/meta-taro/dbboard-web> (Nuxt + NestJS).
The two repos:

- Share **concepts** (adapter pattern, AI provider plugin, DB feature
  parity goals).
- Do **not** share code — they are independent codebases in different
  stacks.
- Should keep adapter feature parity in mind. Coordinate breaking
  contract changes through `docs/decisions.md` in both repos.

### Commit Message Format

```
<type>: <description>

<optional body explaining why, not what>
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.

### Pre-Push Checklist (for the human)

- [ ] Verified the change behaves correctly when run
- [ ] No accidental file changes (`git status` is clean apart from intent)
- [ ] README and docs reflect reality
- [ ] All tests pass
- [ ] No secrets or credentials in the diff
- [ ] Commit granularity is sensible

### After the push: watch CI to the end

A push is not finished when git returns. **Whoever pushed — or the agent that
prepared it — watches the checks until every job has settled, and says what
happened.** Do not announce a PR and move on while its run is still going.

```sh
gh pr checks <number>                     # until nothing reads `pending`
gh run list --branch develop --workflow ci --limit 1
```

Two reasons this is a rule rather than a habit. The hooks are a local
convenience and CI is the gate (ADR-0104), so a green pre-push says nothing
about the runners. And the `deps` job goes red **for reasons no commit caused**:
an upstream yank enters the advisory database and every branch turns red at
once, which is exactly what happened to `chacha20 0.10.1` in v0.14.0 — a red
that nobody sees until somebody looks.

A failing job's log is only readable once the *whole run* completes, so a
failure spotted early still has to be waited out before it can be diagnosed.
The same applies to a tag push: watch `release.yml` through to the published
assets, because that run is the release.

## Git Hooks

Hook scripts live in `.cargo-husky/hooks/`. Install them with:

```sh
sh scripts/install-hooks.sh
```

- **pre-commit**: `pii-scan --staged`, `cargo fmt --check`,
  `cargo clippy -D warnings`, `cargo check`, `cargo test`.
- **commit-msg**: `pii-scan --message`.
- **pre-push**: `cargo build --release`, `cargo test --release`.

Run the installer again after editing a hook. Nothing does it for you:
`cargo-husky` used to, and was dropped when the workspace was restructured
while five documents went on claiming it (ADR-0119).
`crates/dbboard-config/tests/hook_install_drift.rs` fails when `.git/hooks/`
has fallen behind its source.

The hooks are a local convenience, not the gate. CI runs the same commands
on every push and pull request (ADR-0104).

## Documentation Policy

All external-facing documentation is written in **English**. Internal
session notes (e.g. `.claude/project-status.md`) may be written in the
maintainer's preferred language.

| File | Purpose |
|---|---|
| `README.md` | Entry point: what dbboard is, how to set it up, how to run it. |
| `DESIGN.md` | Visual direction: palette, typography, spacing, components. |
| `docs/architecture.md` | Layer/crate map, adapter trait spec, dependency rules. |
| `docs/roadmap.md` | Phase plan. Update when a phase completes. |
| `docs/decisions.md` | ADR log for technical decisions. Append, do not rewrite. |
| `.claude/plans/` | Handed-over plans, stored verbatim. Never edited. |
| `.claude/issues/` | Task tracking — one Markdown file per issue. |
| `.claude/project-status.md` | Running session status (internal). |

When a phase ships, mark it complete in `docs/roadmap.md`. When a
non-trivial decision is made, add an ADR entry to `docs/decisions.md`.

### Plans arrive as files, and are kept as files

A plan handed over as a `.md` goes into `.claude/plans/<date>-<slug>.md`
**byte for byte, before anything is done with it**. Then, separately, derive
whatever issues it implies and link each one back to the file it came from.

Not the other way round. A plan read once and turned straight into issues
survives only as whatever the reader thought was important that day: three
handovers of 360–880 lines each became issues of 140–150 lines, and a fourth
was never written down at all. The parts that get dropped are the reasons —
why this database, why not that ordering, what was already ruled out — and
those are exactly the parts needed later, when the summary no longer answers
the question. The original is a few hundred lines; keeping it costs nothing
and the reader six months from now cannot recover it.

Store it whole even when it is obviously going to be reshaped, even when only
part of it applies, and even when it repeats something already known.
Summarise in the issue, not in the archive.

## Local Development

- Provide a `.env.example` if and when environment variables are
  introduced.
- Document setup in `README.md`.
- Install the git hooks with `sh scripts/install-hooks.sh` after cloning.

## Security

Run a lightweight security review when:

- Adding a new dependency (check downloads, maintenance, license).
- Adding code that handles DB credentials or user secrets.
- Adding network-facing code or AI provider integration.
- Adding GitHub Actions workflows.

Tooling:

- `cargo deny check` covers licenses and RustSec advisories. It is **not a
  suggestion** — the `deps` job in `ci.yml` runs it on every push and pull
  request to `develop` and `main` (ADR-0117), **and nightly** (ADR-0144). It
  was a suggestion until v0.10.0, and in that time it went red without anyone
  noticing. The nightly exists because this job answers a question that
  changes without a commit: two upstream yanks in eight days (`chacha20`,
  `wnaf`) each turned every branch red, and each was found by someone opening
  a pull request for unrelated reasons.
- Anything `deny.toml` ignores carries a per-advisory `reason`. Add entries
  the same way: one line per advisory, saying why this one cannot be fixed
  today and what would clear it. A blanket suppression is not acceptable
  here, because it hides the next one too.
- `cargo audit` is not installed or run; `cargo deny check advisories` reads
  the same RustSec database.

### PII / secret leak scanning (ADR-0055)

This repo is public but developed against real, business-identifying
databases. `scripts/pii-scan.sh` blocks real store names, credentials, and
maintainer PII from entering the repo — on every commit (pre-commit hook),
every commit message (commit-msg hook), and daily in CI (`pii-scan.yml`).

- Real store names / personal email / OS username go ONLY in the untracked
  `.pii-denylist` (locally) and the `PII_DENYLIST` CI secret — never in a
  tracked file, a commit message, or a PR body. Template:
  `.pii-denylist.example`.
- A blocked commit means a real leak (remove it) or a false positive (add a
  narrow regex to `scripts/pii-scan.allow`). Never `--no-verify` past a PII
  finding. The Windows libSQL teardown segfault used to be a sanctioned
  bypass; it is not one any more, because the runner now retries it once by
  itself (see "Mandatory Verification Commands"). A crash that survives the
  retry is a real finding.
- Operator guide: `docs/maintainer/pii-scanning.md`.

## Progress Tracking

- Update `.claude/project-status.md` at the end of each working session.
- Mark completed roadmap phases in `docs/roadmap.md`.
- Track in-flight tasks in `.claude/issues/` until they graduate to
  GitHub Issues.

## Contributor Workflow

1. Read `CLAUDE.md`, `README.md`, `DESIGN.md`.
2. Skim `docs/architecture.md` and `docs/roadmap.md`.
3. Pick a task from `.claude/issues/` or the roadmap.
4. Write a failing test.
5. Implement until the test passes.
6. Run the mandatory verification commands.
7. Commit with a clear English message.
