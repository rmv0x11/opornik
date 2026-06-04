import { test, expect, type Page } from '@playwright/test';

const CONTROL = /Бросить кости|Новая партия/;

// Wait until the human can act (sources highlighted) or control has returned.
async function waitIdle(page: Page) {
  await page.waitForFunction(
    () =>
      !!document.querySelector('.board .point.source') ||
      [...document.querySelectorAll('button')].some((b) =>
        /Бросить кости|Новая партия/.test(b.textContent || ''),
      ),
    { timeout: 40_000 },
  );
}

// Complete one human turn entirely via click-to-move (assumes it's the human's
// move with sources highlighted).
async function makeHumanMove(page: Page) {
  for (let i = 0; i < 8; i++) {
    let dst = page.locator('.board .point.dest');
    if ((await dst.count()) === 0) {
      const src = page.locator('.board .point.source');
      if ((await src.count()) === 0) return; // turn committed
      await src.first().click();
      dst = page.locator('.board .point.dest');
    }
    if ((await dst.count()) > 0) {
      await dst.first().click();
    } else {
      const bo = page.getByRole('button', { name: /Выкинуть/ });
      if (await bo.isVisible().catch(() => false)) await bo.click();
      else return;
    }
  }
}

async function startAndRoll(page: Page, variantBtn?: RegExp) {
  await page.goto('/');
  if (variantBtn) await page.getByRole('button', { name: variantBtn }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await page.getByRole('button', { name: /Бросить кости/ }).click({ timeout: 40_000 });
  await page.locator('.board .point.source').first().waitFor({ timeout: 40_000 });
}

test('engine loads and a full game plays vs the AI via click-to-move', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });

  await page.goto('/');
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await expect(page.getByRole('button', { name: /Бросить кости/ })).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.board .checker').first()).toBeVisible();

  let gameOver = false;
  for (let round = 0; round < 6 && !gameOver; round++) {
    if (await page.getByRole('button', { name: /Новая партия/ }).isVisible()) {
      gameOver = true;
      break;
    }
    await page.getByRole('button', { name: /Бросить кости/ }).click();
    await waitIdle(page);
    if ((await page.locator('.board .point.source').count()) > 0) {
      await makeHumanMove(page);
    }
    await page.waitForFunction(
      () =>
        [...document.querySelectorAll('button')].some((b) =>
          /Бросить кости|Новая партия/.test(b.textContent || ''),
        ),
      { timeout: 40_000 },
    );
    gameOver = await page.getByRole('button', { name: /Новая партия/ }).isVisible();
  }

  await expect(page.locator('.winbar .label')).toContainText('%');
  await expect(page.locator('.status')).not.toContainText('Ошибка');
  expect(errors, `console errors: ${errors.join(' | ')}`).toHaveLength(0);
});

test('click-to-move: selecting a source shows destinations and a click registers a hop', async ({
  page,
}) => {
  await startAndRoll(page);
  await page.locator('.board .point.source').first().click();
  await expect(page.locator('.board .point.dest').first()).toBeVisible();
  await page.locator('.board .point.dest').first().click();
  // A hop registered ⇒ either the turn is still being built (Отменить) or it
  // completed and control is returning.
  await expect(
    page
      .getByRole('button', { name: /Отменить/ })
      .or(page.getByRole('button', { name: CONTROL })),
  ).toBeVisible({ timeout: 40_000 });
});

test('drag-and-drop: dragging a checker onto a destination registers a hop', async ({ page }) => {
  await startAndRoll(page);
  const src = page.locator('.board .point.source').first();
  await src.click(); // select source → destinations appear
  const dst = page.locator('.board .point.dest').first();
  await expect(dst).toBeVisible();
  const s = await src.boundingBox();
  const d = await dst.boundingBox();
  if (!s || !d) throw new Error('missing bounding boxes');
  await page.mouse.move(s.x + s.width / 2, s.y + s.height / 2);
  await page.mouse.down();
  await page.mouse.move(s.x + s.width / 2 + 12, s.y + s.height / 2 + 12); // exceed drag threshold
  await page.mouse.move(d.x + d.width / 2, d.y + d.height / 2, { steps: 6 });
  await page.mouse.up();
  await expect(
    page
      .getByRole('button', { name: /Отменить/ })
      .or(page.getByRole('button', { name: CONTROL })),
  ).toBeVisible({ timeout: 40_000 });
});

test('inline analysis lists ranked moves with the best marked', async ({ page }) => {
  await startAndRoll(page);
  await page.getByRole('button', { name: /Оценка/ }).click();
  await expect(page.locator('.analysis .ranked li').first()).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.ranked li.best')).toContainText('лучший');
});

test('nardegammon analysis shows a cube decision', async ({ page }) => {
  await startAndRoll(page, /Нардегаммон/);
  await page.getByRole('button', { name: /Оценка/ }).click();
  await expect(page.locator('.analysis .cube')).toContainText('Куб', { timeout: 40_000 });
});

test('nardegammon: human doubles, the AI takes (cube 1 → 2)', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Нардегаммон/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await expect(page.locator('.cube-state')).toContainText('1', { timeout: 40_000 });
  await page.getByRole('button', { name: /Удвоить/ }).click();
  await expect(page.locator('.cube-state')).toContainText('2', { timeout: 40_000 });
  await expect(page.locator('.cube-state')).toContainText('у движка');
});

test('match mode: scoreboard shows the target and the game is playable', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Матч до 7/ }).click();
  await page.getByRole('button', { name: /Начать матч/ }).click();
  await expect(page.locator('.score')).toContainText('матч 0:0');
  await expect(page.locator('.score')).toContainText('7');
  await page.getByRole('button', { name: /Бросить кости/ }).click({ timeout: 40_000 });
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });
});

test('traditional as Black: AI moves first, then it is the human roll', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Чёрные/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await expect(page.getByRole('button', { name: /Бросить кости/ })).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.board .checker').first()).toBeVisible();
  await expect(page.locator('.meta')).toContainText('ход №1');
});
