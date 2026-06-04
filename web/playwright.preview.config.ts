import { defineConfig } from '@playwright/test';

// Verifies the PRODUCTION build (vite build + preview) under the /opornik/ base —
// catches base-path / worker / wasm-URL issues before deploying to Pages.
export default defineConfig({
  testDir: './tests/preview',
  timeout: 90_000,
  use: { baseURL: 'http://localhost:4173', headless: true },
  webServer: {
    command: 'npm run build && npm run preview',
    url: 'http://localhost:4173/opornik/',
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
