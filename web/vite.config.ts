import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { VitePWA } from 'vite-plugin-pwa';

const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf8'),
);

// GitHub Project Pages are served from /<repo>/, so the production build (and
// `vite preview`, which serves that build) use that base; the dev server stays at
// root so the dev e2e can use `/`.
export default defineConfig(({ command, isPreview }) => ({
  base: command === 'build' || isPreview ? '/opornik/' : '/',
  plugins: [
    svelte(),
    VitePWA({
      // 'prompt': a new deploy must not yank the page out from under a game in
      // progress — main.ts shows a toast and the user reloads when ready
      registerType: 'prompt',
      includeAssets: ['icons/apple-touch-icon.png', 'icons/favicon-64.png', 'icons/icon.svg'],
      manifest: {
        name: 'Опорник — длинные нарды',
        short_name: 'Опорник',
        description: 'Длинные нарды против движка на нейросетях. Работает офлайн.',
        lang: 'ru',
        display: 'standalone',
        background_color: '#f7f3ec',
        theme_color: '#f7f3ec',
        icons: [
          { src: 'icons/pwa-192.png', sizes: '192x192', type: 'image/png' },
          { src: 'icons/pwa-512.png', sizes: '512x512', type: 'image/png' },
          { src: 'icons/maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
        ],
      },
      workbox: {
        // the whole game must work offline: app shell + engine WASM + the
        // evaluation-net weights (nardy-v2-gen3.bin, ~1.5 MB)
        globPatterns: ['**/*.{js,css,html,svg,png,wasm,bin,woff2}'],
        // headroom for future model generations (default cap is 2 MiB)
        maximumFileSizeToCacheInBytes: 8 * 1024 * 1024,
      },
    }),
  ],
  worker: { format: 'es' },
  // app version (from package.json) surfaced in the UI and for release tracking
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
}));
