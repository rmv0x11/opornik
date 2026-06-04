import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// GitHub Project Pages are served from /<repo>/, so the production build (and
// `vite preview`, which serves that build) use that base; the dev server stays at
// root so the dev e2e can use `/`.
export default defineConfig(({ command, isPreview }) => ({
  base: command === 'build' || isPreview ? '/opornik/' : '/',
  plugins: [svelte()],
  worker: { format: 'es' },
}));
