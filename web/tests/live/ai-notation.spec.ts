import { test, expect, type Page } from '@playwright/test';

const LIVE = 'https://rmv0x11.github.io/opornik/';

// Parse "24/20 20/14" (or with выкид) into sub-moves.
function parse(moves: string): { from: number; to: string }[] {
  return moves
    .trim()
    .split(/\s+/)
    .filter((s) => s.includes('/'))
    .map((s) => {
      const [from, to] = s.split('/');
      return { from: Number(from), to };
    });
}

test('LIVE: AI move notation is internally consistent (no phantom sources)', async ({ page }) => {
  await page.goto(LIVE);
  await page.getByRole('button', { name: /Чёрные/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  // opening roll determines who is first; either way AI moves are observed below
  await page.getByRole('button', { name: /Разыграть первый ход/ }).click({ timeout: 40_000 });

  const seen: string[] = [];
  for (let round = 0; round < 8; round++) {
    const ai = page.locator('.ailast');
    if (await ai.isVisible().catch(() => false)) {
      const moves = ((await page.locator('.ailast .al-moves').textContent().catch(() => '')) || '').trim();
      if (moves) {
        if (!seen.includes(moves)) {
          seen.push(moves);
          const subs = parse(moves);
          // Notation is collapsed (single-checker chains → one segment), so every
          // segment's source must be a real point 1..24 (no phantom sources).
          for (const s of subs) {
            expect(s.from, `bad source in "${moves}"`).toBeGreaterThanOrEqual(1);
            expect(s.from, `bad source in "${moves}"`).toBeLessThanOrEqual(24);
          }
        }
      }
    }
    // advance the game: roll if a roll is pending, then commit a move if one is
    // offered. The commit loop runs regardless of the roll button so the
    // human-first opening move (played with the opening dice, no roll) is handled.
    const roll = page.getByRole('button', { name: /Бросить кости/ });
    if (await roll.isVisible().catch(() => false)) {
      await roll.click();
      await page.waitForTimeout(400);
    }
    let committed = false;
    for (let k = 0; k < 12; k++) {
      const confirm = page.getByRole('button', { name: /Подтвердить ход/ });
      if (await confirm.isVisible().catch(() => false)) {
        await confirm.click();
        committed = true;
        break;
      }
      const dst = page.locator('.board .point.dest');
      if ((await dst.count()) > 0) {
        await dst.first().click();
        await page.waitForTimeout(280);
        continue;
      }
      const bo = page.getByRole('button', { name: /Выкинуть/ });
      if (await bo.isVisible().catch(() => false)) {
        await bo.click();
        continue;
      }
      const src = page.locator('.board .point.source');
      if ((await src.count()) > 0) {
        await src.first().click();
        continue;
      }
      break;
    }
    await page.waitForTimeout(committed ? 600 : 500);
  }
  console.log('AI moves observed live:', JSON.stringify(seen, null, 2));
  expect(seen.length, 'should observe at least one AI move').toBeGreaterThan(0);
});
