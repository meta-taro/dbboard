import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';

// Unit tests only — deliberately NOT the SvelteKit Vite config, so they don't
// drag in the sveltekit() plugin or a browser environment. The svelte plugin is
// here only so a test may import a `.svelte.ts` module and have its runes
// compiled; no component is ever mounted.
export default defineConfig({
  plugins: [svelte({ compilerOptions: { hmr: false } })],
  test: {
    include: ['src/**/*.{test,spec}.ts'],
    environment: 'node',
    // Vitest stubs CSS to an empty string by default, which is right for
    // components (no test mounts one) and wrong for `scoped-classes.test.ts`,
    // which reads the global sheets to know which classes any component may
    // use. Without this it sees every sheet as empty and reports the whole
    // app as unstyled.
    css: true,
  },
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
});
