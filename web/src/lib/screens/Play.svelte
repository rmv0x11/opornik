<script lang="ts">
  import { onMount } from 'svelte';
  import { engine } from '../../engine/client';
  import type {
    CheckerMoveDto,
    CubeDecisionDto,
    PlayerColor,
    PositionDto,
    RankedTurnDto,
    TurnDto,
    VariantId,
  } from '../../engine/types';
  import Board from '../board/Board.svelte';
  import { phys, posOfPhys } from '../board/coords';

  let {
    variant,
    aiPly,
    humanColor,
    matchLength,
    onExit,
  }: {
    variant: VariantId;
    aiPly: number;
    humanColor: PlayerColor;
    matchLength: number | null;
    onExit: () => void;
  } = $props();

  type Phase =
    | 'loading'
    | 'humanRoll'
    | 'rolling'
    | 'humanMove'
    | 'cubeResponse'
    | 'ai'
    | 'over'
    | 'error';

  let pos = $state<PositionDto | null>(null);
  let legal = $state<TurnDto[]>([]);
  let phase = $state<Phase>('loading');
  let status = $state('Загрузка…');
  let humanWin = $state(0.5);
  let ranked = $state<RankedTurnDto[] | null>(null);
  let cubeDec = $state<CubeDecisionDto | null>(null);
  let analyzing = $state(false);
  let pending = $state<CheckerMoveDto[]>([]); // sub-moves chosen so far this turn
  let selSource = $state<number | null>(null); // selected source (mover path pos)
  let lastCells = $state<number[]>([]); // physical cells touched by the last move (pulse)
  let matchScore = $state<[number, number]>([0, 0]); // [human, ai] points
  let crawfordPlayed = $state(false);
  let currentGameCrawford = $state(false);
  let pendingPre = $state(1); // cube value before a pending double, for pass resolution

  const aiColor: PlayerColor = $derived(humanColor === 'white' ? 'black' : 'white');
  const hasCube = $derived(variant === 'nardegammon' || variant === 'hachapuri');
  const canBeaver = $derived(variant === 'hachapuri');
  const canHumanDouble = $derived(
    hasCube &&
      phase === 'humanRoll' &&
      pos != null &&
      pos.cube.value < 64 &&
      !pos.crawford &&
      (pos.cube.owner === null || pos.cube.owner === humanColor),
  );
  const matchOver = $derived(
    matchLength != null && (matchScore[0] >= matchLength || matchScore[1] >= matchLength),
  );

  // The upcoming game is the Crawford game iff a player is exactly 1-away and the
  // Crawford game hasn't been played yet (cube variants only).
  function crawfordForNext(): boolean {
    return (
      matchLength != null &&
      hasCube &&
      !crawfordPlayed &&
      (matchScore[0] === matchLength - 1 || matchScore[1] === matchLength - 1)
    );
  }

  // ---- click-to-move: build a turn sub-move by sub-move ----
  // A sub-move is identified by (from→to) (to=0 = bear off). The partial selection
  // is matched against the legal turns by sub-move multiset; once the multiset
  // equals a full legal turn, that turn is committed.
  function moveKey(m: CheckerMoveDto) {
    return `${m.from}-${m.to}`;
  }
  function counts(moves: CheckerMoveDto[]): Map<string, number> {
    const c = new Map<string, number>();
    for (const m of moves) c.set(moveKey(m), (c.get(moveKey(m)) ?? 0) + 1);
    return c;
  }
  function superset(big: Map<string, number>, small: Map<string, number>) {
    for (const [k, v] of small) if ((big.get(k) ?? 0) < v) return false;
    return true;
  }
  function moverOcc(): Map<number, number> {
    const m = new Map<number, number>();
    if (pos) {
      for (const p of humanColor === 'white' ? pos.white : pos.black) m.set(p.pos, p.count);
    }
    for (const h of pending) {
      m.set(h.from, (m.get(h.from) ?? 0) - 1);
      if (!h.bear_off && h.to > 0) m.set(h.to, (m.get(h.to) ?? 0) + 1);
    }
    return m;
  }
  const nextHops = $derived.by<CheckerMoveDto[]>(() => {
    if (phase !== 'humanMove') return [];
    const pc = counts(pending);
    const occ = moverOcc();
    const hops = new Map<string, CheckerMoveDto>();
    for (const t of legal) {
      const tc = counts(t.moves);
      if (!superset(tc, pc)) continue;
      const rem = new Map(tc);
      for (const [k, v] of pc) rem.set(k, (rem.get(k) ?? 0) - v);
      for (const mv of t.moves) {
        const k = moveKey(mv);
        if ((rem.get(k) ?? 0) > 0 && (occ.get(mv.from) ?? 0) > 0) hops.set(k, mv);
      }
    }
    return [...hops.values()];
  });
  const sourceCells = $derived(new Set(nextHops.map((h) => phys(humanColor, h.from))));
  const destHops = $derived(selSource == null ? [] : nextHops.filter((h) => h.from === selSource));
  const destCells = $derived(
    destHops.filter((h) => !h.bear_off && h.to > 0).map((h) => phys(humanColor, h.to)),
  );
  const bearOffHop = $derived(destHops.find((h) => h.bear_off) ?? null);

  function clearMoveBuild() {
    pending = [];
    selSource = null;
  }

  function handlePointClick(cell: number) {
    if (phase !== 'humanMove') return;
    const p = posOfPhys(humanColor, cell);
    if (nextHops.some((h) => h.from === p)) {
      selSource = selSource === p ? null : p;
      return;
    }
    if (selSource != null) {
      const hop = nextHops.find((h) => h.from === selSource && !h.bear_off && h.to === p);
      if (hop) {
        applyHop(hop);
        return;
      }
    }
    selSource = null;
  }

  function applyHop(hop: CheckerMoveDto) {
    pending = [...pending, hop];
    const pc = counts(pending);
    const done = legal.find((t) => t.moves.length === pending.length && superset(counts(t.moves), pc));
    if (done) {
      void play(done.id, true);
      return;
    }
    // keep the same checker selected if it can move again (smooth chaining)
    selSource = hop.to > 0 && nextHops.some((h) => h.from === hop.to) ? hop.to : null;
  }

  function bearOffSelected() {
    if (bearOffHop) applyHop(bearOffHop);
  }
  function undoPending() {
    pending = pending.slice(0, -1);
    selSource = null;
  }

  function rollDie() {
    return 1 + Math.floor(Math.random() * 6);
  }
  function fmtTurn(t: TurnDto): string {
    if (t.is_pass) return 'пропуск';
    return t.moves.map((m) => (m.bear_off ? `${m.from}/выкид` : `${m.from}/${m.to}`)).join('  ');
  }
  function occArr(p: PositionDto): number[] {
    const o = new Array(24).fill(0);
    for (const pt of p.white) o[phys('white', pt.pos)] = pt.count;
    for (const pt of p.black) o[phys('black', pt.pos)] = -pt.count;
    return o;
  }
  function changedCells(a: PositionDto, b: PositionDto): number[] {
    const oa = occArr(a);
    const ob = occArr(b);
    const cells: number[] = [];
    for (let c = 0; c < 24; c++) if (oa[c] !== ob[c]) cells.push(c);
    return cells;
  }
  function clearAnalysis() {
    ranked = null;
    cubeDec = null;
  }
  function fail(e: unknown) {
    status = `Ошибка: ${e instanceof Error ? e.message : String(e)}`;
    phase = 'error';
  }
  function isOver(p: PositionDto) {
    return p.outcome.kind === 'win';
  }
  async function updateWin() {
    if (!pos) return;
    const p = await engine.winProbabilities();
    humanWin = pos.turn === humanColor ? p.win : 1 - p.win;
  }
  function cubeOwnerDesc(p: PositionDto) {
    if (p.cube.owner === null) return 'в центре';
    return p.cube.owner === humanColor ? 'у вас' : 'у движка';
  }

  function finishGame(winner: PlayerColor, points: number, reason: string) {
    if (winner === humanColor) matchScore[0] += points;
    else matchScore[1] += points;
    if (currentGameCrawford) crawfordPlayed = true;
    if (matchLength != null && (matchScore[0] >= matchLength || matchScore[1] >= matchLength)) {
      const humanWon = matchScore[0] >= matchLength;
      status = `${humanWon ? '🏆 Вы выиграли матч!' : 'Матч за движком.'} Счёт ${matchScore[0]}:${matchScore[1]}.`;
    } else {
      const tail = matchLength != null ? ` (матч ${matchScore[0]}:${matchScore[1]})` : ` (счёт ${matchScore[0]}:${matchScore[1]})`;
      status = `${reason}: ${winner === humanColor ? 'вы' : 'движок'} +${points}${tail}`;
    }
    phase = 'over';
  }

  function announceBoardWin(p: PositionDto) {
    const points = (p.outcome.points ?? 1) * p.cube.value;
    finishGame(p.outcome.winner as PlayerColor, points, p.outcome.mars ? 'марс' : 'оин');
  }

  async function continueAfter() {
    await updateWin();
    if (!pos) return;
    if (isOver(pos)) {
      announceBoardWin(pos);
      return;
    }
    if (pos.turn === humanColor) {
      phase = 'humanRoll';
      status = 'Ваш ход — бросайте кости.';
      clearAnalysis();
    } else {
      await aiTurn();
    }
  }

  onMount(async () => {
    try {
      pos = await engine.init(variant);
      currentGameCrawford = crawfordForNext();
      pos = await engine.setCrawford(currentGameCrawford);
      await updateWin();
      if (isOver(pos)) {
        announceBoardWin(pos);
        return;
      }
      if (pos.turn === humanColor) {
        phase = 'humanRoll';
        status = 'Ваш ход — бросайте кости.';
      } else {
        await aiTurn();
      }
    } catch (e) {
      fail(e);
    }
  });

  async function humanRoll() {
    if (phase !== 'humanRoll') return;
    phase = 'rolling';
    clearAnalysis();
    try {
      const d1 = rollDie(),
        d2 = rollDie();
      await engine.setDice(d1, d2);
      pos = await engine.getPosition();
      legal = await engine.legalTurns();
      clearMoveBuild();
      if (legal.length === 1 && legal[0].is_pass) {
        status = `Кости ${d1}-${d2}: ходов нет, пропуск.`;
        await play(legal[0].id, true);
        return;
      }
      status = `Кости ${d1}-${d2}: кликните шашку или выберите ход.`;
      phase = 'humanMove';
    } catch (e) {
      fail(e);
    }
  }

  async function humanDouble() {
    if (!canHumanDouble || !pos) return;
    const pre = pos.cube.value;
    phase = 'ai';
    status = 'Соперник решает по кубу…';
    try {
      const cd = await engine.cubeDecision();
      if (cd.opponent_should_take) {
        pos = await engine.offerDouble();
        await updateWin();
        status = `Соперник взял (тайк). Куб ×${pos.cube.value}. Ваш ход — бросайте.`;
        phase = 'humanRoll';
      } else {
        finishGame(humanColor, pre, 'Соперник пасанул');
      }
    } catch (e) {
      fail(e);
    }
  }

  async function play(id: number, fromRoll = false) {
    if (!fromRoll && phase !== 'humanMove') return;
    legal = [];
    clearMoveBuild();
    clearAnalysis();
    phase = 'ai';
    status = 'Ход движка…';
    try {
      const before = pos;
      pos = await engine.applyTurn(id);
      if (before) lastCells = changedCells(before, pos);
      await continueAfter();
    } catch (e) {
      fail(e);
    }
  }

  async function aiTurn() {
    phase = 'ai';
    status = 'Ход движка…';
    try {
      if (
        hasCube &&
        pos &&
        pos.cube.value < 64 &&
        !pos.crawford &&
        (pos.cube.owner === null || pos.cube.owner === aiColor)
      ) {
        const cd = await engine.cubeDecision();
        if (cd.action === 'double_take' || cd.action === 'double_pass') {
          pendingPre = pos.cube.value;
          pos = await engine.offerDouble();
          await updateWin();
          status = `Движок удваивает до ${pos.cube.value}. Ваш ответ?`;
          phase = 'cubeResponse';
          return;
        }
      }
      await aiRoll();
    } catch (e) {
      fail(e);
    }
  }

  async function aiRoll() {
    const d1 = rollDie(),
      d2 = rollDie();
    const before = pos;
    await engine.setDice(d1, d2);
    const bm = await engine.bestMove(aiPly);
    pos = await engine.applyTurn(bm.turn.id);
    if (before) lastCells = changedCells(before, pos);
    status = `Движок сыграл ${d1}-${d2}.`;
    await continueAfter();
  }

  async function cubeTake() {
    if (phase !== 'cubeResponse') return;
    phase = 'ai';
    status = 'Вы взяли (тайк). Ход движка…';
    try {
      await aiRoll();
    } catch (e) {
      fail(e);
    }
  }
  function cubeDrop() {
    if (phase !== 'cubeResponse') return;
    finishGame(aiColor, pendingPre, 'Вы пасанули');
  }
  async function cubeBeaver() {
    if (phase !== 'cubeResponse' || !canBeaver) return;
    phase = 'ai';
    try {
      pos = await engine.beaver();
      await updateWin();
      status = `Бивер! Куб ×${pos.cube.value}. Ход движка…`;
      await aiRoll();
    } catch (e) {
      fail(e);
    }
  }

  async function analyze() {
    if (phase !== 'humanMove' || analyzing) return;
    analyzing = true;
    try {
      ranked = await engine.analyze(1);
      if (hasCube) cubeDec = await engine.cubeDecision();
    } catch {
      /* ignore */
    } finally {
      analyzing = false;
    }
  }

  async function nextGame() {
    try {
      clearAnalysis();
      clearMoveBuild();
      lastCells = [];
      pos = await engine.reset();
      currentGameCrawford = crawfordForNext();
      pos = await engine.setCrawford(currentGameCrawford);
      legal = [];
      await updateWin();
      const intro = currentGameCrawford ? 'Кроуфорд (без куба). ' : '';
      if (pos.turn === humanColor) {
        phase = 'humanRoll';
        status = `${intro}Ваш ход — бросайте.`;
      } else {
        await aiTurn();
      }
    } catch (e) {
      fail(e);
    }
  }

  async function newMatch() {
    matchScore = [0, 0];
    crawfordPlayed = false;
    await nextGame();
  }

  const variantName: Record<VariantId, string> = {
    traditional: 'Традиционные',
    nardegammon: 'Нардегаммон',
    classic: 'Классика',
    hachapuri: 'Хачапури',
  };
</script>

<div class="play">
  <header>
    <button class="link" onclick={onExit} disabled={phase === 'ai' || phase === 'rolling'}>
      ← меню
    </button>
    <span class="vname">{variantName[variant]}</span>
    <span class="score">
      {#if matchLength != null}
        матч {matchScore[0]}:{matchScore[1]} → {matchLength}
      {:else if matchScore[0] || matchScore[1]}
        счёт {matchScore[0]}:{matchScore[1]}
      {/if}
    </span>
  </header>

  {#if pos}
    <div class="winbar" title="Ваши шансы">
      <div class="fill" style="width: {(humanWin * 100).toFixed(0)}%"></div>
      <span class="label">вы {(humanWin * 100).toFixed(0)}%</span>
    </div>

    <Board
      position={pos}
      orientation={humanColor}
      interactive={phase === 'humanMove'}
      sources={phase === 'humanMove' ? [...sourceCells] : []}
      dests={destCells}
      selected={selSource == null ? null : phys(humanColor, selSource)}
      {lastCells}
      onPointClick={handlePointClick}
    />

    <div class="panel">
      {#if hasCube}
        <div class="cube-state">🎲² Куб: <strong>{pos.cube.value}</strong> ({cubeOwnerDesc(pos)})</div>
      {/if}
      {#if currentGameCrawford}
        <div class="crawford">⚑ Кроуфорд — удвоение запрещено</div>
      {/if}

      <p class="status">{status}</p>

      {#if phase === 'humanRoll'}
        <div class="moves">
          <button class="primary" onclick={humanRoll}>🎲 Бросить кости</button>
          {#if canHumanDouble}
            <button class="ghost" onclick={humanDouble}>⬆ Удвоить (→{pos.cube.value * 2})</button>
          {/if}
        </div>
      {:else if phase === 'rolling'}
        <p class="thinking">бросаем…</p>
      {:else if phase === 'cubeResponse'}
        <div class="moves">
          <button class="primary" onclick={cubeTake}>Тайк (взять)</button>
          <button class="ghost" onclick={cubeDrop}>Пас (сбросить)</button>
          {#if canBeaver}
            <button class="ghost" onclick={cubeBeaver}>Бивер</button>
          {/if}
        </div>
      {:else if phase === 'humanMove'}
        <div class="moves">
          {#if bearOffHop}
            <button class="primary" onclick={bearOffSelected}>Выкинуть шашку</button>
          {/if}
          {#if pending.length}
            <button class="ghost" onclick={undoPending}>↶ Отменить ({pending.length})</button>
          {/if}
          <button class="ghost" onclick={analyze} disabled={analyzing}>🔎 Оценка</button>
        </div>
        <details class="movelist">
          <summary>или выбрать ход из списка ({legal.length})</summary>
          <div class="moves">
            {#each legal as t}
              <button onclick={() => play(t.id)}>{fmtTurn(t)}</button>
            {/each}
          </div>
        </details>

        {#if ranked}
          <div class="analysis">
            {#if cubeDec}
              <div class="cube {cubeDec.action}">
                Куб: <strong>{cubeDec.note}</strong>
                {#if cubeDec.recommend_beaver}<span class="beaver">(бивер!)</span>{/if}
              </div>
            {/if}
            <ol class="ranked">
              {#each ranked.slice(0, 6) as r, i}
                <li class:best={i === 0}>
                  <span class="mv">{fmtTurn(r.turn)}</span>
                  <span class="num">win {(r.win * 100).toFixed(0)}%</span>
                  <span class="num loss">{i === 0 ? 'лучший' : `−${r.equity_loss.toFixed(3)}`}</span>
                </li>
              {/each}
            </ol>
          </div>
        {/if}
      {:else if phase === 'ai'}
        <p class="thinking">…</p>
      {:else if phase === 'over' || phase === 'error'}
        <div class="moves">
          {#if matchOver}
            <button class="primary" onclick={newMatch}>Новый матч</button>
          {:else}
            <button class="primary" onclick={nextGame}>
              {matchLength != null ? 'Следующая партия' : 'Новая партия'}
            </button>
          {/if}
          <button class="ghost" onclick={onExit}>В меню</button>
        </div>
      {/if}

      <div class="meta">пипы: ⚪ {pos.pip[0]} · ⚫ {pos.pip[1]} · ход №{pos.turn_number}</div>
    </div>
  {:else}
    <p class="status">{status}</p>
  {/if}
</div>

<style>
  .play {
    max-width: 760px;
    margin: 0 auto;
  }
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 0.5rem;
  }
  .vname {
    font-weight: 600;
    color: #6b4423;
  }
  .score {
    margin-left: auto;
    font-weight: 600;
    color: #444;
  }
  .link,
  .ghost {
    background: none;
    border: none;
    color: #6b4423;
    cursor: pointer;
    font: inherit;
  }
  .link:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .ghost {
    border: 1px solid #b5895c;
    border-radius: 6px;
    padding: 0.4rem 0.8rem;
  }
  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .winbar {
    position: relative;
    height: 22px;
    background: #333;
    border-radius: 11px;
    overflow: hidden;
    margin-bottom: 0.6rem;
  }
  .winbar .fill {
    height: 100%;
    background: linear-gradient(90deg, #e8e8e8, #cfcfcf);
    transition: width 0.4s ease;
  }
  .winbar .label {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font: 600 12px system-ui;
    color: #000;
    mix-blend-mode: difference;
    filter: invert(1);
  }
  .panel {
    margin-top: 1rem;
  }
  .cube-state {
    margin-bottom: 0.4rem;
    color: #444;
  }
  .crawford {
    margin-bottom: 0.4rem;
    color: #b22;
    font-weight: 600;
  }
  .status {
    font-weight: 600;
    min-height: 1.4em;
  }
  .moves {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }
  .moves button {
    font-family: ui-monospace, monospace;
    padding: 0.4rem 0.7rem;
    border: 1px solid #b5895c;
    background: #fff;
    border-radius: 6px;
    cursor: pointer;
  }
  .moves button:hover {
    background: #f3e6d2;
  }
  button.primary {
    font-size: 1rem;
    padding: 0.5rem 1.1rem;
    border: none;
    border-radius: 8px;
    background: #6b4423;
    color: #fff;
    cursor: pointer;
    font-family: system-ui, sans-serif;
  }
  .thinking {
    color: #888;
    font-style: italic;
  }
  .movelist {
    margin-top: 0.6rem;
    font-size: 0.9rem;
  }
  .movelist summary {
    cursor: pointer;
    color: #6b4423;
  }
  .movelist .moves {
    margin-top: 0.4rem;
  }
  .analysis {
    margin-top: 0.8rem;
    border: 1px solid #e2d3bb;
    border-radius: 8px;
    padding: 0.6rem 0.8rem;
    background: #fcf8f1;
  }
  .cube {
    margin-bottom: 0.5rem;
    font-size: 0.92rem;
  }
  .cube.too_good strong {
    color: #b22;
  }
  .cube.double_pass strong {
    color: #b80;
  }
  .cube.double_take strong {
    color: #2a6;
  }
  .beaver {
    color: #b22;
    font-weight: 600;
  }
  .ranked {
    list-style: none;
    margin: 0;
    padding: 0;
    font: 0.9rem ui-monospace, monospace;
  }
  .ranked li {
    display: flex;
    gap: 0.8rem;
    padding: 0.15rem 0;
    border-bottom: 1px dotted #e0d4bf;
  }
  .ranked li.best {
    font-weight: 700;
    color: #2a6;
  }
  .ranked .mv {
    flex: 1;
  }
  .ranked .num {
    width: 6.5rem;
    text-align: right;
    color: #777;
  }
  .ranked li.best .num {
    color: #2a6;
  }
  .meta {
    margin-top: 0.8rem;
    color: #777;
    font-size: 0.85rem;
  }
</style>
