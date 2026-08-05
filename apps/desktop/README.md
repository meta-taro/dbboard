# dbboard desktop — Tauri 2 + SvelteKit

**The** dbboard desktop client. Started as a spike proving the UI could move off
egui onto the stack md-business uses ([ADR-0059](../../docs/decisions.md)); it
reached parity at v0.4.0 (ADR-0062) and became the only client when the egui app
was retired (ADR-0089).

A SvelteKit static SPA in a WebView, over a Rust shell that calls the same core
crates the MCP server does. Connections come from the real `connections.toml`
and the OS keychain — the frontend never touches a database directly.

## How it fits together

```
apps/desktop/
├── src/                     SvelteKit frontend (static SPA, ssr=false)
│   ├── routes/+page.svelte     the app shell
│   └── lib/                    api.ts (typed `invoke` wrappers) + feature modules
│       ├── components/            sidebar, editor, result grid, dialogs
│       ├── grid/ sql/ query/      pure logic, unit-tested with vitest
│       └── i18n/                  11 locales, message catalogs
├── src-tauri/               Rust shell (crate: dbboard-desktop)
│   ├── src/lib.rs              commands + the app entry point
│   ├── src/ai.rs               AI assistant commands (ADR-0052)
│   └── src/dump.rs, restore.rs backup / restore (ADR-0064)
└── build/                   prerendered shell (git-ignored)
```

The read path (`list_connections`, `list_tables`, `describe_table`,
`run_read_query`) is a thin wrapper over **`McpService`** — the same service the
MCP server ships, so the engine-enforced **read-only** guarantee is one
implementation, not two. Config, keychain secrets, and adapter connect are
likewise reused; no DB logic lives in this crate.

## Prerequisites

- Rust stable (workspace toolchain) + the platform WebView (WebView2 on Windows).
- **pnpm** (npm is not used — see `~/.claude/rules/package-manager.md`).
  `corepack enable` picks up the pinned version from `packageManager`.

## Run it

```sh
cd apps/desktop
pnpm install        # first time only
pnpm tauri dev      # launches the WebView against the vite dev server
```

It reads the same platform-default `connections.toml` as the MCP server, so
whatever connections you have configured appear in the sidebar.

## Checks

```sh
pnpm check          # svelte-check (type check)
pnpm test           # vitest (frontend unit tests)
pnpm build          # vite build → build/ (prerendered static shell)
```

The Rust side is covered by the workspace commands in `CLAUDE.md`
(`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test
--all-features`).

## Supply-chain notes

`pnpm-workspace.yaml` holds the security config (pnpm 11 reads it there, **not**
from `package.json`'s `pnpm` field or `.npmrc`):

- `minimumReleaseAge: 1440` — quarantine packages published in the last 24h.
- `allowBuilds` / `onlyBuiltDependencies` — only **esbuild** may run an install
  script, and only to link its already-present prebuilt platform binary.

## Releases

Tagging `v*.*.*` builds the NSIS installer and the macOS `.dmg` here and
publishes them with checksums (ADR-0044), plus the signed `latest.json` the
in-app updater verifies (ADR-0067). See `.github/workflows/release.yml`.

Those bundles are what the public **[download page](https://meta-taro.github.io/dbboard/)**
serves (ADR-0047) — it reads the latest release from the GitHub API and offers
the asset matching the visitor's OS.
