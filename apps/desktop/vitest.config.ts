import { defineConfig } from 'vitest/config';
import { fileURLToPath } from 'node:url';

// Pure-logic unit tests only — deliberately NOT the SvelteKit Vite config, so
// unit tests don't drag in the sveltekit() plugin or a browser environment.
export default defineConfig({
  test: {
    include: ['src/**/*.{test,spec}.ts'],
    environment: 'node',
  },
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
});
