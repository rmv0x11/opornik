import { test, expect } from '@playwright/test';

// Production build served under /opornik/ (as on GitHub Pages).
test('production build loads the engine and is playable under /opornik/', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });

  await page.goto('/opornik/');
  await page.getByRole('button', { name: /Начать партию/ }).click();
  // Engine loaded in the worker from /opornik/ assets ⇒ roll button appears.
  await expect(page.getByRole('button', { name: /Бросить кости/ })).toBeVisible({ timeout: 40_000 });
  await page.getByRole('button', { name: /Бросить кости/ }).click();
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });

  expect(errors, `console errors: ${errors.join(' | ')}`).toHaveLength(0);
});
