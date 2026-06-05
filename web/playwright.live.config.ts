import { defineConfig } from '@playwright/test';

// Smoke-tests the deployed GitHub Pages site (no local webServer).
export default defineConfig({
  testDir: './tests/live',
  timeout: 90_000,
  fullyParallel: false,
  use: { headless: true },
});
