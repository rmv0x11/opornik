import { test, expect } from '@playwright/test';

// Production build served under /opornik/ (as on GitHub Pages).
test('production build loads the engine and is playable under /opornik/', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });

  await page.goto('/opornik/');
  await page.getByRole('button', { name: /Начать партию/ }).click();
  // Engine loaded in the worker from /opornik/ assets ⇒ opening-roll button appears.
  await page.getByRole('button', { name: /Разыграть первый ход/ }).click({ timeout: 40_000 });
  // ФСНР: after the opening the human either moves first (sources) or rolls first.
  await page.waitForFunction(
    () =>
      !!document.querySelector('.board .point.source') ||
      [...document.querySelectorAll('button')].some((b) => /Бросить кости/.test(b.textContent || '')),
    { timeout: 40_000 },
  );
  const roll = page.getByRole('button', { name: /Бросить кости/ });
  if (await roll.isVisible().catch(() => false)) await roll.click();
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });

  expect(errors, `console errors: ${errors.join(' | ')}`).toHaveLength(0);
});
