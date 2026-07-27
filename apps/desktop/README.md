# dbboard desktop — Tauri 2 + SvelteKit spike

A **thin vertical spike** proving dbboard's UI can move off egui onto the same
stack md-business uses (Tauri 2 + SvelteKit), without touching the data path.
See [ADR-0059](../../docs/decisions.md) for the why.

One screen: **pick a connection → run a SELECT → see a result grid**, driven by
the real `connections.toml` and secrets in the OS keychain.

## How it fits together

```
apps/desktop/
├── src/                     SvelteKit frontend (static SPA, ssr=false)
│   ├── routes/+page.svelte     the one screen
│   └── lib/api.ts              typed wrappers over `invoke`
├── src-tauri/               Rust shell (crate: dbboard-desktop)
│   └── src/lib.rs              3 commands over McpService
└── build/                   prerendered shell (git-ignored)
```

The three Tauri commands (`list_connections`, `list_tables`, `run_read_query`)
are thin wrappers over **`McpService`** — the same egui-free service the MCP
server ships. Config, keychain secrets, adapter connect, and the
engine-enforced **read-only** query path are all reused; the spike adds no new
DB code.

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

The window opens on the one screen. It reads the same platform-default
`connections.toml` as the GUI and MCP server, so whatever connections you have
configured appear in the dropdown.

## Build the frontend only

```sh
pnpm build          # vite build → build/ (prerendered static shell)
pnpm check          # svelte-check (type check)
```

## Supply-chain notes

`pnpm-workspace.yaml` holds the security config (pnpm 11 reads it there, **not**
from `package.json`'s `pnpm` field or `.npmrc`):

- `minimumReleaseAge: 1440` — quarantine packages published in the last 24h.
- `allowBuilds` / `onlyBuiltDependencies` — only **esbuild** may run an install
  script, and only to link its already-present prebuilt platform binary.

## Status

Spike, not a committed migration. The egui app (`apps/dbboard`) remains the
shipping UI. If the maintainer accepts the direction after trying this, the
full screen-by-screen migration follows; if not, `apps/desktop` is removed.
