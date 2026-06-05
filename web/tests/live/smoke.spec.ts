import { test, expect, type Page } from '@playwright/test';

const LIVE = 'https://rmv0x11.github.io/opornik/';

async function makeHumanMove(page: Page) {
  for (let i = 0; i < 40; i++) {
    const confirm = page.getByRole('button', { name: /Подтвердить ход/ });
    if (await confirm.isVisible().catch(() => false)) {
      await confirm.click();
      return true;
    }
    const bo = page.getByRole('button', { name: /Выкинуть/ });
    if (await bo.isVisible().catch(() => false)) {
      await bo.click();
      await page.waitForTimeout(480);
      continue;
    }
    const dst = page.locator('.board .point.dest');
    if ((await dst.count()) > 0) {
      await dst.first().click();
      await page.waitForTimeout(360); // let the glide settle (ANIM_MS=300)
      continue;
    }
    const src = page.locator('.board .point.source');
    if ((await src.count()) > 0) {
      await src.first().click();
      await page.waitForTimeout(120);
      continue;
    }
    // nothing actionable yet (mid-animation / re-render) — wait and retry rather
    // than bailing, so a transiently-hidden "Подтвердить" doesn't abort the move
    await page.waitForTimeout(200);
  }
  return false;
}

test('LIVE: new flow — пипсы label, confirm-to-commit, AI-dice line', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });

  await page.goto(LIVE);
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await page.getByRole('button', { name: /Разыграть первый ход/ }).click({ timeout: 40_000 });
  // ФСНР: after the opening the human either moves first (sources) or rolls first.
  await page.waitForFunction(
    () =>
      !!document.querySelector('.board .point.source') ||
      [...document.querySelectorAll('button')].some((b) => /Бросить кости/.test(b.textContent || '')),
    { timeout: 40_000 },
  );

  // A: label says "пипсы" (not "пипы")
  await expect(page.locator('.meta')).toContainText('пипсы');

  // B/C: get to a human move (roll if a roll is pending), build a move, confirm it
  const roll1 = page.getByRole('button', { name: /Бросить кости/ });
  if (await roll1.isVisible().catch(() => false)) await roll1.click();
  await page.locator('.board .point.source').first().waitFor({ timeout: 40_000 });
  await page.locator('.board .point.source').first().click();
  await expect(page.locator('.board .point.dest').first()).toBeVisible();
  // building a hop yields an Undo button (turn is NOT auto-applied)
  await page.locator('.board .point.dest').first().click();
  await expect(
    page
      .getByRole('button', { name: /Отменить/ })
      .or(page.getByRole('button', { name: /Подтвердить ход/ })),
  ).toBeVisible({ timeout: 40_000 });
  await makeHumanMove(page);

  // D: after the engine replies it's our roll again, and the AI-dice line shows
  await expect(page.getByRole('button', { name: /Бросить кости|Новая партия/ })).toBeVisible({
    timeout: 40_000,
  });
  await expect(page.locator('.ailast')).toContainText('Соперник бросил', { timeout: 40_000 });

  expect(errors, `console errors: ${errors.join(' | ')}`).toHaveLength(0);
});
