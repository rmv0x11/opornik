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
// move with sources highlighted). Builds the turn sub-move by sub-move, then
// commits it via the explicit "Подтвердить ход" button (no auto-commit).
async function makeHumanMove(page: Page) {
  for (let i = 0; i < 32; i++) {
    const confirm = page.getByRole('button', { name: /Подтвердить ход/ });
    if (await confirm.isVisible().catch(() => false)) {
      await confirm.click();
      return;
    }
    const dst = page.locator('.board .point.dest');
    if ((await dst.count()) > 0) {
      await dst.first().click();
      await page.waitForTimeout(340); // let the glide animation settle (ANIM_MS=300)
      continue;
    }
    const bo = page.getByRole('button', { name: /Выкинуть/ });
    if (await bo.isVisible().catch(() => false)) {
      await bo.click();
      await page.waitForTimeout(480); // bear-off arc into the tray (BEAR_MS=440)
      continue;
    }
    const src = page.locator('.board .point.source');
    if ((await src.count()) > 0) {
      await src.first().click();
      continue;
    }
    // nothing actionable this instant (mid-animation / re-render) — wait and retry
    // rather than bailing, so a transiently-hidden "Подтвердить" never aborts the
    // move with it still pending
    await page.waitForTimeout(150);
  }
}

// Roll for the opening (who moves first). ФСНР: the higher roller goes first and
// plays the OPENING dice immediately (no re-roll). So afterwards the human either
// MOVES first (sources highlighted, no roll button) or, if the AI won, gets a
// normal roll ("Бросить кости") after the AI's opening move.
async function doOpening(page: Page) {
  await page.getByRole('button', { name: /Разыграть первый ход/ }).click({ timeout: 40_000 });
}

// Wait until the game is playable after the opening: human moves first OR a normal
// roll is pending.
async function waitOpeningReady(page: Page) {
  await page.waitForFunction(
    () =>
      !!document.querySelector('.board .point.source') ||
      [...document.querySelectorAll('button')].some((b) =>
        /Бросить кости/.test(b.textContent || ''),
      ),
    { timeout: 40_000 },
  );
}

// Drive to the human's ROLL phase (where "Бросить кости"/"Удвоить" show). If the
// human won the opening they move first with the opening dice — play that move so
// the turn cycles back to a normal human roll.
async function reachHumanRoll(page: Page) {
  for (let i = 0; i < 8; i++) {
    if (
      await page
        .getByRole('button', { name: /Бросить кости/ })
        .isVisible()
        .catch(() => false)
    )
      return;
    if ((await page.locator('.board .point.source').count()) > 0) {
      await makeHumanMove(page);
      await waitIdle(page);
      continue;
    }
    await page.waitForTimeout(300);
  }
}

async function startAndRoll(page: Page, variantBtn?: RegExp) {
  await page.goto('/');
  if (variantBtn) await page.getByRole('button', { name: variantBtn }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await waitOpeningReady(page);
  const roll = page.getByRole('button', { name: /Бросить кости/ });
  if (await roll.isVisible().catch(() => false)) await roll.click();
  await page.locator('.board .point.source').first().waitFor({ timeout: 40_000 });
}

test('engine loads and a full game plays vs the AI via click-to-move', async ({ page }) => {
  const errors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });

  await page.goto('/');
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await waitOpeningReady(page);
  await expect(page.locator('.board .checker').first()).toBeVisible();

  let gameOver = false;
  for (let round = 0; round < 8 && !gameOver; round++) {
    if (await page.getByRole('button', { name: /Новая партия/ }).isVisible()) {
      gameOver = true;
      break;
    }
    const roll = page.getByRole('button', { name: /Бросить кости/ });
    if (await roll.isVisible().catch(() => false)) await roll.click();
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

test('inline analysis auto-shows ranked moves with the best marked (no button)', async ({ page }) => {
  await startAndRoll(page);
  // the best-moves overview appears automatically on the human's move (no «Оценка» click)
  await expect(page.locator('.analysis .ranked li').first()).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.ranked li.best')).toContainText('лучший');
});

test('nardegammon analysis auto-shows a cube decision', async ({ page }) => {
  await startAndRoll(page, /Нардегаммон/);
  await expect(page.locator('.analysis .cube')).toContainText('Куб', { timeout: 40_000 });
});

test('nardegammon: human doubles, the AI takes (cube 1 → 2)', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Нардегаммон/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await reachHumanRoll(page); // "Удвоить" shows on the human's roll
  await page.getByRole('button', { name: /Удвоить/ }).click({ timeout: 40_000 });
  await expect(page.locator('.cube-state')).toContainText('2', { timeout: 40_000 });
  await expect(page.locator('.cube-state')).toContainText('у движка');
});

test('match: a player 1-away from the match cannot double (post-Crawford)', async ({ page }) => {
  // 7-match, score 6:2 → the human is one point from winning → the cube is useless to
  // them (any win wins the match), so «Удвоить» must NOT be offered, even though this
  // is a post-Crawford game (crawfordPlayed=true, crawford=false). A hint explains it.
  const saved = JSON.stringify({
    v: 1,
    id: 't',
    name: 'matchpoint',
    savedAt: '2026-06-04T00:00:00.000Z',
    variant: 'nardegammon',
    aiPly: 2,
    humanColor: 'white',
    matchLength: 7,
    matchScore: [6, 2],
    crawfordPlayed: true,
    setup: {
      variant: 'nardegammon',
      turn: 'white',
      white: [{ pos: 24, player: 'white', count: 15 }],
      black: [{ pos: 24, player: 'black', count: 15 }],
      off: [0, 0],
      dice: null,
      cube: { value: 1, owner: null, turned: false },
      crawford: false,
      turn_number: 0,
    },
  });
  await page.goto('/');
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(saved);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  await expect(page.getByRole('button', { name: /Бросить кости/ })).toBeVisible({ timeout: 40_000 });
  // at match point the cube is gone, and a post-Crawford note says why
  await expect(page.getByRole('button', { name: /Удвоить/ })).toHaveCount(0);
  await expect(page.locator('.crawford.post')).toContainText('Пост-кроуфорд');
});

test('match mode: scoreboard shows the target and the game is playable', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Матч до 7/ }).click();
  await page.getByRole('button', { name: /Начать матч/ }).click();
  await expect(page.locator('.score')).toContainText('матч 0:0');
  await expect(page.locator('.score')).toContainText('7');
  await doOpening(page);
  await waitOpeningReady(page);
  const roll = page.getByRole('button', { name: /Бросить кости/ });
  if (await roll.isVisible().catch(() => false)) await roll.click();
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });
});

test('game log records played turns with evaluations', async ({ page }) => {
  await startAndRoll(page);
  await makeHumanMove(page);
  // after the human move + the AI reply, the log lists both with eval labels
  await expect(page.locator('.gamelog .gl-summary')).toContainText('Лист партии', { timeout: 40_000 });
  await expect(page.locator('.gamelog .logrow').first()).toBeVisible();
  // at least one row carries an evaluation (win% or equity loss / лучший)
  await expect(page.locator('.gamelog .logrow .ev').first()).not.toBeEmpty();
  // a single-checker chain renders collapsed (no "X/Y Y/Z" with repeated Y)
  const moves = await page.locator('.gamelog .logrow .mv').allTextContents();
  for (const m of moves) {
    const segs = m.trim().split(/\s+/).filter((s) => s.includes('/'));
    for (let i = 1; i < segs.length; i++) {
      const prevTo = segs[i - 1].split('/')[1];
      const curFrom = segs[i].split('/')[0];
      expect(prevTo, `chain not collapsed in "${m}"`).not.toBe(curFrom);
    }
  }
});

test('AI roll is shown as pip-dice', async ({ page }) => {
  await startAndRoll(page);
  await makeHumanMove(page); // → the AI replies → its roll shows as pip-dice
  await expect(page.locator('.ailast .minidie')).toHaveCount(2, { timeout: 40_000 });
  await expect(page.locator('.ailast .minidie .mp.on').first()).toBeVisible();
});

test('resign: confirming a concession ends the game (mars = 2 pts at the start)', async ({ page }) => {
  await startAndRoll(page);
  await page.getByRole('button', { name: /Сдаться/ }).click(); // single button → confirm form
  await page.getByRole('button', { name: /Да, сдаться/ }).click();
  await expect(
    page.getByRole('button', { name: /Новая партия|Следующая партия/ }),
  ).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.status')).toContainText('Сдача с марсом');
});

test('resign: at the start the concession is a mars (оин not available yet)', async ({ page }) => {
  await startAndRoll(page);
  await page.getByRole('button', { name: /Сдаться/ }).click();
  // the confirmation states the single correct outcome — марс — not оин
  await expect(page.locator('.resign-confirm')).toContainText('марс');
  await expect(page.locator('.resign-confirm')).not.toContainText('оин');
  // and it can be cancelled (no game over)
  await page.getByRole('button', { name: /Отмена/ }).click();
  await expect(page.locator('.resign-confirm')).toHaveCount(0);
});

test('post-game review: a summary panel with accuracy stats appears when the game ends', async ({
  page,
}) => {
  await startAndRoll(page);
  await makeHumanMove(page); // one scored human move in the log
  // wait for the AI reply to finish, then concede on our next turn
  await page.getByRole('button', { name: /Сдаться/ }).click({ timeout: 40_000 });
  await page.getByRole('button', { name: /Да, сдаться/ }).click();
  const panel = page.locator('.postgame');
  await expect(panel).toBeVisible({ timeout: 40_000 });
  await expect(panel).toContainText('Разбор партии');
  await expect(panel).toContainText('поражение'); // structured result of the concession
  await expect(panel).toContainText('экв./ход'); // accuracy line over scored moves
  await expect(panel.locator('.pg-stat.good')).toContainText('лучших');
  // if the move lost equity, the worst-moves list jumps to that log entry
  const worst = panel.locator('.pg-row');
  if ((await worst.count()) > 0) {
    await worst.first().click();
    await expect(page.locator('.gamelog .logrow.active').first()).toBeVisible();
    await expect(page.locator('.gamelog .review').first()).toBeVisible();
  }
  // starting the next game clears the review panel
  await page.getByRole('button', { name: /Новая партия|Следующая партия/ }).click();
  await expect(panel).toHaveCount(0);
});

test('post-game review: replaying a move out of a finished game takes the awarded points back', async ({
  page,
}) => {
  await startAndRoll(page);
  await makeHumanMove(page);
  await page.getByRole('button', { name: /Сдаться/ }).click({ timeout: 40_000 });
  await page.getByRole('button', { name: /Да, сдаться/ }).click();
  await expect(page.locator('.postgame')).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.score')).toContainText('счёт 0:2'); // mars concession = 2 pts
  // replay our move straight out of the finished game (the review panel invites this)
  await page.locator('.gamelog .logrow.me').first().click();
  await page.getByRole('button', { name: /Переиграть этот ход/ }).click();
  await expect(page.locator('.status')).toContainText('Переиграйте');
  // the concession's 2 points were rolled back — the score line is blank again, the
  // panel is gone; finishing the replayed game must not double-count
  await expect(page.locator('.score')).toHaveText(/^\s*$/);
  await expect(page.locator('.postgame')).toHaveCount(0);
});

test('resign: оин is allowed once a checker is borne off (mars impossible)', async ({ page }) => {
  // White has already borne off → cannot be marsed → оин allowed.
  const saved = JSON.stringify({
    v: 1,
    id: 't',
    name: 'oin',
    savedAt: '2026-06-04T00:00:00.000Z',
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
    matchScore: [0, 0],
    crawfordPlayed: false,
    setup: {
      variant: 'traditional',
      turn: 'white',
      white: [{ pos: 5, player: 'white', count: 5 }],
      black: [{ pos: 24, player: 'black', count: 15 }],
      off: [10, 0],
      dice: null,
      cube: { value: 1, owner: null, turned: false },
      crawford: false,
      turn_number: 30,
    },
  });
  await page.goto('/');
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(saved);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  await page.getByRole('button', { name: /Сдаться/ }).click();
  // a checker is off → mars impossible → the single concession is оин (1 pt)
  await expect(page.locator('.resign-confirm')).toContainText('оин');
  await page.getByRole('button', { name: /Да, сдаться/ }).click();
  await expect(
    page.getByRole('button', { name: /Новая партия|Следующая партия/ }),
  ).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.status')).toContainText('Сдача');
});

test('resign: a bear-off already built (not yet confirmed) counts as оин, not марс', async ({
  page,
}) => {
  // White all home, NOTHING borne off yet (committed off=0 → mars possible), dice
  // 2-1. Bear one checker off (pending, unconfirmed) → the board shows it off → the
  // concession must read оин, not марс (regression guard for the displayed-vs-
  // committed off bug).
  const saved = JSON.stringify({
    v: 1,
    id: 't',
    name: 'pending-bearoff',
    savedAt: '2026-06-04T00:00:00.000Z',
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
    matchScore: [0, 0],
    crawfordPlayed: false,
    setup: {
      variant: 'traditional',
      turn: 'white',
      white: [
        { pos: 2, player: 'white', count: 1 },
        { pos: 1, player: 'white', count: 14 },
      ],
      black: [{ pos: 24, player: 'black', count: 15 }],
      off: [0, 0],
      dice: [2, 1],
      cube: { value: 1, owner: null, turned: false },
      crawford: false,
      turn_number: 50,
    },
  });
  await page.goto('/');
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(saved);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  // select the point-2 checker (physical cell 22 for white) and bear it off
  await page.locator('.board .point[data-cell="22"]').click();
  await page.getByRole('button', { name: /Выкинуть/ }).click();
  await page.waitForTimeout(480); // bear-off arc into the tray (BEAR_MS=440)
  // now resign → a checker is off the board → оин, NOT марс
  await page.getByRole('button', { name: /Сдаться/ }).click();
  await expect(page.locator('.resign-confirm')).toContainText('оин');
  await expect(page.locator('.resign-confirm')).not.toContainText('марс');
});

test('resign with the cube at 2 concedes double points (mars = 4)', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Нардегаммон/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await reachHumanRoll(page);
  await page.getByRole('button', { name: /Удвоить/ }).click({ timeout: 40_000 });
  await expect(page.locator('.cube-state')).toContainText('2', { timeout: 40_000 });
  await page.getByRole('button', { name: /Сдаться/ }).click();
  await expect(page.locator('.resign-confirm')).toContainText('+4'); // mars × cube(2)
  await page.getByRole('button', { name: /Да, сдаться/ }).click();
  await expect(page.locator('.status')).toContainText('+4', { timeout: 40_000 });
});

test('match: resigning awards the points to the engine on the scoreboard', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Матч до 7/ }).click();
  await page.getByRole('button', { name: /Начать матч/ }).click();
  await doOpening(page);
  await waitOpeningReady(page); // resign is available whether the human moves or rolls
  await page.getByRole('button', { name: /Сдаться/ }).click();
  await page.getByRole('button', { name: /Да, сдаться/ }).click();
  await expect(page.locator('.score')).toContainText('0:2', { timeout: 40_000 });
});

test('rolling by clicking the roll prompt on the board', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await reachHumanRoll(page); // the side-rail roll prompt appears on the human's roll
  await expect(page.locator('.board .rollzone')).toBeVisible({ timeout: 40_000 });
  await page.locator('.board .rollzone').click();
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });
});

test('auto-roll: once enabled the dice throw without clicking the roll prompt', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await doOpening(page);
  await waitOpeningReady(page);
  // enable auto-roll (toggle lives in the pips row, always visible)
  await page.getByRole('button', { name: /авто-бросок/ }).click();
  // it auto-throws → the human gets movable sources WITHOUT us clicking «Бросить кости»
  await page.locator('.board .point.source').first().waitFor({ timeout: 40_000 });
  await makeHumanMove(page);
  // after the engine replies it becomes our roll again and auto-roll fires once more —
  // sources reappear with no manual roll click
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });
});

test('save a game and resume it from the menu', async ({ page }) => {
  await startAndRoll(page);
  await page.getByRole('button', { name: /Сохранить/ }).click();
  await expect(page.locator('.savemsg')).toContainText('сохранена');
  await page.getByRole('button', { name: /меню/ }).click();
  await expect(page.locator('.save-go').first()).toBeVisible({ timeout: 10_000 });
  await page.locator('.save-go').first().click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.status')).toContainText('загружена');
});

test('export a position and continue from it via import', async ({ page }) => {
  await startAndRoll(page);
  await page.getByRole('button', { name: /Экспорт/ }).click();
  const str = await page.locator('.exportbox').inputValue();
  expect(str.length).toBeGreaterThan(10);
  await page.getByRole('button', { name: /меню/ }).click();
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(str);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  await expect(page.locator('.status')).toContainText('загружена');
});

test('bear-off respects the over-roll order rule (no 1/off while point 3 occupied)', async ({
  page,
}) => {
  // White: 2 on point 1, 1 on point 3, 12 borne off; dice 2-2. The only legal first
  // move is 3→1; point 1 must NOT be playable/bear-offable until point 3 clears.
  const saved = JSON.stringify({
    v: 1,
    id: 't',
    name: 'bearoff-bug',
    savedAt: '2026-06-04T00:00:00.000Z',
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
    matchScore: [0, 0],
    crawfordPlayed: false,
    setup: {
      variant: 'traditional',
      turn: 'white',
      white: [
        { pos: 1, player: 'white', count: 2 },
        { pos: 3, player: 'white', count: 1 },
      ],
      black: [{ pos: 24, player: 'black', count: 15 }],
      off: [12, 0],
      dice: [2, 2],
      cube: { value: 1, owner: null, turned: false },
      crawford: false,
      turn_number: 40,
    },
  });
  await page.goto('/');
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(saved);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  // Only point 3 (physical cell 21) is a movable source; point 1 (cell 23) is not.
  await expect(page.locator('.board .point.source')).toHaveCount(1);
  await expect(page.locator('.board .point[data-cell="21"]')).toHaveClass(/source/);
  await expect(page.locator('.board .point[data-cell="23"]')).not.toHaveClass(/source/);
});

test('move-building: a checker can play EITHER die first to reach the same square', async ({
  page,
}) => {
  // White checker on pos 10 (+ 14 on the head for a valid 15), dice 3-1. From pos
  // 10 the player may go 10/9 (the 1) or 10/7 (the 3) first, both chaining to pos 6.
  // The old multiset-matching UI offered only ONE order (it forced the smaller die
  // first); the engine-driven sequences offer both — so selecting pos 10 must
  // highlight pos 9, pos 7 AND pos 6 as destinations.
  const saved = JSON.stringify({
    v: 1,
    id: 't',
    name: 'transpose',
    savedAt: '2026-06-04T00:00:00.000Z',
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
    matchScore: [0, 0],
    crawfordPlayed: false,
    setup: {
      variant: 'traditional',
      turn: 'white',
      white: [
        { pos: 24, player: 'white', count: 14 },
        { pos: 10, player: 'white', count: 1 },
      ],
      black: [{ pos: 24, player: 'black', count: 15 }],
      off: [0, 0],
      dice: [3, 1],
      cube: { value: 1, owner: null, turned: false },
      crawford: false,
      turn_number: 6,
    },
  });
  await page.goto('/');
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(saved);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  // locate a point by the number printed on it (orientation-independent)
  const ptByNum = (n: number) =>
    page.locator('.board .point', { has: page.locator('.ptnum', { hasText: new RegExp(`^${n}$`) }) });
  await ptByNum(10).click(); // select the lone advanced checker
  await expect(ptByNum(9)).toHaveClass(/dest/, { timeout: 40_000 }); // play the 1 first
  await expect(ptByNum(7)).toHaveClass(/dest/); // play the 3 first (was forbidden before)
  await expect(ptByNum(6)).toHaveClass(/dest/); // both dice → final square
});

test('game log review: click a move to see alternatives, then replay it', async ({ page }) => {
  await startAndRoll(page);
  await makeHumanMove(page); // commit a human move → AI replies → log has both
  await expect(page.locator('.gamelog')).toBeVisible({ timeout: 40_000 });
  // open the review for the human's move
  await page.locator('.logrow.me').first().click();
  await expect(page.locator('.review .alts .alt').first()).toBeVisible({ timeout: 40_000 });
  // the played move is tagged
  await expect(page.locator('.review .alt.played').first()).toBeVisible();
  // replay this move → re-enter the move phase with the same dice
  await page.getByRole('button', { name: /Переиграть этот ход/ }).click();
  await expect(page.locator('.status')).toContainText('Переиграйте', { timeout: 40_000 });
  await expect(page.locator('.board .point.source').first()).toBeVisible({ timeout: 40_000 });
});

test('game log review: clicking an alternative plays it and the game continues', async ({
  page,
}) => {
  await startAndRoll(page);
  await makeHumanMove(page);
  await page.locator('.logrow.me').first().click();
  await expect(page.locator('.review .alts .alt').first()).toBeVisible({ timeout: 40_000 });
  // click the top-ranked alternative → it is replayed and the engine responds
  await page.locator('.review .alts .alt').first().click();
  await expect(
    page.getByRole('button', { name: /Бросить кости|Новая партия|Подтвердить ход/ }),
  ).toBeVisible({ timeout: 40_000 });
});

test('can move a head checker onto a point holding your own checker (stacking by click)', async ({
  page,
}) => {
  // White: 14 on head (24) + 1 on point 18; dice 6-4. Head→18 (die 6) lands on the
  // own checker. Clicking that occupied point must MOVE there, not re-select it.
  const saved = JSON.stringify({
    v: 1,
    id: 't',
    name: 'stack',
    savedAt: '2026-06-04T00:00:00.000Z',
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
    matchScore: [0, 0],
    crawfordPlayed: false,
    setup: {
      variant: 'traditional',
      turn: 'white',
      white: [
        { pos: 24, player: 'white', count: 14 },
        { pos: 18, player: 'white', count: 1 },
      ],
      black: [{ pos: 24, player: 'black', count: 15 }],
      off: [0, 0],
      dice: [6, 4],
      cube: { value: 1, owner: null, turned: false },
      crawford: false,
      turn_number: 5,
    },
  });
  await page.goto('/');
  await page.locator('.import > summary').click();
  await page.locator('.import-box').fill(saved);
  await page.getByRole('button', { name: /Продолжить с этой позиции/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 });
  await page.locator('.board .point[data-cell="0"]').click(); // select head
  await expect(page.locator('.board .point[data-cell="6"]')).toHaveClass(/dest/, {
    timeout: 40_000,
  });
  await page.locator('.board .point[data-cell="6"]').click(); // own-occupied target
  await page.waitForTimeout(300);
  await expect(page.getByRole('button', { name: /Отменить/ })).toBeVisible({ timeout: 40_000 });
});

test('theme picker applies the chosen board/checker theme to the board', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Классик', exact: true }).click();
  await page.getByRole('button', { name: /Янтарь и лазурь/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await expect(page.locator('.board')).toBeVisible({ timeout: 40_000 }); // board shows from the opening roll
  const style = (await page.locator('.board').getAttribute('style')) || '';
  expect(style).toContain('--felt');
  expect(style).toContain('--checker-grad-white');
});

test('rules screen opens from the menu and returns', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Правила/ }).click();
  await expect(page.getByRole('heading', { name: /Правила/ })).toBeVisible();
  await expect(page.locator('.rules')).toContainText('правило головы');
  await page.getByRole('button', { name: /Понятно, играть/ }).click();
  await expect(page.getByRole('button', { name: /Начать партию/ })).toBeVisible({ timeout: 40_000 });
});

test('opening roll: higher die goes first; the game becomes playable', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: /Чёрные/ }).click();
  await page.getByRole('button', { name: /Начать партию/ }).click();
  await expect(page.getByRole('button', { name: /Разыграть первый ход/ })).toBeVisible({
    timeout: 40_000,
  });
  await doOpening(page);
  // the game becomes playable: the human either moves first with the opening dice
  // or (if the AI won) gets a normal roll
  await waitOpeningReady(page);
  await expect(page.locator('.board .checker').first()).toBeVisible();
});

// ---------------------------------------------------------------------------
// Mobile layout usability: the game log must scroll INTERNALLY and the page must
// NOT scroll (regression guard for v0.9.6→0.9.7, where a <details> flex container
// clipped the log and broke its scrolling).
// ---------------------------------------------------------------------------
test.describe('mobile: game-log + move-review usability', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  // Play `rounds` human turns so the game log accumulates several rows.
  async function playRounds(page: Page, rounds: number) {
    await startAndRoll(page);
    for (let r = 0; r < rounds; r++) {
      await makeHumanMove(page);
      await page.waitForFunction(
        () =>
          [...document.querySelectorAll('button')].some((b) =>
            /Бросить кости|Новая партия/.test(b.textContent || ''),
          ),
        { timeout: 40_000 },
      );
      if (await page.getByRole('button', { name: /Новая партия/ }).isVisible().catch(() => false)) break;
      const roll = page.getByRole('button', { name: /Бросить кости/ });
      if (!(await roll.isVisible().catch(() => false))) break;
      await roll.click();
      await page.locator('.board .point.source').first().waitFor({ timeout: 40_000 }).catch(() => {});
    }
  }

  test('the game log is a roomy, scrollable box', async ({ page }) => {
    await playRounds(page, 6);
    await expect(page.locator('.gamelog .log')).toBeVisible({ timeout: 40_000 });
    expect(await page.locator('.gamelog .logrow').count()).toBeGreaterThan(4);

    const m = await page.evaluate(() => {
      const l = document.querySelector('.gamelog .log') as HTMLElement;
      return { sh: l.scrollHeight, ch: l.clientHeight };
    });
    // the log gets a ROOMY box (not crammed into a tiny strip)…
    expect(m.ch, 'log box should be roomy').toBeGreaterThan(160);
    // …and scrolls within itself when there are more rows than fit.
    expect(m.sh, 'log content should exceed its box (so it scrolls)').toBeGreaterThan(m.ch + 5);
    const moved = await page.evaluate(() => {
      const l = document.querySelector('.gamelog .log') as HTMLElement;
      l.scrollTop = 9999;
      return l.scrollTop > 5;
    });
    expect(moved, 'log should scroll internally').toBe(true);
  });

  test('the move-review opens with room for the alternatives, and the log toggles', async ({
    page,
  }) => {
    await playRounds(page, 3);
    await expect(page.locator('.gamelog .log')).toBeVisible({ timeout: 40_000 });
    // open the review for a human move → its alternatives must be readable (the
    // cramped-box regression hid them in a ~1-row strip)
    await page.locator('.logrow.me').first().click();
    await expect(page.locator('.review .alts .alt').first()).toBeVisible({ timeout: 40_000 });
    const ch = await page.evaluate(
      () => (document.querySelector('.gamelog .log') as HTMLElement).clientHeight,
    );
    expect(ch, 'the review should open in a roomy log box').toBeGreaterThan(160);
    // the log collapses and expands
    await page.locator('.gamelog .gl-summary').click();
    await expect(page.locator('.gamelog .log')).toHaveCount(0);
    await page.locator('.gamelog .gl-summary').click();
    await expect(page.locator('.gamelog .log')).toBeVisible();
  });
});
