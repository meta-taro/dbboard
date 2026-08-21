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
| `.claude/issues/` | Task tracking — one Markdown file per issue. |
| `.claude/project-status.md` | Running session status (internal). |

When a phase ships, mark it complete in `docs/roadmap.md`. When a
non-trivial decision is made, add an ADR entry to `docs/decisions.md`.

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
  request to `develop` and `main` (ADR-0117). It was a suggestion until
  v0.10.0, and in that time it went red without anyone noticing.
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
  bypass; it is not one any more, because the hooks no longer trigger it (see
  "Mandatory Verification Commands"). A crash there is now a real finding.
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
