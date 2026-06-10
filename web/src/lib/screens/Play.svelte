<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { engine } from '../../engine/client';
  import type {
    CheckerMoveDto,
    CubeDecisionDto,
    PlayerColor,
    PositionDto,
    RankedTurnDto,
    SequenceDto,
    TurnDto,
    VariantId,
  } from '../../engine/types';
  import Board from '../board/Board.svelte';
  import { phys, posOfPhys } from '../board/coords';
  import { setSoundEnabled, sfx } from '../sound';
  import { exportGame, newId, saveGame, type SavedGame } from '../storage';

  let {
    variant,
    aiPly,
    humanColor,
    matchLength,
    resume = null,
    boardStyle = '',
    onExit,
  }: {
    variant: VariantId;
    aiPly: number;
    humanColor: PlayerColor;
    matchLength: number | null;
    resume?: SavedGame | null;
    boardStyle?: string;
    onExit: () => void;
  } = $props();

  type Phase =
    | 'loading'
    | 'openingRoll'
    | 'humanRoll'
    | 'rolling'
    | 'humanMove'
    | 'cubeResponse'
    | 'ai'
    | 'over'
    | 'error';

  let pos = $state<PositionDto | null>(null);
  let legal = $state<TurnDto[]>([]); // deduped legal turns (move list / analysis / AI)
  let sequences = $state<SequenceDto[]>([]); // every legal ordering (drives move-building)
  let phase = $state<Phase>('loading');
  let status = $state('Загрузка…');
  let humanWin = $state(0.5);
  let displayWin = $state(0.5); // eased count-up of humanWin (bar + label tick together)
  $effect(() => {
    const target = humanWin;
    const start = untrack(() => displayWin);
    const reduce =
      typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (reduce || Math.abs(target - start) < 0.005) {
      displayWin = target;
      return;
    }
    const t0 = performance.now();
    const dur = 420;
    let raf = requestAnimationFrame(function step(now) {
      const k = Math.min(1, (now - t0) / dur);
      displayWin = start + (target - start) * (1 - Math.pow(1 - k, 3)); // ease-out cubic
      if (k < 1) raf = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(raf);
  });
  let ranked = $state<RankedTurnDto[] | null>(null);
  let cubeDec = $state<CubeDecisionDto | null>(null);
  let analyzing = $state(false);
  let pending = $state<CheckerMoveDto[]>([]); // sub-moves chosen so far this turn
  let selSource = $state<number | null>(null); // selected source (mover path pos)
  let lastCells = $state<number[]>([]); // physical cells touched by the last move (pulse)
  let glide = $state<{ from: number; to: number; color: PlayerColor; bearOff?: boolean } | null>(
    null,
  );
  let animating = $state(false);
  let aiLast = $state<{ d1: number; d2: number; moves: string } | null>(null); // comp's last roll+move
  let openRoll = $state<{ you: number; opp: number } | null>(null); // opening roll for first move

  // ---- game log: every played turn with its evaluation ----
  type LogEntry = {
    n: number; // turn number
    color: PlayerColor; // who moved
    dice: [number, number] | null;
    notation: string;
    win: number | null; // win% from the mover's perspective
    loss: number | null; // equity lost vs the best move (null = not scored)
    before: PositionDto; // board state before this move (to review / replay)
    ranked: RankedTurnDto[] | null; // alternative moves with evaluations (human moves)
    isHuman: boolean; // whether this was the human's move (replayable)
    cube?: boolean; // true → a cube action (double / take / pass / beaver), not a checker move
  };
  let history = $state<LogEntry[]>([]);
  let reviewIdx = $state<number | null>(null); // which log entry is expanded for review
  let logOpen = $state(true); // game-log panel expanded (plain div, not <details>, so
  // its scroll container flexes reliably — <details> wraps content in a box that
  // breaks flex-based internal scrolling)
  // collapsible win-bar + best-moves overview (preference persisted across games)
  function loadPref(key: string, def: boolean): boolean {
    try {
      const v = localStorage.getItem(key);
      return v == null ? def : v === '1';
    } catch {
      return def;
    }
  }
  function savePref(key: string, val: boolean) {
    try {
      localStorage.setItem(key, val ? '1' : '0');
    } catch {
      /* storage unavailable — preference just isn't remembered */
    }
  }
  let barOpen = $state(loadPref('opornik.barOpen', true)); // win-probability bar shown
  let analysisOpen = $state(loadPref('opornik.analysisOpen', true)); // best-moves panel shown
  let autoRoll = $state(loadPref('opornik.autoRoll', false)); // roll the dice automatically
  let soundOn = $state(loadPref('opornik.sound', true)); // synthesized sound effects
  $effect(() => savePref('opornik.barOpen', barOpen));
  $effect(() => savePref('opornik.analysisOpen', analysisOpen));
  $effect(() => savePref('opornik.autoRoll', autoRoll));
  $effect(() => {
    savePref('opornik.sound', soundOn);
    setSoundEnabled(soundOn);
  });
  // Auto-roll: when enabled, throw the dice automatically a beat after it becomes
  // your roll (and auto-play the opening throw). The short delay lets you SEE the
  // turn and still double first (the double button is up during the delay); the
  // guards inside humanRoll/doOpeningRoll make a late timer harmless.
  $effect(() => {
    if (!autoRoll) return;
    let act: (() => void) | null = null;
    if (phase === 'humanRoll') {
      act = () => {
        if (autoRoll && phase === 'humanRoll') void humanRoll();
      };
    } else if (phase === 'openingRoll' && !openRoll) {
      act = () => {
        if (autoRoll && phase === 'openingRoll' && !openRoll) void doOpeningRoll();
      };
    }
    if (!act) return;
    const t = setTimeout(act, 350);
    return () => clearTimeout(t);
  });
  function snap(p: PositionDto): PositionDto {
    return JSON.parse(JSON.stringify(p));
  }
  // severity of a move from its equity loss (human moves only)
  function sev(loss: number | null): '' | 'ok' | 'inacc' | 'blunder' {
    if (loss == null) return '';
    if (loss < 0.02) return 'ok';
    if (loss <= 0.08) return 'inacc';
    return 'blunder';
  }
  function evalLabel(h: LogEntry): string {
    if (h.loss == null) return h.win != null ? `${(h.win * 100).toFixed(0)}%` : '';
    if (h.loss < 0.0005) return '✓ лучший';
    const s = sev(h.loss);
    const word = s === 'blunder' ? 'ошибка' : s === 'inacc' ? 'неточность' : '';
    return `−${h.loss.toFixed(3)}${word ? ' · ' + word : ''}`;
  }

  // ---- post-game review (Разбор партии) ----
  // Structured result of the finished game; `winnerIsHuman: null` = draw.
  let gameResult = $state<{ winnerIsHuman: boolean | null; points: number } | null>(null);
  // Human checker moves the engine managed to score (loss != null). Cube entries
  // and AI moves carry no real loss (AI loss is hardcoded 0), so they are excluded.
  const scoredMoves = $derived(
    history
      .map((h, idx) => ({ idx, h }))
      .filter(({ h }) => h.isHuman && !h.cube && h.loss != null),
  );
  const reviewStats = $derived.by(() => {
    if (!scoredMoves.length) return null;
    let best = 0;
    let inacc = 0;
    let blunder = 0;
    let total = 0;
    for (const { h } of scoredMoves) {
      const l = h.loss!;
      total += l;
      if (l < 0.0005) best++;
      const s = sev(l);
      if (s === 'inacc') inacc++;
      else if (s === 'blunder') blunder++;
    }
    return { n: scoredMoves.length, best, inacc, blunder, avg: total / scoredMoves.length };
  });
  // Light-hearted skill label from the mean equity loss per scored move.
  function gradeLabel(avg: number): string {
    if (avg < 0.01) return 'мировой класс';
    if (avg < 0.02) return 'эксперт';
    if (avg < 0.04) return 'сильный игрок';
    if (avg < 0.07) return 'крепкий любитель';
    if (avg < 0.12) return 'любитель';
    return 'новичок';
  }
  const worstMoves = $derived(
    scoredMoves
      .filter(({ h }) => h.loss! >= 0.02)
      .sort((a, b) => b.h.loss! - a.h.loss!)
      .slice(0, 3),
  );
  // Your win chance after every checker move (AI entries flipped to your side),
  // closed with the actual result — the data behind the review sparkline.
  const sparkPoints = $derived.by(() => {
    const pts: number[] = [];
    for (const h of history) {
      if (h.cube || h.win == null) continue;
      pts.push(h.isHuman ? h.win : 1 - h.win);
    }
    if (gameResult && gameResult.winnerIsHuman != null) {
      pts.push(gameResult.winnerIsHuman ? 1 : 0);
    }
    if (pts.length < 2) return '';
    const last = pts.length - 1;
    return pts
      .map((p, i) => `${((i / last) * 100).toFixed(2)},${(2 + (1 - p) * 24).toFixed(2)}`)
      .join(' ');
  });
  // Jump from the review panel to a move's expanded log entry.
  function openReview(idx: number) {
    logOpen = true;
    reviewIdx = idx;
    setTimeout(() => {
      document
        .querySelector(`.gamelog .log li:nth-child(${idx + 1})`)
        ?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }, 30);
  }

  const ANIM_MS = 300; // matches the board glide (pick-up → travel → settle)
  const BEAR_MS = 440; // bear-off arc into the tray
  const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));
  let matchScore = $state<[number, number]>([0, 0]); // [human, ai] points
  let crawfordPlayed = $state(false);
  let currentGameCrawford = $state(false);
  let pendingPre = $state(1); // cube value before a pending double, for pass resolution

  const aiColor: PlayerColor = $derived(humanColor === 'white' ? 'black' : 'white');
  const hasCube = $derived(variant === 'nardegammon' || variant === 'hachapuri');
  const canBeaver = $derived(variant === 'hachapuri');
  // A player exactly 1 point from winning the match is at "match point": the cube is
  // useless to them — any win already wins the match, so doubling can only RISK giving
  // the opponent more for zero upside. They must never double. This holds in the
  // Crawford game AND every post-Crawford game. (The Crawford game additionally bars
  // the TRAILER from doubling, via pos.crawford.)
  const humanOneAway = $derived(matchLength != null && matchScore[0] === matchLength - 1);
  const aiOneAway = $derived(matchLength != null && matchScore[1] === matchLength - 1);
  const canHumanDouble = $derived(
    hasCube &&
      phase === 'humanRoll' &&
      pos != null &&
      pos.cube.value < 64 &&
      !pos.crawford &&
      !humanOneAway &&
      (pos.cube.owner === null || pos.cube.owner === humanColor),
  );
  const doubleTo = $derived(pos ? pos.cube.value * 2 : 0); // value after a pending double
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
  // Move-building is driven by the engine's legal *ordered* sequences (every legal
  // ordering, each legal at every intermediate step — head rule, blocked landings,
  // the transient full-prime ban and the bear-off over-roll rule are all enforced
  // by the generator). The player's chosen sub-moves (`pending`) are matched as an
  // ordered PREFIX of those sequences; the legal next sub-moves are whatever comes
  // next in any sequence that still matches. This replaces the old multiset match
  // against the deduped turn list, which silently forbade some legal orderings
  // (e.g. playing the smaller die first to reach the same square).
  function sameMove(a: CheckerMoveDto, b: CheckerMoveDto) {
    return a.from === b.from && a.die === b.die && a.to === b.to && a.bear_off === b.bear_off;
  }
  function isPrefix(seq: CheckerMoveDto[], p: CheckerMoveDto[]) {
    if (seq.length < p.length) return false;
    for (let i = 0; i < p.length; i++) if (!sameMove(seq[i], p[i])) return false;
    return true;
  }
  function hopKey(m: CheckerMoveDto) {
    return `${m.from}-${m.die}-${m.to}-${m.bear_off ? 1 : 0}`;
  }
  // The legal next single sub-moves given the sub-moves chosen so far (`p`).
  function availableHops(p: CheckerMoveDto[]): CheckerMoveDto[] {
    if (phase !== 'humanMove') return [];
    const hops = new Map<string, CheckerMoveDto>();
    for (const s of sequences) {
      if (s.moves.length <= p.length || !isPrefix(s.moves, p)) continue;
      const nx = s.moves[p.length];
      hops.set(hopKey(nx), nx);
    }
    return [...hops.values()];
  }
  const nextHops = $derived(availableHops(pending));
  const sourceCells = $derived(new Set(nextHops.map((h) => phys(humanColor, h.from))));

  // For the selected checker: every reachable landing cell → the chain of hops to
  // reach it (so a single drag/click can play the FINAL square, using both dice).
  const reachable = $derived.by<Map<number, CheckerMoveDto[]>>(() => {
    const map = new Map<number, CheckerMoveDto[]>();
    if (selSource == null) return map;
    const visit = (cur: number, sim: CheckerMoveDto[], chain: CheckerMoveDto[], depth: number) => {
      if (depth >= 4) return; // at most 4 dice (doubles)
      for (const h of availableHops(sim)) {
        if (h.bear_off || h.to <= 0 || h.from !== cur) continue;
        const cell = phys(humanColor, h.to);
        const next = [...chain, h];
        if (!map.has(cell)) map.set(cell, next);
        visit(h.to, [...sim, h], next, depth + 1);
      }
    };
    visit(selSource, pending, [], 0);
    return map;
  });
  const destCells = $derived([...reachable.keys()]);
  // Chain (possibly multi-hop) that ends in bearing the selected checker off.
  const bearOffChain = $derived.by<CheckerMoveDto[] | null>(() => {
    if (selSource == null) return null;
    const find = (
      cur: number,
      sim: CheckerMoveDto[],
      chain: CheckerMoveDto[],
      depth: number,
    ): CheckerMoveDto[] | null => {
      if (depth >= 4) return null;
      const hops = availableHops(sim).filter((h) => h.from === cur);
      const bo = hops.find((h) => h.bear_off);
      if (bo) return [...chain, bo];
      for (const h of hops) {
        if (h.bear_off || h.to <= 0) continue;
        const r = find(h.to, [...sim, h], [...chain, h], depth + 1);
        if (r) return r;
      }
      return null;
    };
    return find(selSource, pending, [], 0);
  });
  // Physical cell of the selected checker when it can bear off (for swipe-up).
  const bearOffCell = $derived(
    selSource != null && bearOffChain ? phys(humanColor, selSource) : null,
  );

  // Apply a list of sub-moves for `color` to a copy of `base` (purely visual).
  function posWithMoves(
    base: PositionDto,
    color: PlayerColor,
    moves: CheckerMoveDto[],
  ): PositionDto {
    const arr = color === 'white' ? base.white : base.black;
    const m = new Map<number, number>();
    for (const p of arr) m.set(p.pos, p.count);
    let off = color === 'white' ? base.off[0] : base.off[1];
    for (const h of moves) {
      m.set(h.from, (m.get(h.from) ?? 0) - 1);
      if (h.bear_off || h.to <= 0) off += 1;
      else m.set(h.to, (m.get(h.to) ?? 0) + 1);
    }
    const points = [...m.entries()]
      .filter(([, c]) => c > 0)
      .map(([p, count]) => ({ pos: p, player: color, count }));
    const next: PositionDto = { ...base };
    if (color === 'white') {
      next.white = points;
      next.off = [off, base.off[1]];
    } else {
      next.black = points;
      next.off = [base.off[0], off];
    }
    return next;
  }

  // Board shows the human turn as it's built (null = no pending → show `pos`).
  const displayPos = $derived.by<PositionDto | null>(() =>
    !pos || pending.length === 0 ? null : posWithMoves(pos, humanColor, pending),
  );
  // Intermediate board during the AI's animated move (null when not animating).
  let aiAnim = $state<PositionDto | null>(null);

  // The legal sequence the pending sub-moves exactly complete (if any) — and the
  // deduped turn id it commits to via `play()`.
  const completeSeq = $derived.by<SequenceDto | null>(() => {
    if (phase !== 'humanMove' || pending.length === 0) return null;
    return (
      sequences.find((s) => s.moves.length === pending.length && isPrefix(s.moves, pending)) ?? null
    );
  });

  function clearMoveBuild() {
    pending = [];
    selSource = null;
  }

  function handlePointClick(cell: number) {
    if (phase !== 'humanMove' || animating) return;
    const p = posOfPhys(humanColor, cell);
    // clicking the already-selected checker deselects it
    if (selSource === p) {
      selSource = null;
      return;
    }
    // with a checker selected, a reachable target WINS — even when that point
    // already holds your own checkers (stacking). This is the move-priority fix
    // for "can't move a checker onto another checker".
    if (selSource != null && reachable.has(cell)) {
      void applyChain(reachable.get(cell)!, true);
      return;
    }
    // otherwise (re)select a movable checker, or clear
    if (nextHops.some((h) => h.from === p)) {
      selSource = p;
      return;
    }
    selSource = null;
  }

  // Drag release over a destination cell — play the full chain to the FINAL square
  // (both dice in one gesture). The ghost already showed the motion, so no glide.
  function onDrop(cell: number) {
    if (phase !== 'humanMove' || animating || selSource == null) return;
    if (reachable.has(cell)) void applyChain(reachable.get(cell)!, false);
  }

  // Apply a chain of sub-moves (one checker, one or more dice). Glides each hop
  // when `animate`. NEVER auto-commits — the player confirms.
  async function applyChain(hops: CheckerMoveDto[], animate: boolean) {
    if (animating || hops.length === 0) return;
    for (const h of hops) {
      if (animate && h.bear_off) {
        animating = true;
        glide = { from: phys(humanColor, h.from), to: -1, color: humanColor, bearOff: true };
        pending = [...pending, h];
        sfx.bear();
        await delay(BEAR_MS);
        glide = null;
        animating = false;
      } else if (animate && h.to > 0) {
        animating = true;
        glide = { from: phys(humanColor, h.from), to: phys(humanColor, h.to), color: humanColor };
        pending = [...pending, h];
        sfx.move();
        await delay(ANIM_MS);
        glide = null;
        animating = false;
      } else {
        pending = [...pending, h];
        // non-animated hops land in the same instant — voice the gesture once,
        // not one stacked click per hop
        if (h === hops[0]) {
          if (h.bear_off) sfx.bear();
          else sfx.move();
        }
      }
    }
    // keep the checker selected if it can still move (smooth chaining), else clear
    const last = hops[hops.length - 1];
    selSource =
      !last.bear_off && last.to > 0 && availableHops(pending).some((h) => h.from === last.to)
        ? last.to
        : null;
  }

  function bearOffSelected() {
    if (bearOffChain) void applyChain(bearOffChain, true);
  }
  function undoPending() {
    if (animating) return;
    pending = pending.slice(0, -1);
    selSource = null;
  }
  // Load a full turn from the list as a pending preview (player then confirms).
  function previewTurn(t: TurnDto) {
    if (animating) return;
    pending = [...t.moves];
    selSource = null;
  }
  function confirmMove() {
    if (completeSeq) void play(completeSeq.turn_id, true);
  }

  // ---- provably-fair dice ----
  // A d6 from the platform CSPRNG (crypto.getRandomValues) with REJECTION SAMPLING
  // to remove modulo bias entirely: a byte is 0..255; 252 = 6×42, so 0..251 map
  // uniformly to faces 1..6 (42 byte-values each) and 252..255 are rejected and
  // redrawn. Result: every face is EXACTLY 1/6, and the source is cryptographically
  // strong (unpredictable, unseedable). Math.random is only a fallback — it's
  // statistically uniform but a non-crypto PRNG with a (negligible) multiply bias.
  const dieBuf = new Uint8Array(1);
  function rollDie(): number {
    const c = globalThis.crypto;
    let face: number;
    if (c && c.getRandomValues) {
      let v: number;
      do {
        c.getRandomValues(dieBuf);
        v = dieBuf[0];
      } while (v >= 252);
      face = (v % 6) + 1;
    } else {
      face = 1 + Math.floor(Math.random() * 6);
    }
    bumpDiceStat(face);
    return face;
  }

  // Per-face counts of EVERY die thrown (you + the engine), accumulated across the
  // session in localStorage so fairness can be checked over a large sample.
  const DICE_KEY = 'opornik.diceStats';
  function loadDiceStats(): number[] {
    try {
      const raw = localStorage.getItem(DICE_KEY);
      if (raw) {
        const a = JSON.parse(raw);
        if (Array.isArray(a) && a.length === 7) return a.map((n) => Number(n) || 0);
      }
    } catch {
      /* ignore */
    }
    return [0, 0, 0, 0, 0, 0, 0]; // index 1..6 used
  }
  let diceStats = $state<number[]>(loadDiceStats());
  let fairOpen = $state(false);
  function bumpDiceStat(face: number) {
    diceStats[face] += 1;
    try {
      localStorage.setItem(DICE_KEY, JSON.stringify(diceStats));
    } catch {
      /* ignore */
    }
  }
  const diceTotal = $derived(diceStats.reduce((a, b) => a + b, 0));
  // Largest deviation of any face from the ideal 1/6 (a quick fairness read-out).
  const diceMaxDev = $derived.by(() => {
    if (diceTotal === 0) return 0;
    let m = 0;
    for (let f = 1; f <= 6; f++) m = Math.max(m, Math.abs(diceStats[f] / diceTotal - 1 / 6));
    return m * 100;
  });

  // ---- куши (combination counts) + roll sequence ----
  // kushStats: count per unordered combination "lo-hi" (persisted, like the faces).
  // rollSeq: the chronological list of throws with who rolled (session-only — it
  // grows unbounded, so it isn't persisted).
  const KUSH_KEY = 'opornik.kushStats';
  function loadKush(): Record<string, number> {
    try {
      const r = localStorage.getItem(KUSH_KEY);
      if (r) {
        const o = JSON.parse(r);
        if (o && typeof o === 'object' && !Array.isArray(o)) return o as Record<string, number>;
      }
    } catch {
      /* ignore */
    }
    return {};
  }
  let kushStats = $state<Record<string, number>>(loadKush());
  let rollSeq = $state<{ who: 'you' | 'opp'; a: number; b: number; opening?: boolean }[]>([]);
  let diceTab = $state<'faces' | 'kush' | 'seq'>('faces');
  // The 21 distinct combinations, in order (1-1,1-2,…,1-6,2-2,…,6-6).
  const KUSH_COMBOS: [number, number][] = (() => {
    const out: [number, number][] = [];
    for (let a = 1; a <= 6; a++) for (let b = a; b <= 6; b++) out.push([a, b]);
    return out;
  })();
  function kushKey(a: number, b: number) {
    return `${Math.min(a, b)}-${Math.max(a, b)}`;
  }
  // Record one throw (a pair) by `who`. Per-die fairness counts are bumped in
  // rollDie; this adds the combination + sequence.
  function recordRoll(who: 'you' | 'opp', a: number, b: number, opening = false) {
    if (!opening) sfx.dice(); // the opening throw sounds when its dice APPEAR (doOpeningRoll)
    rollSeq = [...rollSeq, { who, a, b, opening }];
    const k = kushKey(a, b);
    kushStats[k] = (kushStats[k] ?? 0) + 1;
    try {
      localStorage.setItem(KUSH_KEY, JSON.stringify(kushStats));
    } catch {
      /* ignore */
    }
  }
  const kushTotal = $derived(Object.values(kushStats).reduce((s, n) => s + n, 0));
  const doublesCount = $derived(
    [1, 2, 3, 4, 5, 6].reduce((n, f) => n + (kushStats[`${f}-${f}`] ?? 0), 0),
  );

  function resetDiceStats() {
    diceStats = [0, 0, 0, 0, 0, 0, 0];
    kushStats = {};
    rollSeq = [];
    try {
      localStorage.removeItem(DICE_KEY);
      localStorage.removeItem(KUSH_KEY);
    } catch {
      /* ignore */
    }
  }
  // Signed equity, e.g. +0.412 / −0.683 (− is a real minus glyph for alignment).
  function fmtEq(e: number): string {
    return (e >= 0 ? '+' : '−') + Math.abs(e).toFixed(3);
  }
  // Dot layout per die value (indices into a 3×3 grid) — for the AI-roll readout.
  const PIPS: Record<number, number[]> = {
    1: [4],
    2: [0, 8],
    3: [0, 4, 8],
    4: [0, 2, 6, 8],
    5: [0, 2, 4, 6, 8],
    6: [0, 2, 3, 5, 6, 8],
  };
  // Render a turn, collapsing a single checker's chained sub-moves (24→20→14)
  // into one segment (24/14) so it reads as ONE checker moving, not two.
  function fmtTurn(t: TurnDto): string {
    if (t.is_pass) return 'пропуск';
    const ms = t.moves;
    const segs: string[] = [];
    let i = 0;
    while (i < ms.length) {
      const start = ms[i].from;
      let j = i;
      // extend while the next sub-move continues the same checker (its source
      // is this hop's destination), stopping at a bear-off
      while (j + 1 < ms.length && !ms[j].bear_off && ms[j + 1].from === ms[j].to) j++;
      segs.push(ms[j].bear_off ? `${start}/выкид` : `${start}/${ms[j].to}`);
      i = j + 1;
    }
    return segs.join('  ');
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
    // never leave an animation overlay stuck on an error screen
    aiAnim = null;
    glide = null;
    animating = false;
  }
  function isOver(p: PositionDto) {
    return p.outcome.kind === 'win' || p.outcome.kind === 'draw';
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

  // Record a cube action (double offer / take / pass / beaver) in the game sheet
  // so the лист партии reflects the full doubling history, not just checker moves.
  function logCube(color: PlayerColor, text: string) {
    if (!pos) return;
    history = [
      ...history,
      {
        n: pos.turn_number,
        color,
        dice: null,
        notation: text,
        win: null,
        loss: null,
        before: snap(pos),
        ranked: null,
        isHuman: false,
        cube: true,
      },
    ];
  }

  function finishGame(winner: PlayerColor, points: number, reason: string, silent = false) {
    if (winner === humanColor) matchScore[0] += points;
    else matchScore[1] += points;
    if (currentGameCrawford) crawfordPlayed = true;
    gameResult = { winnerIsHuman: winner === humanColor, points };
    if (!silent) {
      if (winner === humanColor) sfx.win();
      else sfx.lose();
    }
    if (matchLength != null && (matchScore[0] >= matchLength || matchScore[1] >= matchLength)) {
      const humanWon = matchScore[0] >= matchLength;
      status = `${humanWon ? '🏆 Вы выиграли матч!' : 'Матч за движком.'} Счёт ${matchScore[0]}:${matchScore[1]}.`;
    } else {
      const tail = matchLength != null ? ` (матч ${matchScore[0]}:${matchScore[1]})` : ` (счёт ${matchScore[0]}:${matchScore[1]})`;
      status = `${reason}: ${winner === humanColor ? 'вы' : 'движок'} +${points}${tail}`;
    }
    phase = 'over';
  }

  // `silent` suppresses the win/lose jingle — used when RESTORING an already
  // finished game (save/import), where the result is old news, not an event.
  function announceBoardWin(p: PositionDto, silent = false) {
    if (p.outcome.kind === 'draw') {
      // Classic last-roll equalisation: both sides borne off — no points.
      if (currentGameCrawford) crawfordPlayed = true;
      status = `Ничья — обе стороны вывели все шашки${
        matchLength != null ? ` (матч ${matchScore[0]}:${matchScore[1]})` : ''
      }.`;
      gameResult = { winnerIsHuman: null, points: 0 };
      phase = 'over';
      return;
    }
    const points = (p.outcome.points ?? 1) * p.cube.value;
    finishGame(p.outcome.winner as PlayerColor, points, p.outcome.mars ? 'марс' : 'оин', silent);
  }

  // Mars is impossible (so a single-point оин concession is allowed) once the human
  // has at least one checker off. Read the DISPLAYED position so a bear-off you've
  // already built (but not yet confirmed) counts — the board shows that checker off
  // the board, so resigning must read оин, not марс. (Bug fix: while a bear-off was
  // pending the off-tray showed off≥1, yet resign used the pre-move committed off
  // and wrongly demanded a марс.)
  const canResignSingle = $derived.by(() => {
    const p = displayPos ?? pos;
    return p ? (humanColor === 'white' ? p.off[0] : p.off[1]) >= 1 : false;
  });

  // The human concedes the game: оин (1 pt) or марс (2 pt), times the cube.
  function resign(mars: boolean) {
    if (!pos || animating || (phase !== 'humanRoll' && phase !== 'humanMove')) return;
    if (!mars && !canResignSingle) return; // оин only when mars is impossible
    const points = (mars ? 2 : 1) * pos.cube.value;
    clearAnalysis();
    clearMoveBuild();
    finishGame(aiColor, points, mars ? 'Сдача с марсом' : 'Сдача (оин)');
  }
  // Resign has exactly ONE correct outcome per position (no оин/марс choice to
  // make): оин once a checker is borne off, else марс — ×cube. The button just
  // asks for confirmation.
  let confirmResign = $state(false);
  const resignPoints = $derived(pos ? (canResignSingle ? 1 : 2) * pos.cube.value : 0);
  function askResign() {
    if (!pos || animating || (phase !== 'humanRoll' && phase !== 'humanMove')) return;
    confirmResign = true;
  }
  function doResign() {
    confirmResign = false;
    resign(!canResignSingle); // оин when mars is impossible, else марс
  }

  async function continueAfter() {
    confirmResign = false;
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

  // Resume from a saved/imported game: restore the board + match context and
  // pick the right phase to continue from.
  async function resumeFrom(g: SavedGame) {
    await engine.init(g.variant);
    matchScore = [g.matchScore[0], g.matchScore[1]];
    crawfordPlayed = g.crawfordPlayed;
    pos = await engine.setPosition(g.setup);
    currentGameCrawford = !!g.setup.crawford;
    await updateWin();
    if (isOver(pos)) {
      announceBoardWin(pos, true); // restored result — no jingle
      return;
    }
    if (pos.turn === humanColor) {
      if (pos.dice) {
        await loadLegal();
        clearMoveBuild();
        if (legal.length === 1 && legal[0].is_pass) {
          status = 'Ходов нет, пропуск.';
          await play(legal[0].id, true);
          return;
        }
        status = `Партия загружена. Кости ${pos.dice[0]}-${pos.dice[1]}: ваш ход.`;
        phase = 'humanMove';
        void analyze();
      } else {
        phase = 'humanRoll';
        status = 'Партия загружена. Ваш ход — бросайте кости.';
      }
    } else {
      await aiTurn();
    }
  }

  // ---- opening roll: each side throws one die, higher goes first (ФСНР) ----
  function startOpeningRoll() {
    openRoll = null;
    phase = 'openingRoll';
    status = 'Розыгрыш первого хода: бросьте кость.';
  }
  async function doOpeningRoll() {
    if (phase !== 'openingRoll' || openRoll) return;
    let you = rollDie();
    let opp = rollDie();
    while (you === opp) {
      // a tie can't decide the order → re-roll (only the order needs deciding)
      you = rollDie();
      opp = rollDie();
    }
    openRoll = { you, opp };
    sfx.dice(); // the dice are visible NOW; recordRoll below runs after the pause
    status = `Вы: ${you} · соперник: ${opp}`;
    await delay(1100); // let the player read the dice
    const humanFirst = you > opp;
    recordRoll(humanFirst ? 'you' : 'opp', you, opp, true); // opening pair (played by the higher roller)
    try {
      // ФСНР: the first move is played with the two dice that just came up (one
      // from each player); the higher roller goes first — there is NO re-roll.
      pos = await engine.setTurn(humanFirst ? humanColor : aiColor);
      await engine.setDice(you, opp);
      await updateWin();
      if (humanFirst) {
        await enterHumanMove(you, opp);
      } else {
        status = `Первым ходит соперник (кости ${you}-${opp})…`;
        await aiRoll([you, opp]);
      }
    } catch (e) {
      fail(e);
    }
  }

  onMount(async () => {
    try {
      if (resume) {
        await resumeFrom(resume);
        return;
      }
      pos = await engine.init(variant);
      currentGameCrawford = crawfordForNext();
      pos = await engine.setCrawford(currentGameCrawford);
      await updateWin();
      if (isOver(pos)) {
        announceBoardWin(pos, true); // restored result — no jingle
        return;
      }
      startOpeningRoll();
    } catch (e) {
      fail(e);
    }
  });

  // ---- save / export the current game so it can be resumed later ----
  let saveMsg = $state('');
  let sheetMsg = $state(''); // confirmation for the game-sheet (лист партии) export
  let exportStr = $state('');
  function defaultName(): string {
    const sc =
      matchLength != null
        ? `матч ${matchScore[0]}:${matchScore[1]}`
        : `${matchScore[0]}:${matchScore[1]}`;
    return `${variantName[variant]} · ${sc} · ход ${pos?.turn_number ?? 0}`;
  }
  function currentSaved(): SavedGame | null {
    if (!pos) return null;
    return {
      v: 1,
      id: newId(),
      name: defaultName(),
      savedAt: new Date().toISOString(),
      variant,
      aiPly,
      humanColor,
      matchLength,
      matchScore: [matchScore[0], matchScore[1]],
      crawfordPlayed,
      setup: {
        variant,
        turn: pos.turn,
        white: pos.white,
        black: pos.black,
        off: pos.off,
        dice: pos.dice,
        cube: pos.cube,
        crawford: pos.crawford,
        turn_number: pos.turn_number,
      },
    };
  }
  function flash(msg: string) {
    saveMsg = msg;
    setTimeout(() => (saveMsg = ''), 2500);
  }
  function doSave() {
    const g = currentSaved();
    if (!g) return;
    saveGame(g);
    flash('Партия сохранена ✓');
  }
  async function doExport() {
    const g = currentSaved();
    if (!g) return;
    exportStr = exportGame(g);
    try {
      await navigator.clipboard.writeText(exportStr);
      flash('Строка скопирована ✓');
    } catch {
      flash('Скопируйте строку ниже');
    }
  }

  // ---- save the full game sheet (лист партии) as a readable transcript ----
  function transcriptText(): string {
    const L: string[] = [];
    const me = humanColor === 'white' ? '⚪ белые' : '⚫ чёрные';
    const opp = aiColor === 'white' ? '⚪ белые' : '⚫ чёрные';
    L.push(`Длинные нарды — ${variantName[variant]}`);
    L.push(
      matchLength != null
        ? `Матч до ${matchLength} · счёт ${matchScore[0]}:${matchScore[1]}`
        : `Счёт ${matchScore[0]}:${matchScore[1]}`,
    );
    L.push(`Вы: ${me} · движок: ${opp} (поиск ${aiPly}-ply)`);
    L.push(`Сохранено: ${new Date().toLocaleString('ru-RU')}`);
    L.push('');
    L.push([pad('№', 4), pad('игрок', 8), pad('кости', 6), pad('ход', 22), 'оценка'].join(' '));
    L.push('─'.repeat(52));
    for (const h of history) {
      const who = h.color === humanColor ? 'вы' : 'движок';
      if (h.cube) {
        L.push(`${pad(String(h.n), 4)} ${pad(who, 8)} 🎲² ${h.notation}`);
        continue;
      }
      const dice = h.dice ? `${h.dice[0]}-${h.dice[1]}` : '';
      L.push(
        [pad(String(h.n), 4), pad(who, 8), pad(dice, 6), pad(h.notation, 22), evalLabel(h)].join(' '),
      );
    }
    L.push('─'.repeat(52));
    if (pos) L.push(`Пипсы: ⚪ ${pos.pip[0]} · ⚫ ${pos.pip[1]} · ход №${pos.turn_number}`);
    if (phase === 'over') L.push(status);
    L.push('');
    L.push('opornik · https://rmv0x11.github.io/opornik/');
    return L.join('\n');
  }
  function pad(s: string, n: number): string {
    return s.length >= n ? s : s + ' '.repeat(n - s.length);
  }
  function flashSheet(msg: string) {
    sheetMsg = msg;
    setTimeout(() => (sheetMsg = ''), 2800);
  }
  async function saveTranscript() {
    if (!history.length) return;
    const text = transcriptText();
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-');
    const fname = `opornik-${variant}-${stamp}.txt`;
    let downloaded = false;
    try {
      const blob = new Blob([text], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = fname;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(url), 2000);
      downloaded = true;
    } catch {
      /* download unsupported — fall back to clipboard only */
    }
    try {
      await navigator.clipboard.writeText(text);
      flashSheet(downloaded ? 'Лист партии скачан и скопирован ✓' : 'Лист партии скопирован ✓');
    } catch {
      flashSheet(downloaded ? 'Лист партии скачан ✓' : 'Скопируйте лист вручную');
    }
  }

  // ---- review / replay from the game log ----
  function toggleReview(i: number) {
    reviewIdx = reviewIdx === i ? null : i;
  }
  // Rewind the engine to just before the move at `idx`, discard everything after.
  async function rewindTo(idx: number): Promise<boolean> {
    const e = history[idx];
    if (!e) return false;
    // Replaying out of a FINISHED game (the review panel invites this): take back
    // what finishGame already awarded, otherwise the replayed game's own finish
    // would double-count the score (and could falsely end a match).
    if (phase === 'over' && gameResult) {
      if (gameResult.winnerIsHuman === true) matchScore[0] -= gameResult.points;
      else if (gameResult.winnerIsHuman === false) matchScore[1] -= gameResult.points;
      if (currentGameCrawford) crawfordPlayed = false;
    }
    gameResult = null;
    reviewIdx = null;
    clearAnalysis();
    clearMoveBuild();
    aiLast = null;
    aiAnim = null;
    glide = null;
    pos = await engine.setPosition(e.before);
    history = history.slice(0, idx);
    return true;
  }
  // "Переиграть": rewind to a human move and re-enter the move phase (same dice).
  async function replayFrom(idx: number) {
    if (!history[idx]?.isHuman) return;
    try {
      const dice = history[idx].dice;
      if (!(await rewindTo(idx))) return;
      await loadLegal();
      await updateWin();
      if (legal.length === 1 && legal[0].is_pass) {
        status = 'Ходов нет, пропуск.';
        await play(legal[0].id, true);
        return;
      }
      phase = 'humanMove';
      status = dice ? `Переиграйте ход (кости ${dice[0]}-${dice[1]}).` : 'Переиграйте ход.';
      void analyze();
    } catch (e) {
      fail(e);
    }
  }
  // Replay a specific alternative move directly from the review list.
  async function replayWith(idx: number, alt: TurnDto) {
    if (!history[idx]?.isHuman) return;
    try {
      if (!(await rewindTo(idx))) return;
      await loadLegal();
      const match =
        legal.find((t) => fmtTurn(t) === fmtTurn(alt)) ?? legal.find((t) => t.id === alt.id);
      if (!match) {
        phase = 'humanMove';
        status = 'Переиграйте ход.';
        return;
      }
      await play(match.id, true);
    } catch (e) {
      fail(e);
    }
  }

  // Fetch the deduped legal turns (list / analysis / AI) and the legal ordered
  // sequences that drive interactive move-building, together.
  async function loadLegal() {
    legal = await engine.legalTurns();
    sequences = await engine.legalSequences();
  }

  // Enter the move-building phase with dice ALREADY set on the engine.
  async function enterHumanMove(d1: number, d2: number) {
    pos = await engine.getPosition();
    await loadLegal();
    clearMoveBuild();
    if (legal.length === 1 && legal[0].is_pass) {
      status = `Кости ${d1}-${d2}: ходов нет, пропуск.`;
      await play(legal[0].id, true);
      return;
    }
    status = `Кости ${d1}-${d2}: кликните шашку или выберите ход.`;
    phase = 'humanMove';
    void analyze(); // обзор лучших ходов всегда на виду (без нажатия кнопки)
  }

  async function humanRoll() {
    if (phase !== 'humanRoll') return;
    confirmResign = false;
    phase = 'rolling';
    clearAnalysis();
    try {
      const d1 = rollDie(),
        d2 = rollDie();
      recordRoll('you', d1, d2);
      await engine.setDice(d1, d2);
      await enterHumanMove(d1, d2);
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
        logCube(humanColor, `удвоение ×${pre} → ×${pos.cube.value}`);
        logCube(aiColor, 'тайк (взял)');
        status = `Соперник взял (тайк). Куб ×${pos.cube.value}. Ваш ход — бросайте.`;
        phase = 'humanRoll';
      } else {
        logCube(humanColor, `удвоение ×${pre} → ×${pre * 2}`);
        logCube(aiColor, 'пас (сброс)');
        finishGame(humanColor, pre, 'Соперник пасанул');
      }
    } catch (e) {
      fail(e);
    }
  }

  async function play(id: number, fromRoll = false) {
    if (!fromRoll && phase !== 'humanMove') return;
    const chosen = legal.find((t) => t.id === id) ?? null;
    const moverColor = pos ? pos.turn : humanColor;
    const moverDice = pos ? pos.dice : null;
    const moverN = pos ? pos.turn_number : 0;
    const beforeSnap = pos ? snap(pos) : null;
    const wasHuman = moverColor === humanColor;
    legal = [];
    sequences = [];
    clearMoveBuild();
    clearAnalysis();
    phase = 'ai';
    status = 'Ход движка…';
    try {
      // score the human's chosen move + capture all alternatives for review
      let ev: { win: number; loss: number } | null = null;
      let rankedAll: RankedTurnDto[] | null = null;
      if (chosen && !chosen.is_pass) {
        try {
          const r = await engine.analyze(1);
          rankedAll = r.slice(0, 8);
          const mine = r.find((x) => x.turn.id === id);
          if (mine) ev = { win: mine.win, loss: mine.equity_loss };
        } catch {
          /* eval is best-effort */
        }
      }
      const before = pos;
      pos = await engine.applyTurn(id);
      if (before) lastCells = changedCells(before, pos);
      if (beforeSnap) {
        history = [
          ...history,
          {
            n: moverN,
            color: moverColor,
            dice: moverDice,
            notation: chosen ? fmtTurn(chosen) : '—',
            win: ev?.win ?? null,
            loss: ev?.loss ?? null,
            before: beforeSnap,
            ranked: rankedAll,
            isHuman: wasHuman,
          },
        ];
      }
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
        !aiOneAway &&
        (pos.cube.owner === null || pos.cube.owner === aiColor)
      ) {
        const cd = await engine.cubeDecision();
        if (cd.action === 'double_take' || cd.action === 'double_pass') {
          pendingPre = pos.cube.value;
          pos = await engine.offerDouble();
          await updateWin();
          logCube(aiColor, `удвоение ×${pendingPre} → ×${pos.cube.value}`);
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

  // Glide the AI's chosen turn across the board, one sub-move at a time.
  async function animateTurn(turn: TurnDto, color: PlayerColor, base: PositionDto) {
    if (turn.is_pass) return;
    let cur = base;
    aiAnim = cur;
    for (const mv of turn.moves) {
      const bear = mv.bear_off;
      if (bear) {
        glide = { from: phys(color, mv.from), to: -1, color, bearOff: true };
      } else if (mv.to > 0) {
        glide = { from: phys(color, mv.from), to: phys(color, mv.to), color };
      }
      cur = posWithMoves(cur, color, [mv]);
      aiAnim = cur;
      if (bear) sfx.bear();
      else sfx.move();
      await delay(bear ? BEAR_MS : ANIM_MS);
      glide = null;
    }
    // keep the final animated board on screen; aiRoll clears it once `pos` commits
  }

  // Play one AI turn. `preset` forces the dice (used for the opening move, which
  // is played with the two dice from the opening roll — no re-roll); otherwise the
  // AI rolls its own pair.
  async function aiRoll(preset?: [number, number]) {
    const d1 = preset ? preset[0] : rollDie();
    const d2 = preset ? preset[1] : rollDie();
    if (!preset) recordRoll('opp', d1, d2); // preset = opening, already recorded
    const before = pos;
    await engine.setDice(d1, d2);
    pos = await engine.getPosition(); // reflect the dice on the board first
    const beforeSnap = pos ? snap(pos) : null;
    const bm = await engine.bestMove(aiPly);
    const aiN = before ? before.turn_number : (pos?.turn_number ?? 0);
    status = `Движок сыграл ${d1}-${d2}.`;
    if (pos) await animateTurn(bm.turn, aiColor, pos); // glide the move
    pos = await engine.applyTurn(bm.turn.id); // commit the exact engine state
    aiAnim = null; // committed state now drives the board (no snap-back)
    if (before) lastCells = changedCells(before, pos);
    aiLast = { d1, d2, moves: fmtTurn(bm.turn) };
    if (beforeSnap) {
      history = [
        ...history,
        {
          n: aiN,
          color: aiColor,
          dice: [d1, d2],
          notation: fmtTurn(bm.turn),
          win: bm.probs?.win ?? null,
          loss: 0, // engine plays its own best
          before: beforeSnap,
          ranked: null,
          isHuman: false,
        },
      ];
    }
    await continueAfter();
  }

  async function cubeTake() {
    if (phase !== 'cubeResponse') return;
    logCube(humanColor, 'тайк (взял)');
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
    logCube(humanColor, 'пас (сброс)');
    finishGame(aiColor, pendingPre, 'Вы пасанули');
  }
  async function cubeBeaver() {
    if (phase !== 'cubeResponse' || !canBeaver) return;
    phase = 'ai';
    try {
      pos = await engine.beaver();
      await updateWin();
      logCube(humanColor, `бивер → ×${pos.cube.value}`);
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
      aiLast = null;
      aiAnim = null;
      history = [];
      reviewIdx = null;
      gameResult = null;
      pos = await engine.reset();
      currentGameCrawford = crawfordForNext();
      pos = await engine.setCrawford(currentGameCrawford);
      legal = [];
      await updateWin();
      startOpeningRoll();
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

<div class="play" class:fit={history.length > 0}>
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
    <div class="winbar" class:collapsed={!barOpen}>
      {#if barOpen}
        <div class="track" title="Ваши шансы">
          <div class="fill" style="transform: scaleX({displayWin})"></div>
          <span class="label">вы {(displayWin * 100).toFixed(0)}%</span>
        </div>
      {:else}
        <span class="label muted">шкала шансов скрыта · вы {(displayWin * 100).toFixed(0)}%</span>
      {/if}
      <button
        type="button"
        class="bar-toggle"
        aria-label={barOpen ? 'Скрыть шкалу шансов' : 'Показать шкалу шансов'}
        onclick={() => (barOpen = !barOpen)}>{barOpen ? '▾' : '▸'}</button
      >
    </div>

    {#snippet boardCenter()}
      {#if phase === 'openingRoll' && !openRoll}
        <button class="act primary" onclick={doOpeningRoll}>🎲 Разыграть первый ход</button>
      {:else if phase === 'humanRoll'}
        {#if canHumanDouble}
          <button class="act ghost-act" onclick={humanDouble}>⬆ Удвоить (→{doubleTo})</button>
        {/if}
      {:else if phase === 'cubeResponse'}
        <div class="act-puck">
          <span class="cube-offer">🎲² движок удвоил → ×{pos?.cube.value}</span>
          <div class="act-row">
            <button class="act primary" onclick={cubeTake}>Тайк (взять)</button>
            <button class="act ghost-act" onclick={cubeDrop}>Пас (сбросить)</button>
            {#if canBeaver}<button class="act ghost-act" onclick={cubeBeaver}>Бивер</button>{/if}
          </div>
        </div>
      {:else if phase === 'humanMove' && completeSeq}
        <button class="act primary" onclick={confirmMove}>✓ Подтвердить ход</button>
      {/if}
    {/snippet}

    <Board
      position={displayPos ?? aiAnim ?? pos}
      orientation={humanColor}
      interactive={phase === 'humanMove' && !animating}
      sources={phase === 'humanMove' ? [...sourceCells] : []}
      dests={destCells}
      selected={selSource == null ? null : phys(humanColor, selSource)}
      {lastCells}
      {glide}
      canRoll={phase === 'humanRoll'}
      {bearOffCell}
      {boardStyle}
      center={boardCenter}
      onPointClick={handlePointClick}
      {onDrop}
      onRoll={humanRoll}
      onBearOff={bearOffSelected}
    />

    <div class="panel">
      <div class="pips" title="Пипсы — сумма очков до вывода всех шашек (меньше = ближе к победе)">
        <span class="pip-cap">пипсы</span>
        <span class="pip-val"><span class="pip-dot white"></span>{pos.pip[0]}</span>
        <span class="pip-val"><span class="pip-dot black"></span>{pos.pip[1]}</span>
        <span class="pip-turn">ход №{pos.turn_number}</span>
        <button
          type="button"
          class="auto-toggle"
          class:on={autoRoll}
          aria-pressed={autoRoll}
          onclick={() => (autoRoll = !autoRoll)}
          title="Бросать кости автоматически"
        >⚡ авто-бросок{autoRoll ? ' ✓' : ''}</button>
        <button
          type="button"
          class="auto-toggle"
          class:on={soundOn}
          aria-pressed={soundOn}
          onclick={() => (soundOn = !soundOn)}
          title="Звуковые эффекты"
        >{soundOn ? '🔊' : '🔇'} звук</button>
      </div>
      {#if hasCube}
        <div class="cube-state">🎲² Куб: <strong>{pos.cube.value}</strong> ({cubeOwnerDesc(pos)})</div>
      {/if}
      {#if currentGameCrawford}
        <div class="crawford">⚑ Кроуфорд — удвоение запрещено</div>
      {:else if hasCube && (humanOneAway || aiOneAway)}
        <div class="crawford post">
          ⚑ Пост-кроуфорд: {humanOneAway ? 'вам' : 'движку'} до матча 1 очко — куб бесполезен,
          {humanOneAway ? 'вы не удваиваете' : 'движок не удваивает'}
        </div>
      {/if}

      <div class="status-row">
        <p class="status">{status}</p>
        {#if (phase === 'humanRoll' || phase === 'humanMove') && !confirmResign}
          <button type="button" class="resign-btn" onclick={askResign} disabled={animating}>
            🏳 Сдаться
          </button>
        {/if}
      </div>
      {#if (phase === 'humanRoll' || phase === 'humanMove') && confirmResign}
        <div class="resign-confirm">
          <p class="rc-q">
            Сдать партию? Соперник получит <strong>+{resignPoints}</strong>
            ({canResignSingle ? 'оин' : 'марс'}).
          </p>
          <div class="moves">
            <button type="button" class="ghost danger" onclick={doResign} disabled={animating}>
              Да, сдаться
            </button>
            <button type="button" class="ghost" onclick={() => (confirmResign = false)}>
              Отмена
            </button>
          </div>
        </div>
      {/if}

      {#if aiLast}
        <div class="ailast">
          <span class="al-label">Соперник бросил</span>
          <span class="al-dice">
            {#each [aiLast.d1, aiLast.d2] as d}
              <span class="minidie">
                {#each Array(9) as _, idx}
                  <span class="mp" class:on={PIPS[d]?.includes(idx)}></span>
                {/each}
              </span>
            {/each}
          </span>
          <span class="al-moves">{aiLast.moves}</span>
        </div>
      {/if}

      {#if phase === 'openingRoll'}
        <div class="opening">
          {#if !openRoll}
            <p class="opening-hint">Кто выбросит больше — ходит первым. Кнопка — в центре доски.</p>
          {:else}
            <div class="open-dice">
              <span class="od">
                <span class="minidie">
                  {#each Array(9) as _, idx}
                    <span class="mp" class:on={PIPS[openRoll.you]?.includes(idx)}></span>
                  {/each}
                </span>
                <span class="od-label">вы</span>
              </span>
              <span class="od">
                <span class="minidie">
                  {#each Array(9) as _, idx}
                    <span class="mp" class:on={PIPS[openRoll.opp]?.includes(idx)}></span>
                  {/each}
                </span>
                <span class="od-label">соперник</span>
              </span>
            </div>
            <div class="open-res" class:win={openRoll.you > openRoll.opp}>
              {openRoll.you > openRoll.opp ? '✓ Ваш первый ход' : 'Первым ходит соперник'}
            </div>
          {/if}
        </div>
      {:else if phase === 'rolling'}
        <p class="thinking">бросаем…</p>
      {:else if phase === 'humanMove'}
        <div class="moves">
          {#if bearOffChain}
            <button class="primary" onclick={bearOffSelected}>Выкинуть шашку ↑</button>
            <span class="hint">или свайпните фишку вверх</span>
          {/if}
          {#if pending.length}
            <button class="ghost" onclick={undoPending}>↶ Отменить ({pending.length})</button>
          {/if}
        </div>
        <details class="movelist">
          <summary>или выбрать ход из списка ({legal.length})</summary>
          <div class="moves">
            {#each legal as t}
              <button onclick={() => previewTurn(t)}>{fmtTurn(t)}</button>
            {/each}
          </div>
        </details>

        {#if ranked}
          <div class="analysis" class:collapsed={!analysisOpen}>
            <button
              type="button"
              class="an-head"
              aria-expanded={analysisOpen}
              onclick={() => (analysisOpen = !analysisOpen)}
            >
              Лучшие ходы
              <span class="an-caret">{analysisOpen ? '▾' : '▸'}</span>
            </button>
            {#if analysisOpen}
            {#if cubeDec}
              <div class="cube {cubeDec.action}">
                Куб: <strong>{cubeDec.note}</strong>
                {#if cubeDec.recommend_beaver}<span class="beaver">(бивер!)</span>{/if}
              </div>
            {/if}
            <ol class="ranked">
              <li class="rhead">
                <span class="mv">ход</span>
                <span class="num" title="эквити — мера ценности с учётом марса (по ней сортировка)"
                  >эквити</span
                >
                <span class="num" title="вероятность победы (без учёта марса)">win</span>
                <span class="num" title="потеря эквити относительно лучшего хода">потеря</span>
              </li>
              {#each ranked.slice(0, 6) as r, i}
                <li class="stagger" class:best={i === 0} style="--i: {i}">
                  <span class="mv">{fmtTurn(r.turn)}</span>
                  <span class="num eq">{fmtEq(r.equity)}</span>
                  <span class="num">{(r.win * 100).toFixed(1)}%</span>
                  <span class="num loss"
                    >{i === 0 || r.equity_loss < 0.0005
                      ? '✓ лучший'
                      : `−${r.equity_loss.toFixed(3)}`}</span
                  >
                </li>
              {/each}
            </ol>
            <p class="ranknote">Сортировка по эквити (учитывает марс), а не по чистому win%.</p>
            {/if}
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

      {#if phase === 'over' && reviewStats}
        <div class="postgame">
          <div class="pg-head">
            Разбор партии
            {#if gameResult}
              <span
                class="pg-res"
                class:won={gameResult.winnerIsHuman === true}
                class:lost={gameResult.winnerIsHuman === false}
              >
                {gameResult.winnerIsHuman == null
                  ? 'ничья'
                  : gameResult.winnerIsHuman
                    ? `победа +${gameResult.points}`
                    : `поражение −${gameResult.points}`}
              </span>
            {/if}
          </div>
          {#if sparkPoints}
            <svg class="pg-chart" viewBox="0 0 100 28" preserveAspectRatio="none" aria-hidden="true">
              <line x1="0" y1="14" x2="100" y2="14" class="pg-mid" />
              <polyline points={sparkPoints} class="pg-line" />
            </svg>
            <div class="pg-axis">ваши шансы по ходам партии (50% — пунктир)</div>
          {/if}
          <div class="pg-stats">
            <span class="pg-stat good">✓ лучших: {reviewStats.best}/{reviewStats.n}</span>
            <span class="pg-stat warn">неточностей: {reviewStats.inacc}</span>
            <span class="pg-stat bad">ошибок: {reviewStats.blunder}</span>
          </div>
          <div class="pg-grade">
            точность {reviewStats.avg.toFixed(3)} экв./ход — {gradeLabel(reviewStats.avg)}
          </div>
          {#if worstMoves.length}
            <div class="pg-worst">
              <span class="pg-cap">главные потери (нажмите, чтобы разобрать):</span>
              {#each worstMoves as w (w.idx)}
                <button type="button" class="pg-row" onclick={() => openReview(w.idx)}>
                  <span class="pg-mv">
                    №{w.h.n}{w.h.dice ? ` · ${w.h.dice[0]}-${w.h.dice[1]}` : ''} · {w.h.notation}
                  </span>
                  <span class="pg-loss">−{w.h.loss!.toFixed(3)}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#if phase === 'humanRoll' || phase === 'humanMove'}
        <div class="savebar">
          <button class="ghost" onclick={doSave}>💾 Сохранить</button>
          <button class="ghost" onclick={doExport}>📤 Экспорт</button>
          {#if saveMsg}<span class="savemsg">{saveMsg}</span>{/if}
        </div>
        {#if exportStr}
          <textarea
            class="exportbox"
            readonly
            rows="2"
            onclick={(e) => (e.currentTarget as HTMLTextAreaElement).select()}
            >{exportStr}</textarea
          >
        {/if}
      {/if}

      {#if history.length}
        <div class="gamelog" class:open={logOpen}>
          <div class="gl-head">
            <button
              type="button"
              class="gl-summary"
              aria-expanded={logOpen}
              onclick={() => (logOpen = !logOpen)}
            >
              Лист партии ({history.length})
              <span class="gl-caret">{logOpen ? '▾' : '▸'}</span>
            </button>
            <button
              type="button"
              class="gl-save"
              onclick={saveTranscript}
              title="Скачать .txt и скопировать в буфер"
            >
              📄 Скачать лист
            </button>
            {#if sheetMsg}<span class="gl-msg">{sheetMsg}</span>{/if}
          </div>
          {#if logOpen}
          <ol class="log">
            {#each history as h, i}
              <li>
                <button
                  type="button"
                  class="logrow {sev(h.loss)}"
                  class:me={h.color === humanColor}
                  class:active={reviewIdx === i}
                  class:cube={h.cube}
                  onclick={() => toggleReview(i)}
                  title={h.cube ? 'Действие с кубом' : 'Показать другие варианты'}
                >
                  <span class="ln">{h.n}</span>
                  <span class="who">{h.color === humanColor ? 'вы' : 'движок'}</span>
                  <span class="dc">{h.dice ? `${h.dice[0]}-${h.dice[1]}` : ''}</span>
                  <span class="mv">{h.cube ? `🎲² ${h.notation}` : h.notation}</span>
                  <span class="ev">{evalLabel(h)}</span>
                  <span class="caret">{reviewIdx === i ? '▾' : '▸'}</span>
                </button>
                {#if reviewIdx === i}
                  <div class="review">
                    {#if h.cube}
                      <div class="review-head">Действие с кубом — запись для листа партии.</div>
                    {:else if h.ranked && h.ranked.length}
                      <div class="review-head">Другие варианты:</div>
                      <ol class="alts">
                        {#each h.ranked as r, ri}
                          <li>
                            <button
                              type="button"
                              class="alt"
                              class:best={ri === 0}
                              class:played={fmtTurn(r.turn) === h.notation}
                              disabled={!h.isHuman}
                              onclick={() => replayWith(i, r.turn)}
                              title={h.isHuman ? 'Сыграть этот вариант' : ''}
                            >
                              <span class="mv">{fmtTurn(r.turn)}</span>
                              <span class="num eq">{fmtEq(r.equity)}</span>
                              <span class="num">{(r.win * 100).toFixed(1)}%</span>
                              {#if fmtTurn(r.turn) === h.notation}<span class="num played-tag"
                                  >сыграно</span
                                >{/if}
                            </button>
                          </li>
                        {/each}
                      </ol>
                    {:else}
                      <div class="review-head">Ход движка — альтернативы не сохранены.</div>
                    {/if}
                    {#if h.isHuman}
                      <button class="ghost" onclick={() => replayFrom(i)}>↩ Переиграть этот ход</button>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ol>
          {/if}
        </div>
      {/if}

      {#if diceTotal > 0}
        <div class="dicefair" class:open={fairOpen}>
          <button
            type="button"
            class="fair-summary"
            aria-expanded={fairOpen}
            onclick={() => (fairOpen = !fairOpen)}
          >
            🎲 Кости — статистика ({diceTotal})
            <span class="gl-caret">{fairOpen ? '▾' : '▸'}</span>
          </button>
          {#if fairOpen}
            <div class="fairbody">
              <div class="dicetabs">
                <button type="button" class:active={diceTab === 'faces'} onclick={() => (diceTab = 'faces')}>Грани</button>
                <button type="button" class:active={diceTab === 'kush'} onclick={() => (diceTab = 'kush')}>Куши</button>
                <button type="button" class:active={diceTab === 'seq'} onclick={() => (diceTab = 'seq')}>Последовательность</button>
              </div>

              {#if diceTab === 'faces'}
                <p class="fairnote">
                  Кости — из криптогенератора (<code>crypto.getRandomValues</code>) с
                  rejection-sampling: каждая грань строго 1/6 (16.7%), источник
                  непредсказуем. Считаются все кости — ваши и движка.
                </p>
                <ol class="facebars">
                  {#each [1, 2, 3, 4, 5, 6] as f}
                    {@const cnt = diceStats[f]}
                    {@const pct = diceTotal ? (cnt / diceTotal) * 100 : 0}
                    <li>
                      <span class="face">{f}</span>
                      <span class="bar"
                        ><span class="barfill" style="width: {Math.min(100, (pct / 33) * 100)}%"
                        ></span></span
                      >
                      <span class="cnt">{cnt}</span>
                      <span class="pct">{pct.toFixed(1)}%</span>
                    </li>
                  {/each}
                </ol>
                <div class="fairfoot">
                  <span class="fairdev">макс. отклонение от 16.7%: {diceMaxDev.toFixed(1)}%</span>
                  <button type="button" class="ghost" onclick={resetDiceStats}>Сбросить</button>
                </div>
              {:else if diceTab === 'kush'}
                <p class="fairnote">
                  Сколько раз выпала каждая комбинация ({kushTotal} бросков, из них
                  дублей {doublesCount}). Ожидаемо: пара ≈ 5.6%, дубль ≈ 2.8%.
                </p>
                <ol class="kushgrid">
                  {#each KUSH_COMBOS as [a, b]}
                    {@const cnt = kushStats[`${a}-${b}`] ?? 0}
                    {@const pct = kushTotal ? (cnt / kushTotal) * 100 : 0}
                    <li class:dbl={a === b} title="{pct.toFixed(1)}%">
                      <span class="kk">{a}-{b}</span>
                      <span class="kc">{cnt}</span>
                    </li>
                  {/each}
                </ol>
                <div class="fairfoot">
                  <span class="fairdev">всего: {kushTotal} · дублей: {doublesCount}</span>
                  <button type="button" class="ghost" onclick={resetDiceStats}>Сбросить</button>
                </div>
              {:else}
                <p class="fairnote">Последовательность бросков этой сессии (новые сверху).</p>
                {#if rollSeq.length === 0}
                  <p class="seq-empty">Бросков пока нет.</p>
                {:else}
                  <ol class="rollseq">
                    {#each [...rollSeq].reverse() as r, i}
                      <li class="seqrow" class:me={r.who === 'you'} class:dbl={r.a === r.b}>
                        <span class="sn">{rollSeq.length - i}</span>
                        <span class="swho">{r.who === 'you' ? 'вы' : 'движок'}</span>
                        <span class="sdice">{r.a}-{r.b}{r.a === r.b ? ' ⚡' : ''}</span>
                        {#if r.opening}<span class="sopen">розыгрыш</span>{/if}
                      </li>
                    {/each}
                  </ol>
                {/if}
              {/if}
            </div>
          {/if}
        </div>
      {/if}
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
    color: var(--ink-wood);
    cursor: pointer;
    font: inherit;
  }
  .link:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .ghost {
    background: var(--surface);
    border: 1px solid var(--pill-border);
    border-radius: var(--radius-sm);
    padding: 0.45rem 0.85rem;
    transition:
      background var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out);
  }
  .ghost:hover {
    background: var(--surface-raised);
  }
  .ghost:active {
    transform: scale(0.97);
  }
  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .winbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 0.6rem;
  }
  .winbar .track {
    position: relative;
    flex: 1;
    height: 26px;
    background: linear-gradient(180deg, #3a3a3e, #161618);
    border: 1px solid #5a371d;
    border-radius: 13px;
    overflow: hidden;
    box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.5);
  }
  .winbar .fill {
    position: absolute;
    inset: 0;
    width: 100%;
    background: linear-gradient(180deg, #fffdf7, #e6ddc8);
    transform-origin: left center;
    will-change: transform;
  }
  .winbar .label {
    font-variant-numeric: var(--num-tabular);
  }
  .winbar .track .label {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font: 700 12px system-ui;
    color: #000;
    mix-blend-mode: difference;
    filter: invert(1);
  }
  .winbar .label.muted {
    flex: 1;
    color: var(--ink-faint);
    font: 600 0.8rem var(--font-ui);
  }
  .bar-toggle {
    flex: 0 0 auto;
    background: var(--surface);
    border: 1px solid var(--pill-border);
    border-radius: var(--radius-sm);
    color: var(--ink-wood);
    cursor: pointer;
    padding: 2px 9px;
    font-size: 12px;
    line-height: 1.7;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .bar-toggle:hover {
    background: var(--surface-raised);
  }
  /* best-moves overview: collapsible header */
  .an-head {
    display: flex;
    align-items: center;
    width: 100%;
    gap: 0.4rem;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0 0 0.3rem;
    font: var(--fw-semi) 0.92rem var(--font-ui);
    color: #6b4423;
    text-align: left;
  }
  .an-head:hover {
    color: #8a5a2f;
  }
  .an-caret {
    margin-left: auto;
    color: #bba784;
  }
  .analysis.collapsed {
    padding-bottom: 0.5rem;
  }
  .panel {
    margin-top: 1rem;
  }
  /* pip counts right under the board (was buried at the very bottom before) */
  .pips {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.4rem 0.7rem;
    margin: 0.1rem 0 0.55rem;
    padding: 0.32rem 0.6rem;
    background: var(--surface-sunken);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    font: var(--fw-semi) 0.92rem var(--font-mono);
    font-variant-numeric: var(--num-tabular);
    color: var(--ink);
  }
  .pip-cap {
    color: var(--ink-faint);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 0.72rem;
  }
  .pip-val {
    display: inline-flex;
    align-items: center;
    gap: 0.32rem;
  }
  .pip-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    display: inline-block;
  }
  .pip-dot.white {
    background: var(--checker-grad-white);
    border: 1px solid var(--chip-white-edge, #9a8f78);
  }
  .pip-dot.black {
    background: var(--checker-grad-black);
    border: 1px solid var(--chip-black-edge, #000);
  }
  .pip-turn {
    margin-left: auto;
    color: var(--ink-faint);
    font-size: 0.8rem;
    font-weight: var(--fw-medium);
  }
  .auto-toggle {
    flex: 0 0 auto;
    cursor: pointer;
    padding: 0.2rem 0.55rem;
    border: 1px solid var(--pill-border);
    border-radius: var(--radius-pill);
    background: var(--surface);
    color: var(--ink-muted);
    font: var(--fw-semi) 0.72rem var(--font-ui);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }
  .auto-toggle:hover {
    background: var(--surface-raised);
  }
  .auto-toggle.on {
    background: linear-gradient(180deg, #ffe9a8, #f3cf6f);
    border-color: #d9a93f;
    color: #5a3d12;
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
  .crawford.post {
    color: var(--ink-muted);
    font-weight: var(--fw-medium);
    font-size: 0.86rem;
  }
  .status-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem 0.8rem;
  }
  .status-row .status {
    flex: 1;
    min-width: 0;
  }
  .status {
    font-weight: 600;
    min-height: 1.4em;
  }
  .ailast {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    margin: -0.2rem 0 0.6rem;
    padding: 0.4rem 0.7rem;
    background: #f1ece3;
    border-left: 3px solid #6b4423;
    border-radius: 4px;
    font: 0.9rem ui-monospace, monospace;
    color: #4a3320;
  }
  .al-label {
    font-weight: 700;
    color: #6b4423;
  }
  .al-dice {
    display: inline-flex;
    gap: 4px;
  }
  .minidie {
    width: 22px;
    height: 22px;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(3, 1fr);
    gap: 1px;
    padding: 3px;
    box-sizing: border-box;
    background: linear-gradient(145deg, #fffdf7, #e6ddc8);
    border-radius: 5px;
    box-shadow:
      0 1px 2px rgba(0, 0, 0, 0.35),
      inset 0 1px 1px rgba(255, 255, 255, 0.6);
  }
  .mp {
    border-radius: 50%;
    align-self: center;
    justify-self: center;
    width: 4px;
    height: 4px;
    background: transparent;
  }
  .mp.on {
    background: #2a1a0c;
  }
  .al-moves {
    margin-left: auto;
    color: #4a3320;
  }
  .hint {
    font-size: 0.8rem;
    color: #a08a6a;
    font-style: italic;
  }
  .opening {
    padding: 0.4rem 0;
  }
  .opening-hint {
    margin: 0 0 0.6rem;
    color: var(--ink-muted);
    font-size: 0.9rem;
  }
  .open-dice {
    display: flex;
    gap: 1.4rem;
    align-items: center;
    margin-bottom: 0.6rem;
  }
  .od {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--ink-muted);
    font-size: 0.9rem;
  }
  .od .minidie {
    width: 30px;
    height: 30px;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(3, 1fr);
    gap: 1px;
    padding: 4px;
    box-sizing: border-box;
    background: linear-gradient(145deg, #fffdf7, #e6ddc8);
    border-radius: 6px;
    box-shadow: var(--shadow-1), inset 0 1px 1px rgba(255, 255, 255, 0.6);
    animation: reveal var(--dur-quick) var(--ease-settle);
  }
  .od .mp {
    align-self: center;
    justify-self: center;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: transparent;
  }
  .od .mp.on {
    background: #2a1a0c;
  }
  .open-res {
    font-weight: 700;
    color: var(--ink-muted);
  }
  .open-res.win {
    color: var(--ok);
  }
  .savebar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.7rem;
  }
  .savemsg {
    color: #2a6;
    font-size: 0.85rem;
    font-weight: 600;
  }
  .exportbox {
    width: 100%;
    box-sizing: border-box;
    margin-top: 0.4rem;
    font: 0.78rem ui-monospace, monospace;
    border: 1px solid #d8c4a6;
    border-radius: 8px;
    padding: 0.4rem;
    resize: vertical;
    color: #4a3320;
  }
  .resign-btn {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 0.32rem;
    cursor: pointer;
    color: var(--ink-muted);
    background: linear-gradient(180deg, var(--surface), var(--surface-raised));
    border: 1px solid var(--pill-border);
    border-radius: var(--radius-pill);
    padding: 0.34rem 0.85rem;
    font: var(--fw-semi) 0.82rem var(--font-ui);
    box-shadow: var(--shadow-1);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out);
  }
  .resign-btn:hover {
    background: linear-gradient(180deg, var(--danger-soft), #f3dcd6);
    color: var(--danger);
    border-color: #d8a8a0;
    box-shadow: var(--shadow-2);
    transform: translateY(-1px);
  }
  .resign-btn:active {
    transform: scale(0.97);
  }
  .resign-btn:disabled {
    opacity: 0.5;
    cursor: default;
    transform: none;
    box-shadow: none;
  }
  .resign-confirm {
    margin-top: 0.2rem;
    padding: 0.6rem 0.8rem;
    background: var(--danger-soft);
    border: 1px solid #e0b8b0;
    border-radius: var(--radius-md);
    animation: reveal var(--dur-base) var(--ease-decelerate);
  }
  .rc-q {
    margin: 0 0 0.55rem;
    color: #7a4a3a;
    font-size: 0.92rem;
  }
  .resign-confirm .moves {
    margin-top: 0;
  }
  .ghost.danger {
    border-color: #c79a9a;
    color: #b22;
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
    padding: 0.55rem 1.15rem;
    border: none;
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, var(--btn-primary-hi), var(--btn-primary));
    color: #fff;
    cursor: pointer;
    font-family: var(--font-ui);
    font-weight: var(--fw-semi);
    box-shadow: var(--shadow-2), inset 0 1px 0 rgba(255, 255, 255, 0.18);
    transition:
      transform var(--dur-instant) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out),
      filter var(--dur-fast) var(--ease-out);
  }
  button.primary:hover {
    filter: brightness(1.06);
    box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.22);
  }
  button.primary:active {
    transform: scale(0.97);
  }
  /* ---- on-board action buttons (centred over the felt) ---- */
  .act {
    font-family: var(--font-ui);
    font-weight: var(--fw-semi);
    font-size: 0.95rem;
    padding: 0.5rem 1.05rem;
    border-radius: var(--radius-pill);
    cursor: pointer;
    white-space: nowrap;
    border: 1px solid rgba(255, 255, 255, 0.16);
    box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.16);
    transition:
      transform var(--dur-instant) var(--ease-out),
      filter var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out);
    animation: act-in var(--dur-base) var(--ease-settle) backwards;
  }
  .act.primary {
    background: linear-gradient(180deg, var(--btn-primary-hi), var(--btn-primary));
    color: #fff;
  }
  .act.ghost-act {
    background: rgba(18, 28, 22, 0.82);
    color: var(--felt-label);
    -webkit-backdrop-filter: blur(2px);
    backdrop-filter: blur(2px);
  }
  .act:hover {
    filter: brightness(1.07);
    transform: translateY(-1px);
  }
  .act:active {
    transform: scale(0.96);
  }
  .act-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    justify-content: center;
  }
  .act-puck {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    padding: 0.55rem 0.7rem;
    background: rgba(16, 26, 20, 0.74);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-3);
    -webkit-backdrop-filter: blur(3px);
    backdrop-filter: blur(3px);
    animation: act-in var(--dur-base) var(--ease-settle) backwards;
  }
  .cube-offer {
    color: #ffe7a8;
    font: var(--fw-semi) 0.86rem var(--font-ui);
    letter-spacing: 0.01em;
    text-align: center;
  }
  @keyframes act-in {
    from {
      opacity: 0;
      transform: translateY(9px) scale(0.9);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .act,
    .act-puck {
      animation: none;
    }
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
    font: 0.88rem ui-monospace, monospace;
  }
  .ranked li {
    display: flex;
    gap: 0.6rem;
    padding: 0.18rem 0;
    border-bottom: 1px dotted #e0d4bf;
  }
  .ranked li.rhead {
    color: #a08a6a;
    font-size: 0.74rem;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    border-bottom: 1px solid #d9c7a8;
  }
  .ranked li.best {
    font-weight: 700;
    color: #2a6;
  }
  .ranked .mv {
    flex: 1;
    min-width: 0;
  }
  .ranked .num {
    width: 4.6rem;
    text-align: right;
    color: #777;
  }
  .ranked .num.eq {
    color: #3a2e1c;
    font-weight: 600;
  }
  .ranked li.best .num {
    color: #2a6;
  }
  .ranknote {
    margin: 0.35rem 0 0;
    font-size: 0.74rem;
    color: #a08a6a;
  }
  /* post-game review panel */
  .postgame {
    margin-top: 0.9rem;
    border: 1px solid #e2d3bb;
    border-radius: 8px;
    background: #fcf8f1;
    padding: 0.6rem 0.7rem 0.65rem;
  }
  .pg-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.6rem;
    font: var(--fw-semi) 0.95rem var(--font-ui);
    color: #6b4423;
    margin-bottom: 0.45rem;
  }
  .pg-res {
    font: var(--fw-semi) 0.85rem var(--font-ui);
    color: #8a6a48;
  }
  .pg-res.won {
    color: #2e7d32;
  }
  .pg-res.lost {
    color: #b0413e;
  }
  .pg-chart {
    display: block;
    width: 100%;
    height: 56px;
    border: 1px solid #ecdfc9;
    border-radius: 6px;
    background: #fffdf8;
  }
  .pg-mid {
    stroke: #d8c7a8;
    stroke-width: 0.5;
    stroke-dasharray: 2 2;
  }
  .pg-line {
    fill: none;
    stroke: #2e7d32;
    stroke-width: 1.1;
    stroke-linejoin: round;
    stroke-linecap: round;
    vector-effect: non-scaling-stroke;
  }
  .pg-axis {
    font: 400 0.7rem var(--font-ui);
    color: #a08a6c;
    margin: 0.15rem 0 0.45rem;
  }
  .pg-stats {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem 0.9rem;
    font: 400 0.85rem var(--font-ui);
    color: #5a4632;
  }
  .pg-stat.good {
    color: #2e7d32;
  }
  .pg-stat.warn {
    color: #b07d2a;
  }
  .pg-stat.bad {
    color: #b0413e;
  }
  .pg-grade {
    margin-top: 0.35rem;
    font: var(--fw-semi) 0.85rem var(--font-ui);
    color: #6b4423;
  }
  .pg-worst {
    margin-top: 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .pg-cap {
    font: 400 0.78rem var(--font-ui);
    color: #a08a6c;
  }
  .pg-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.6rem;
    width: 100%;
    text-align: left;
    padding: 0.3rem 0.5rem;
    border: 1px solid #ecdfc9;
    border-radius: 6px;
    background: #fffdf8;
    cursor: pointer;
    font: 400 0.85rem var(--font-mono, monospace);
    color: #5a4632;
  }
  .pg-row:hover {
    background: #f6efe2;
    border-color: #d8c7a8;
  }
  .pg-loss {
    font: var(--fw-semi) 0.85rem var(--font-mono, monospace);
    color: #b0413e;
    white-space: nowrap;
  }
  .gamelog {
    margin-top: 0.9rem;
    border: 1px solid #e2d3bb;
    border-radius: 8px;
    background: #fcf8f1;
  }
  /* dice-fairness panel */
  .dicefair {
    margin-top: 0.7rem;
    border: 1px solid #e2d3bb;
    border-radius: 8px;
    background: #fcf8f1;
  }
  .fair-summary {
    display: block;
    width: 100%;
    text-align: left;
    cursor: pointer;
    padding: 0.5rem 0.7rem;
    font: var(--fw-semi) 0.95rem var(--font-ui);
    color: #6b4423;
    background: none;
    border: none;
    border-radius: 8px;
  }
  .fair-summary:hover {
    background: #f6efe2;
  }
  .fairbody {
    padding: 0 0.7rem 0.6rem;
  }
  .fairnote {
    margin: 0 0 0.55rem;
    font-size: 0.78rem;
    color: var(--ink-muted);
    line-height: 1.45;
  }
  .fairnote code {
    font: 0.74rem var(--font-mono);
    color: #6b4423;
  }
  .facebars {
    list-style: none;
    margin: 0 0 0.5rem;
    padding: 0;
    font: 0.84rem var(--font-mono);
    font-variant-numeric: var(--num-tabular);
  }
  .facebars li {
    display: grid;
    grid-template-columns: 1.2rem 1fr 2.6rem 3rem;
    gap: 0.5rem;
    align-items: center;
    padding: 0.13rem 0;
  }
  .facebars .face {
    font-weight: 700;
    color: #6b4423;
    text-align: center;
  }
  .facebars .bar {
    height: 10px;
    background: var(--surface-sunken);
    border-radius: 5px;
    overflow: hidden;
  }
  .facebars .barfill {
    display: block;
    height: 100%;
    background: linear-gradient(90deg, var(--wood-hi), var(--wood));
    border-radius: 5px;
    transition: width var(--dur-base) var(--ease-out);
  }
  .facebars .cnt {
    text-align: right;
    color: #888;
  }
  .facebars .pct {
    text-align: right;
    color: #3a2e1c;
    font-weight: 600;
  }
  .fairfoot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .fairdev {
    font-size: 0.76rem;
    color: var(--ink-faint);
  }
  /* dice-stats tabs */
  .dicetabs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin-bottom: 0.55rem;
  }
  .dicetabs button {
    cursor: pointer;
    padding: 0.28rem 0.65rem;
    border: 1px solid var(--pill-border);
    border-radius: var(--radius-pill);
    background: var(--surface);
    color: var(--ink-muted);
    font: var(--fw-semi) 0.78rem var(--font-ui);
    transition: background var(--dur-fast) var(--ease-out);
  }
  .dicetabs button:hover {
    background: var(--surface-raised);
  }
  .dicetabs button.active {
    background: linear-gradient(180deg, var(--btn-primary-hi), var(--btn-primary));
    color: #fff;
    border-color: var(--btn-primary);
  }
  /* куши grid */
  .kushgrid {
    list-style: none;
    margin: 0 0 0.5rem;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(3.6rem, 1fr));
    gap: 0.3rem;
    font: 0.82rem var(--font-mono);
    font-variant-numeric: var(--num-tabular);
  }
  .kushgrid li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.4rem;
    white-space: nowrap;
    padding: 0.22rem 0.45rem;
    background: var(--surface-sunken);
    border-radius: var(--radius-sm);
  }
  .kushgrid li.dbl {
    background: #f3e6c8;
    box-shadow: inset 0 0 0 1px #e0c98a;
  }
  .kushgrid .kk {
    font-weight: 700;
    color: #6b4423;
  }
  .kushgrid .kc {
    color: #3a2e1c;
    font-weight: 600;
  }
  /* roll sequence */
  .seq-empty {
    color: var(--ink-faint);
    font-size: 0.82rem;
  }
  .rollseq {
    list-style: none;
    margin: 0;
    padding: 0 0.2rem 0.2rem 0;
    max-height: clamp(180px, 40dvh, 320px);
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    overscroll-behavior: contain;
    font: 0.84rem var(--font-mono);
    font-variant-numeric: var(--num-tabular);
  }
  .seqrow {
    display: grid;
    grid-template-columns: 2.2rem 3.4rem 1fr auto;
    gap: 0.5rem;
    align-items: center;
    padding: 0.22rem 0.3rem;
    border-bottom: 1px dotted #e8dcc6;
  }
  .seqrow .sn {
    color: #aaa;
    text-align: right;
  }
  .seqrow .swho {
    color: #6b4423;
  }
  .seqrow.me .swho {
    font-weight: 700;
  }
  .seqrow .sdice {
    color: #2c2c2c;
  }
  .seqrow.dbl .sdice {
    color: #9a6a16;
    font-weight: 700;
  }
  .seqrow .sopen {
    color: #a08a6a;
    font-size: 0.72rem;
    font-style: italic;
  }
  .gl-head {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding-right: 0.4rem;
  }
  .gl-summary {
    flex: 1;
    min-width: 0;
    text-align: left;
    cursor: pointer;
    padding: 0.5rem 0.7rem;
    font: 600 1rem var(--font-ui);
    color: #6b4423;
    background: none;
    border: none;
    border-radius: 8px;
  }
  .gl-summary:hover {
    background: #f6efe2;
  }
  .gl-save {
    flex: 0 0 auto;
    cursor: pointer;
    padding: 0.42rem 0.7rem;
    font: 600 0.84rem var(--font-ui);
    color: var(--ink-wood);
    background: var(--surface);
    border: 1px solid var(--pill-border);
    border-radius: var(--radius-sm);
    white-space: nowrap;
    transition:
      background var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out);
  }
  .gl-save:hover {
    background: var(--surface-raised);
  }
  .gl-save:active {
    transform: scale(0.97);
  }
  .gl-msg {
    flex: 0 0 auto;
    color: #2a6;
    font-size: 0.82rem;
    font-weight: 600;
  }
  .gl-caret {
    float: right;
    color: #bba784;
  }
  .log {
    list-style: none;
    margin: 0;
    padding: 0 0.4rem 0.4rem;
    max-height: 220px;
    overflow-y: auto;
    font: 0.86rem ui-monospace, monospace;
  }
  .logrow {
    display: grid;
    grid-template-columns: 1.6rem 3.2rem 2.4rem 1fr auto 0.9rem;
    gap: 0.5rem;
    align-items: center;
    width: 100%;
    padding: 0.28rem 0.3rem;
    border: none;
    border-bottom: 1px dotted #e8dcc6;
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: 5px;
  }
  .logrow:hover {
    background: #f6efe2;
  }
  .logrow.active {
    background: #f3e6d2;
  }
  .logrow .caret {
    color: #bba784;
    text-align: right;
  }
  .logrow .ln {
    color: #aaa;
    text-align: right;
  }
  .logrow .who {
    color: #6b4423;
  }
  .logrow.me .who {
    font-weight: 700;
  }
  .logrow .dc {
    color: #888;
  }
  .logrow .mv {
    color: #2c2c2c;
  }
  .logrow .ev {
    text-align: right;
    color: #999;
    white-space: nowrap;
  }
  .logrow.ok .ev {
    color: #2a6;
  }
  .logrow.inacc .ev {
    color: #b80;
  }
  .logrow.blunder .ev {
    color: #c22;
    font-weight: 700;
  }
  .logrow.cube .mv {
    color: #8a5a16;
    font-weight: 600;
  }
  .logrow.cube .who {
    color: #8a5a16;
  }
  .review {
    padding: 0.5rem 0.4rem 0.6rem 1.6rem;
    border-bottom: 1px dotted #e8dcc6;
    background: #fcf8f1;
  }
  .review-head {
    font: 600 0.78rem system-ui;
    color: #a08a6a;
    margin-bottom: 0.35rem;
  }
  .alts {
    list-style: none;
    margin: 0 0 0.5rem;
    padding: 0;
  }
  .alt {
    display: grid;
    grid-template-columns: 1fr 4.4rem 3.6rem auto;
    gap: 0.5rem;
    align-items: center;
    width: 100%;
    padding: 0.2rem 0.3rem;
    border: none;
    background: none;
    font: 0.84rem ui-monospace, monospace;
    text-align: left;
    cursor: pointer;
    border-radius: 5px;
  }
  .alt:hover:not(:disabled) {
    background: #f1e6d2;
  }
  .alt:disabled {
    cursor: default;
  }
  .alt .num {
    text-align: right;
    color: #888;
  }
  .alt .num.eq {
    color: #3a2e1c;
    font-weight: 600;
  }
  .alt.best {
    color: #2a6;
    font-weight: 700;
  }
  .alt.best .num {
    color: #2a6;
  }
  .alt.played {
    background: #efe3cd;
  }
  .alt .played-tag {
    color: #6b4423;
    font-style: italic;
  }
  .meta {
    margin-top: 0.8rem;
    color: var(--ink-faint);
    font-size: 0.85rem;
    font-variant-numeric: var(--num-tabular);
  }
  .score {
    font-variant-numeric: var(--num-tabular);
  }
  .ranked,
  .log {
    font-variant-numeric: var(--num-tabular);
  }
  .analysis,
  .ailast {
    animation: reveal var(--dur-base) var(--ease-decelerate);
  }
  .ranked li.stagger {
    animation: reveal var(--dur-base) var(--ease-decelerate) backwards;
    animation-delay: calc(var(--i, 0) * 38ms);
  }
  @keyframes reveal {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .analysis,
    .ailast,
    .ranked li.stagger {
      animation: none;
    }
  }
  @media (max-width: 600px) {
    .play {
      max-width: 100%;
    }
    /* The sparse opening/roll screen keeps a tall board to fill the view; once a
       game is in progress (.fit) the board is a bit shorter so the controls and a
       ROOMY, scrollable game log / move-review sit comfortably below. The page
       scrolls as needed — the review reads far better with real room than crammed
       into a tiny internal-scroll box. */
    .play.fit :global(.board) {
      --row-h: clamp(
        calc(var(--pt) * 3.0),
        calc((var(--pt) * 4.4 + (100dvh - 24rem) / 2) / 2),
        27dvh
      );
    }
    header {
      gap: 0.6rem;
      margin-bottom: 0.3rem;
    }
    .winbar {
      margin-bottom: 0.45rem;
    }
    .panel {
      margin-top: 0.55rem;
    }
    .status {
      font-size: 1rem;
      margin: 0.2rem 0;
    }
    /* compact the secondary controls so the active play area + a scrollable game
       log fit the screen without the page itself scrolling */
    .ailast {
      margin: 0 0 0.4rem;
      padding: 0.3rem 0.6rem;
      font-size: 0.82rem;
      gap: 0.4rem;
    }
    .savebar,
    .resign,
    .meta {
      margin-top: 0.4rem;
    }
    .meta {
      font-size: 0.8rem;
    }
    /* the full-move list is redundant with click-to-move + «Оценка» on a phone;
       hide it to keep the screen fitting without scroll */
    .movelist {
      display: none;
    }
    /* roomy, scrollable game log so the move-review (alternatives + replay) reads
       comfortably — the list scrolls inside this box; the page scrolls to reveal
       it (this is the pre-0.9.6 behaviour the cramped fit-box regressed) */
    .log {
      max-height: clamp(200px, 46dvh, 340px);
      -webkit-overflow-scrolling: touch;
      overscroll-behavior: contain;
    }
    /* larger, comfortable touch targets on phones */
    .moves button,
    .ghost,
    .movelist summary,
    .resign-btn {
      padding: 0.55rem 0.85rem;
      min-height: 42px;
      display: inline-flex;
      align-items: center;
    }
    button.primary {
      padding: 0.6rem 1.05rem;
      min-height: 44px;
    }
    .logrow,
    .alt {
      min-height: 36px;
    }
    /* roomy, tappable on-board action buttons on phones */
    .act {
      min-height: 44px;
      display: inline-flex;
      align-items: center;
      font-size: 0.92rem;
      padding: 0.55rem 1rem;
    }
    .gl-save {
      min-height: 40px;
    }
  }
</style>
