import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Tauri expects a fixed dev server URL (matches build.devUrl in
// tauri.conf.json). strictPort makes a port clash fail fast instead of
// silently shifting the URL out from under Tauri. clearScreen:false keeps the
// Rust-side logs from being wiped by Vite.
export default defineConfig({
  plugins: [sveltekit()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
    // The About dialog imports the repo's CHANGELOG.md, which sits two levels
    // above this app. A production build inlines it, but the dev server
    // refuses to serve outside its root unless told — and the failure is a
    // dialog with no release notes, in dev only.
    fs: { allow: ['../..'] },
  },
});
