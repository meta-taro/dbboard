# dbboard

A high-performance desktop database client for modern serverless and
distributed databases.

dbboard is a learning and reference project that explores multi-database
integration, local-first tooling, and pluggable AI-assisted workflows. It
exposes a unified, native UI for Neon, Supabase, and Turso/libSQL, with an
adapter-based architecture that makes adding new databases straightforward.

## Status

Early development. See [`docs/roadmap.md`](docs/roadmap.md) for the current
phase.

This is the **desktop** implementation. The web counterpart lives at
[meta-taro/dbboard-web](https://github.com/meta-taro/dbboard-web) (Nuxt +
NestJS). The two share concepts and feature parity goals but are
independent codebases.

## Goals

- **Performance first** — native Rust UI built on egui.
- **Local first** — no required external services to run.
- **Modular** — database and AI layers are decoupled.
- **Extensible** — new databases and AI providers can be added behind traits.

## Supported Databases (initial scope)

- Neon (PostgreSQL)
- Supabase (PostgreSQL + API)
- Turso / libSQL (SQLite-based distributed DB)

The Turso adapter ships first. Neon and Supabase follow once the adapter
trait is extracted (see roadmap).

## Architecture

Three main layers, organised as a cargo workspace:

- **UI layer** — Rust + egui, native desktop interface.
- **Database adapter layer** — abstracts database-specific logic behind a
  single trait so multiple providers plug in.
- **AI integration layer (optional)** — pluggable providers (Claude,
  OpenAI, local LLMs). Isolated from core DB operations.

See [`docs/architecture.md`](docs/architecture.md) for the full crate map
and dependency rules.

## Requirements

- Rust stable (latest)
- `cargo` (bundled with Rust)
- A C/C++ toolchain for `libsql` native deps:
  - Windows: MSVC Build Tools
  - macOS: Xcode Command Line Tools
  - Linux: `build-essential`

## Setup

```sh
git clone https://github.com/<your-org>/dbboard.git
cd dbboard
cargo test
```

Running `cargo test` once installs the `cargo-husky` git hooks
(pre-commit, pre-push).

## Run

```sh
cargo run -p dbboard
```

## Development

Before committing, the pre-commit hook runs:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets --all-features
cargo test --all-features
```

Before pushing, the pre-push hook also runs:

```sh
cargo build --release
cargo test --all-features --release
```

You can run these manually at any time.

## Contributing

This project follows the rules in [`CLAUDE.md`](CLAUDE.md). In short:

1. Write a failing test before changing behaviour.
2. Keep changes small and focused.
3. Use conventional-style commit messages in English.
4. Record non-trivial decisions in [`docs/decisions.md`](docs/decisions.md).

## License

See [`LICENSE`](LICENSE).
