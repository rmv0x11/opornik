import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf8'),
);

// GitHub Project Pages are served from /<repo>/, so the production build (and
// `vite preview`, which serves that build) use that base; the dev server stays at
// root so the dev e2e can use `/`.
export default defineConfig(({ command, isPreview }) => ({
  base: command === 'build' || isPreview ? '/opornik/' : '/',
  plugins: [svelte()],
  worker: { format: 'es' },
  // app version (from package.json) surfaced in the UI and for release tracking
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
}));
