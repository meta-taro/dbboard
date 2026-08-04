import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// Tauri loads the built SPA from the filesystem in a webview, so we prerender
// to static assets with an index.html fallback and disable SSR at the root
// layout. Matches the reference stack (md-business desktop).
/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({ fallback: 'index.html' }),
  },
};

export default config;
