import { test, expect, type Page } from '@playwright/test';

// Verification spec for the cube-status column in the game sheet (лист партии).
// Drives a Нардегаммон game, doubles so the cube turns to ×2, plays a turn so the
// sheet accumulates checker rows under the turned cube, then asserts the badges
// render and screenshots the sheet.

async function doOpening(page: Page) {
  await page.getByRole('button', { name: /Разыграть первый ход/ }).click({ timeout: 40_000 });
}

async function makeHumanMove(page: Page) {
  for (let i = 0; i < 64; i++) {
    const confirm = page.getByRole('button', { name: /Подтвердить ход/ });
    if (await confirm.isVisible().catch(() => false)) {
      await confirm.click();
      return;
    }
    const dst = page.locator('.board .point.dest');
    if ((await dst.count()) > 0) {
      await dst.first().click();
      await page.waitForTimeout(340);
      continue;
    }
    const src = page.locator('.board .point.source');
    if ((await src.count()) > 0) {
      await src.first().click();
      continue;
    }
    await page.waitForTimeout(150);
  }
}

async function reachHumanRoll(page: Page) {
  for (let i = 0; i < 8; i++) {
    if (await page.getByRole('button', { name: /Бросить кости/ }).isVisible().catch(() => false)) return;
    if ((await page.locator('.board .point.source').count()) > 0) {
      await makeHumanMove(page);
      await page.waitForTimeout(400);
      continue;
    }
    await page.waitForTimeout(300);
  }
}

test('game sheet shows a cube-status badge on each turn (turned ×2 after a double)', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Нардегаммон/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await reachHumanRoll(page);

  // every checker-move row in a cube variant carries a cube badge (≥1 by now)
  await expect(page.locator('.gamelog .logrow.cubevar').first()).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.gamelog .logrow .cb').first()).toContainText('×');

  // double → cube turns to ×2 and gains an owner
  await page.getByRole('button', { name: /Удвоить/ }).click({ timeout: 40_000 });
  await expect(page.locator('.cube-state')).toContainText('2', { timeout: 40_000 });

  // play one more turn under the turned cube so a row records owner = у движка/вас
  await reachHumanRoll(page);
  if (await page.getByRole('button', { name: /Бросить кости/ }).isVisible().catch(() => false)) {
    await page.getByRole('button', { name: /Бросить кости/ }).click();
    await page.locator('.board .point.source').first().waitFor({ timeout: 40_000 }).catch(() => {});
    await makeHumanMove(page);
    await page.waitForTimeout(600);
  }

  // a turned-cube badge (c-me or c-opp) now appears in the sheet, reading ×2
  const turned = page.locator('.gamelog .logrow .cb.c-me, .gamelog .logrow .cb.c-opp');
  await expect(turned.first()).toBeVisible({ timeout: 40_000 });
  await expect(turned.first()).toContainText('×2');

  // a cube-ACTION row exists and its cube cell is EMPTY (transition is in the notation)
  const cubeRow = page.locator('.gamelog .logrow.cube').first();
  await expect(cubeRow).toBeVisible();
  await expect(cubeRow.locator('.cb')).toHaveText('');

  await page.locator('.gamelog').screenshot({ path: 'test-results/cube-sheet.png' });
});
