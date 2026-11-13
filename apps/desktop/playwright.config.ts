// End-to-end tests of the agent cursor (ADR-0177). They load the cursor
// module itself into a real browser page, so they test the code the app
// runs without starting Tauri or a database.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  timeout: 20_000,
  use: { viewport: { width: 800, height: 600 } },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
});
