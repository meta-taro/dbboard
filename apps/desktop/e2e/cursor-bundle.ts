// Bundles src/lib/agent/cursor.ts into one script a test page can load, with
// the same toolchain the app is built with.
import { build } from 'vite';
import { fileURLToPath } from 'node:url';

let cached: string | undefined;

export async function cursorBundle(): Promise<string> {
  if (cached) return cached;
  const entry = fileURLToPath(new URL('../src/lib/agent/cursor.ts', import.meta.url));
  const out = await build({
    configFile: false,
    logLevel: 'silent',
    build: {
      write: false,
      minify: false,
      lib: { entry, name: 'AgentCursorLib', formats: ['iife'], fileName: () => 'cursor.js' },
    },
  });
  const outputs = Array.isArray(out) ? out : [out];
  const chunk = outputs.flatMap((o) => ('output' in o ? o.output : [])).find((c) => c.type === 'chunk');
  if (!chunk || chunk.type !== 'chunk') throw new Error('the cursor did not bundle');
  cached = chunk.code;
  return cached;
}
