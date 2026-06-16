<script lang="ts">
  import { onDestroy, type Snippet } from 'svelte';
  import type { PositionDto } from '../../engine/types';
  import { buildSlotToCell, phys, posOfPhys, type PlayerColor } from './coords';

  let {
    position,
    orientation = 'white',
    interactive = false,
    sources = [],
    dests = [],
    selected = null,
    lastCells = [],
    showHints = true,
    glide = null,
    canRoll = false,
    showCube = false,
    bearOffCell = null,
    boardStyle = '',
    center,
    onPointClick,
    onDragStart,
    onDrop,
    onRoll,
    onBearOff,
  }: {
    position: PositionDto;
    orientation?: PlayerColor;
    interactive?: boolean;
    sources?: number[];
    dests?: number[];
    selected?: number | null;
    lastCells?: number[]; // physical cells touched by the most recent move (pulse)
    showHints?: boolean; // draw the move hints (movable checkers + destination dots); the sets still drive interaction when off
    glide?: { from: number; to: number; color: PlayerColor; bearOff?: boolean } | null; // fly a piece (point→point, or point→tray on bear-off)
    canRoll?: boolean; // show a clickable roll prompt in the centre
    showCube?: boolean; // render the doubling cube on the board (cube variants only)
    bearOffCell?: number | null; // the selected checker's cell, if it can bear off
    boardStyle?: string; // theme CSS-variable overrides applied to the board
    center?: Snippet; // action buttons rendered centred ON the board (confirm/double/cube)
    onPointClick?: (cell: number) => void;
    onDragStart?: (cell: number) => void; // drag begun on a source — force-select it
    onDrop?: (cell: number) => void; // drag release over a destination
    onRoll?: () => void; // tap the centre of the board to roll
    onBearOff?: () => void; // swipe a checker up (out of the board) to bear it off
  } = $props();

  interface Cell {
    color: PlayerColor | null;
    count: number;
  }

  const occupancy = $derived.by<Cell[]>(() => {
    const occ: Cell[] = Array.from({ length: 24 }, () => ({ color: null, count: 0 }));
    for (const pt of position.white) occ[phys('white', pt.pos)] = { color: 'white', count: pt.count };
    for (const pt of position.black) occ[phys('black', pt.pos)] = { color: 'black', count: pt.count };
    return occ;
  });
  const slotToCell = $derived(buildSlotToCell(orientation));
  // Doubling cube placement: centred on the bar while nobody owns it; once a player
  // takes a double it sits on that player's side (you at the bottom, opponent up top).
  const cubeSide = $derived<'center' | 'top' | 'bottom'>(
    position.cube.owner == null ? 'center' : position.cube.owner === orientation ? 'bottom' : 'top',
  );
  const sourceSet = $derived(new Set(sources));
  const destSet = $derived(new Set(dests));
  const lastSet = $derived(new Set(lastCells));

  const topSlots = [12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23];
  const bottomSlots = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];

  // Dot layout per die value (indices into a 3×3 grid).
  const PIPS: Record<number, number[]> = {
    1: [4],
    2: [0, 8],
    3: [0, 4, 8],
    4: [0, 2, 6, 8],
    5: [0, 2, 4, 6, 8],
    6: [0, 2, 3, 5, 6, 8],
  };

  function discs(count: number): number {
    return Math.min(count, 5);
  }
  function click(cell: number) {
    // a drag gesture already did its work (select / drop) — the browser still
    // fires a trailing `click` after pointerup, which must not toggle anything
    if (squelchClick) return;
    if (interactive) onPointClick?.(cell);
  }

  // ---- drag (select source on press-move, apply on release over a destination) ----
  const DRAG_SLOP = 8; // px of movement before a press becomes a drag (≈ touch slop)
  const SWIPE_UP = 40; // px of upward drag that triggers a bear-off
  let squelchClick = false;
  let downCell: number | null = null;
  let downXY = { x: 0, y: 0 };
  let dragging = $state(false);
  let dragColor = $state<PlayerColor | null>(null);
  let dragXY = $state({ x: 0, y: 0 });

  function cellFromEl(el: Element | null): number | null {
    const pt = el?.closest('.point') as HTMLElement | null;
    if (!pt?.dataset.cell) return null;
    return Number(pt.dataset.cell);
  }

  function onBoardPointerDown(e: PointerEvent) {
    if (!interactive) return;
    const cell = cellFromEl(e.target as Element);
    if (cell == null || !sourceSet.has(cell)) return;
    downCell = cell;
    downXY = { x: e.clientX, y: e.clientY };
    window.addEventListener('pointermove', onWinMove);
    window.addEventListener('pointerup', onWinUp);
  }
  function onWinMove(e: PointerEvent) {
    if (downCell == null) return;
    const dx = e.clientX - downXY.x;
    const dy = e.clientY - downXY.y;
    if (!dragging && dx * dx + dy * dy > DRAG_SLOP * DRAG_SLOP) {
      dragging = true;
      dragColor = occupancy[downCell].color;
      // force-select the dragged checker (onPointClick would instead PLAY onto
      // this cell when it is a reachable destination of the previous selection)
      if (selected !== downCell) (onDragStart ?? onPointClick)?.(downCell);
    }
    if (dragging) dragXY = { x: e.clientX, y: e.clientY };
  }
  function onWinUp(e: PointerEvent) {
    window.removeEventListener('pointermove', onWinMove);
    window.removeEventListener('pointerup', onWinUp);
    if (dragging) {
      const t = cellFromEl(document.elementFromPoint(e.clientX, e.clientY));
      if (t != null && destSet.has(t)) {
        onDrop?.(t);
      } else if (downCell != null && downCell === bearOffCell && e.clientY - downXY.y < -SWIPE_UP) {
        // dragged the checker upward, out of the board → bear it off
        onBearOff?.();
      }
      // the trailing click (fires synchronously after pointerup, when the release
      // stayed on the pressed button) must not undo the selection / replay a move
      squelchClick = true;
      setTimeout(() => (squelchClick = false), 0);
    }
    dragging = false;
    dragColor = null;
    downCell = null;
  }
  onDestroy(() => {
    window.removeEventListener('pointermove', onWinMove);
    window.removeEventListener('pointerup', onWinUp);
  });

  // ---- glide animation: a piece flying from one point's centre to another ----
  // Compositor-only: the wrapper holds the FROM centre (left/top, static) and we
  // animate the inner disc's TRANSFORM (pick-up → travel → settle) via WAAPI.
  let gx = $state(0);
  let gy = $state(0);
  let gShown = $state(false);
  let glideEl = $state<HTMLDivElement>();
  $effect(() => {
    if (!glide) {
      gShown = false;
      return;
    }
    const f = document.querySelector(`.board [data-cell="${glide.from}"]`)?.getBoundingClientRect();
    const t = glide.bearOff
      ? document.querySelector(`.board .off-chip.${glide.color}`)?.getBoundingClientRect()
      : document.querySelector(`.board [data-cell="${glide.to}"]`)?.getBoundingClientRect();
    if (!f || !t) {
      gShown = false;
      return;
    }
    gx = f.x + f.width / 2;
    gy = f.y + f.height / 2;
    gShown = true;
    const dx = t.x + t.width / 2 - gx;
    const dy = t.y + t.height / 2 - gy;
    const bearOff = !!glide.bearOff;
    const reduceMotion =
      typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
    requestAnimationFrame(() => {
      const el = glideEl;
      if (!el) return;
      if (reduceMotion) {
        el.style.transform = bearOff ? 'scale(0)' : `translate(${dx}px, ${dy}px)`;
        return;
      }
      el.style.willChange = 'transform';
      // Bear-off: lift up and arc out into the tray, shrinking + fading on the way.
      const lift = Math.min(40, Math.abs(dy) * 0.4 + 18);
      const keyframes = bearOff
        ? [
            { transform: 'translate(0,0) scale(1)', opacity: 1 },
            {
              transform: `translate(${dx * 0.45}px, ${dy * 0.4 - lift}px) scale(1.08)`,
              opacity: 1,
              offset: 0.4,
              easing: 'cubic-bezier(.2,0,.2,1)',
            },
            { transform: `translate(${dx}px, ${dy}px) scale(0.42)`, opacity: 0, offset: 1 },
          ]
        : [
            { transform: 'translate(0,0) scale(1)' },
            { transform: 'translate(0,-4px) scale(1.1)', offset: 0.16, easing: 'cubic-bezier(.05,.7,.1,1)' },
            {
              transform: `translate(${dx * 1.02}px, ${dy * 1.02}px) scale(1.05)`,
              offset: 0.86,
              easing: 'cubic-bezier(.34,1.16,.64,1)',
            },
            { transform: `translate(${dx}px, ${dy}px) scaleX(1.06) scaleY(0.92)`, offset: 0.94 },
            { transform: `translate(${dx}px, ${dy}px) scale(1)`, offset: 1 },
          ];
      const anim = el.animate(keyframes, {
        duration: bearOff ? 440 : 280,
        fill: 'forwards',
        easing: bearOff ? 'cubic-bezier(.4,0,.7,1)' : 'cubic-bezier(.05,.7,.1,1)',
      });
      anim.finished
        .then(() => {
          if (el) el.style.willChange = '';
        })
        .catch(() => {});
    });
  });
</script>

<div
  class="board"
  class:interactive
  role="application"
  aria-label="Игровая доска"
  style={boardStyle}
  onpointerdown={onBoardPointerDown}
>
  <div class="table">
  <div class="row top">
    {#each topSlots as slot, i}
      {#if i === 6}<div class="bar"></div>{/if}
      {@const cell = slotToCell[slot]}
      {@const c = occupancy[cell]}
      <button
        type="button"
        class="point up"
        class:dark={slot % 2 === 0}
        class:source={showHints && sourceSet.has(cell)}
        class:dest={destSet.has(cell)}
        class:selected={selected === cell}
        class:last={lastSet.has(cell)}
        data-cell={cell}
        onclick={() => click(cell)}
      >
        <span class="tri"></span>
        <span class="ptnum up">{posOfPhys(orientation, cell)}</span>
        {#if showHints && destSet.has(cell)}<span class="dot"></span>{/if}
        <span class="stack down">
          {#if c.color}
            {#each Array(discs(c.count)) as _}
              <span class="checker {c.color}"></span>
            {/each}
            {#if c.count > 5}<span class="badge">{c.count}</span>{/if}
          {/if}
        </span>
      </button>
    {/each}
  </div>

  <div class="row bottom">
    {#each bottomSlots as slot, i}
      {#if i === 6}<div class="bar"></div>{/if}
      {@const cell = slotToCell[slot]}
      {@const c = occupancy[cell]}
      <button
        type="button"
        class="point down"
        class:dark={slot % 2 === 0}
        class:source={showHints && sourceSet.has(cell)}
        class:dest={destSet.has(cell)}
        class:selected={selected === cell}
        class:last={lastSet.has(cell)}
        data-cell={cell}
        onclick={() => click(cell)}
      >
        <span class="tri"></span>
        <span class="ptnum down">{posOfPhys(orientation, cell)}</span>
        {#if showHints && destSet.has(cell)}<span class="dot"></span>{/if}
        <span class="stack up">
          {#if c.color}
            {#each Array(discs(c.count)) as _}
              <span class="checker {c.color}"></span>
            {/each}
            {#if c.count > 5}<span class="badge">{c.count}</span>{/if}
          {/if}
        </span>
      </button>
    {/each}
  </div>

    <!-- Action buttons centred ON the board: the roll cup (when it is your roll),
         plus whatever the host passes in `center` (confirm / double / cube reply).
         The container is click-through (pointer-events:none); only the buttons
         capture taps, so building a move on the board is never blocked. -->
    <div class="board-actions" class:cube-shift={showCube && cubeSide === 'center'}>
      {#if canRoll}
        <button type="button" class="rollzone" onclick={() => onRoll?.()} aria-label="Бросить кости">
          <span class="cup-row">
            <span class="cup die-face">🎲</span>
            <span class="cup die-face">🎲</span>
          </span>
          <span class="rolltext">Бросить кости</span>
        </button>
      {/if}
      {@render center?.()}
    </div>

    <!-- the doubling cube, sitting on the central bar: centred when unclaimed,
         on the owning player's side once a double has been taken -->
    {#if showCube}
      <div
        class="cube-piece {cubeSide}"
        aria-label={`куб ${position.cube.value}, ${
          cubeSide === 'center' ? 'в центре' : cubeSide === 'bottom' ? 'у вас' : 'у соперника'
        }`}
        title={`Куб ×${position.cube.value} — ${
          cubeSide === 'center' ? 'в центре' : cubeSide === 'bottom' ? 'у вас' : 'у соперника'
        }`}
      >
        <span class="cube-val">{position.cube.value}</span>
      </div>
    {/if}
  </div>

  <div class="rail">
    <div class="dice-slot">
      {#if position.dice}
        {#key position.dice}
          {#each position.dice as d}
            <span class="die {position.turn} rolling" aria-label={`кость ${d}`}>
              {#each Array(9) as _, idx}
                <span class="pip" class:on={PIPS[d]?.includes(idx)}></span>
              {/each}
            </span>
          {/each}
        {/key}
      {/if}
    </div>

    <div class="off-tray" title="снято с доски">
      <span class="off-row"><span class="off-chip white"></span>{position.off[0]}</span>
      <span class="off-row"><span class="off-chip black"></span>{position.off[1]}</span>
    </div>

    <div class="home-label">дом<br />{orientation === 'white' ? '⚪' : '⚫'}</div>
  </div>
</div>

{#if dragging && dragColor}
  <div class="ghost checker {dragColor}" style="left: {dragXY.x}px; top: {dragXY.y}px"></div>
{/if}

{#if glide && gShown}
  <div class="glidewrap" style="left: {gx}px; top: {gy}px">
    <div bind:this={glideEl} class="glidepiece checker {glide.color}"></div>
  </div>
{/if}

<style>
  .board {
    /* a narrow side rail (dice + bear-off tray + home) sits to the RIGHT of the
       playing field; the field's 12 points + central bar ≈ 13 units fill the rest
       of the viewport width, capped on desktop so the whole board stays on-screen */
    --rail: clamp(44px, 11vw, 68px);
    --pt: clamp(18px, calc((100vw - 3.5rem - var(--rail)) / 13), 50px);
    /* row height: on wide screens proportional to the point width; on portrait
       phones it grows to fill the otherwise-wasted vertical space (taller points),
       leaving ~14rem of chrome for the controls below. */
    --row-h: calc(var(--pt) * 4.4);
    background: linear-gradient(150deg, var(--wood-hi), var(--wood-mid));
    border: clamp(8px, 1.4vw, 14px) solid;
    border-image: linear-gradient(150deg, var(--wood), var(--wood-edge)) 1;
    border-radius: var(--radius-lg);
    padding: 6px;
    display: flex;
    flex-direction: row;
    align-items: stretch;
    gap: 6px;
    user-select: none;
    touch-action: none;
    box-shadow: var(--bevel-wood), var(--shadow-board);
  }
  /* the playing field: two rows that meet directly (no horizontal bar) — the
     central vertical bar inside each row forms the continuous divider */
  .table {
    display: flex;
    flex-direction: column;
    gap: 0;
    flex: 1 1 auto;
    min-width: 0;
    position: relative; /* positioning context for the centred action overlay */
  }
  /* centred, click-through overlay holding the on-board action buttons */
  .board-actions {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 20;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    pointer-events: none; /* taps fall through to the points… */
    width: max-content;
    max-width: 92%;
  }
  .board-actions > :global(*) {
    pointer-events: auto; /* …except the buttons themselves */
  }
  /* when the doubling cube sits dead-centre, drop the action stack below it so the
     roll-cup / «Удвоить» buttons never bury the cube during your roll */
  .board-actions.cube-shift {
    top: calc(50% + clamp(40px, var(--row-h) * 0.28, 74px));
  }
  /* doubling cube: an ivory tile on the central bar; value shown big and dark.
     Centred while unclaimed, it slides to the owning player's half once taken.
     z-index sits above the felt/checkers but below the action overlay (z 20), so
     a transient roll-cup/confirm button can momentarily cover the centred cube. */
  .cube-piece {
    position: absolute;
    left: 50%;
    z-index: 8;
    width: clamp(22px, calc(var(--pt) * 0.92), 42px);
    height: clamp(22px, calc(var(--pt) * 0.92), 42px);
    display: grid;
    place-items: center;
    border-radius: clamp(4px, calc(var(--pt) * 0.16), 8px);
    background: linear-gradient(155deg, #fdf6e6, #e6d4ad);
    box-shadow:
      inset 0 1px 0 #fffaf0,
      inset 0 -2px 3px #b89a64,
      0 2px 5px rgba(0, 0, 0, 0.45);
    color: #3a2c12;
    pointer-events: none;
    transition: top 0.35s var(--ease-decelerate, ease), transform 0.35s ease;
  }
  .cube-piece .cube-val {
    font: 700 clamp(11px, calc(var(--pt) * 0.5), 22px) ui-monospace, monospace;
    line-height: 1;
  }
  /* an owned cube (sitting on a player's side) gets a warm ring so it reads as "live" */
  .cube-piece.top,
  .cube-piece.bottom {
    box-shadow:
      inset 0 1px 0 #fffaf0,
      inset 0 -2px 3px #b89a64,
      0 0 0 2px rgba(170, 120, 40, 0.65),
      0 2px 6px rgba(0, 0, 0, 0.5);
  }
  .cube-piece.center {
    top: 50%;
    transform: translate(-50%, -50%);
  }
  .cube-piece.top {
    top: calc(var(--row-h) * 0.5);
    transform: translate(-50%, -50%);
  }
  .cube-piece.bottom {
    top: calc(var(--row-h) * 1.5);
    transform: translate(-50%, -50%);
  }
  .row {
    display: flex;
    gap: 0;
    height: var(--row-h);
    background: var(--felt);
    box-shadow: var(--inset-felt);
  }
  /* Portrait phones: the board is intrinsically wide-but-short, so let the points
     grow tall to use the full screen height instead of leaving the lower half
     empty. Falls back to the proportional height if dvh is unsupported. */
  @media (max-width: 600px) {
    .board {
      /* middle ground: the average of the proportional height (compact) and the
         full screen-fill height — board uses ~half the screen without elongating
         the points so far that the page has to scroll. The reserve leaves room for
         the play controls + game log below. */
      --row-h: clamp(
        calc(var(--pt) * 3.2),
        calc((var(--pt) * 4.4 + (100dvh - 17rem) / 2) / 2),
        33dvh
      );
    }
  }
  .row.top {
    border-radius: 4px 4px 0 0;
  }
  .row.bottom {
    border-radius: 0 0 4px 4px;
  }
  .point {
    width: var(--pt);
    position: relative;
    display: flex;
    padding: 0;
    margin: 0;
    border: none;
    background: transparent;
    cursor: default;
    font: inherit;
    overflow: visible;
  }
  /* the triangular point itself */
  .tri {
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    background: var(--point-light);
    transition:
      background var(--dur-fast) var(--ease-standard),
      filter var(--dur-fast) var(--ease-standard);
  }
  .point.dark .tri {
    background: var(--point-dark);
  }
  .point.up .tri {
    clip-path: polygon(2% 0, 98% 0, 50% 92%);
  }
  .point.down .tri {
    clip-path: polygon(50% 8%, 2% 100%, 98% 100%);
  }
  /* point numbers (mover/orientation perspective) on the outer edge */
  .ptnum {
    position: absolute;
    left: 0;
    right: 0;
    text-align: center;
    font: 600 9px ui-monospace, monospace;
    color: var(--felt-label, #efe6d4);
    opacity: 0.55;
    z-index: 2;
    pointer-events: none;
  }
  .ptnum.up {
    top: 1px;
  }
  .ptnum.down {
    bottom: 1px;
  }
  .board.interactive .point {
    cursor: pointer;
  }
  /* highlights act on the triangle so they follow the point shape */
  .point.source .tri {
    background: var(--hl-source);
  }
  .point.dark.source .tri {
    background: var(--hl-source-dark);
  }
  .point.selected .tri {
    background: var(--hl-selected);
    filter: drop-shadow(0 0 5px var(--hl-selected-glow));
  }
  .point.last .tri {
    animation: lastpulse 1s ease;
  }
  @keyframes lastpulse {
    0% {
      filter: drop-shadow(0 0 0 rgba(255, 210, 70, 0)) brightness(1.7);
    }
    100% {
      filter: drop-shadow(0 0 0 rgba(255, 210, 70, 0)) brightness(1);
    }
  }
  .dot {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 34%;
    aspect-ratio: 1;
    transform: translate(-50%, -50%);
    background: radial-gradient(circle at 40% 35%, var(--hl-dest-core), var(--hl-dest));
    border-radius: 50%;
    z-index: 3;
    pointer-events: none;
    box-shadow: 0 0 7px var(--hl-selected-glow);
    animation: dotpop var(--dur-fast) var(--ease-decelerate);
  }
  @keyframes dotpop {
    from {
      transform: translate(-50%, -50%) scale(0.4);
      opacity: 0;
    }
    to {
      transform: translate(-50%, -50%) scale(1);
      opacity: 1;
    }
  }
  .stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    gap: 1px;
    padding: 3px 0;
    pointer-events: none;
    position: relative;
    z-index: 1;
  }
  .stack.up {
    justify-content: flex-end;
  }
  .stack.down {
    justify-content: flex-start;
  }
  .checker {
    width: calc(var(--pt) * 0.82);
    height: calc(var(--pt) * 0.82);
    border-radius: 50%;
    flex: 0 0 auto;
    box-shadow: var(--checker-bevel), var(--checker-drop);
  }
  .checker.white {
    background: var(--checker-grad-white);
    border: 1px solid var(--chip-white-edge, #b1a78f);
  }
  .checker.black {
    background: var(--checker-grad-black);
    border: 1px solid var(--chip-black-edge, #101013);
  }
  .ghost {
    position: fixed;
    width: clamp(24px, 5vw, 48px);
    height: clamp(24px, 5vw, 48px);
    transform: translate(-50%, -50%) scale(1.08);
    transition: none;
    z-index: 100;
    pointer-events: none;
    box-shadow: var(--checker-bevel), var(--shadow-drag);
  }
  .glidewrap {
    position: fixed;
    transform: translate(-50%, -50%);
    z-index: 90;
    pointer-events: none;
  }
  .glidepiece {
    width: clamp(24px, 5vw, 48px);
    height: clamp(24px, 5vw, 48px);
    box-shadow: var(--checker-bevel), var(--shadow-drag);
    /* transform driven by the Web Animations API (compositor-only) */
  }
  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
  .badge {
    position: absolute;
    top: 4px;
    left: 50%;
    transform: translateX(-50%);
    font: 700 11px system-ui;
    color: #fff;
    background: rgba(0, 0, 0, 0.65);
    border-radius: 9px;
    padding: 1px 6px;
    z-index: 4;
  }
  .point.down .badge {
    top: auto;
    bottom: 4px;
  }
  .bar {
    width: calc(var(--pt) * 0.6);
    background: linear-gradient(90deg, var(--wood-dark), var(--wood), var(--wood-dark));
    box-shadow: inset 0 0 8px hsl(28 45% 12% / 0.5);
  }
  /* right-side rail: dice on top, the bear-off tray in the middle, home at the
     bottom — like the side tray of a real board */
  .rail {
    width: var(--rail);
    flex: 0 0 var(--rail);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 2px;
    background: linear-gradient(180deg, var(--bar-wood), var(--wood-dark));
    border-radius: var(--radius-sm);
    box-shadow: inset 0 0 10px hsl(28 45% 12% / 0.45);
    color: var(--felt-label);
    font: var(--fw-semi) 12px var(--font-ui);
    font-variant-numeric: var(--num-tabular);
  }
  .dice-slot {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    min-height: 30px;
  }
  .off-tray {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .off-row {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .home-label {
    text-align: center;
    line-height: 1.15;
    font-size: 11px;
    opacity: 0.85;
  }
  .off-chip {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    display: inline-block;
  }
  .off-chip.white {
    background: var(--checker-grad-white);
    border: 1px solid var(--chip-white-edge, #9a8f78);
  }
  .off-chip.black {
    background: var(--checker-grad-black);
    border: 1px solid var(--chip-black-edge, #000);
  }
  /* the roll cup now lives in the centre of the board (not the side rail): a
     prominent, inviting target shown only when it is the player's turn to roll */
  .rollzone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    box-sizing: border-box;
    padding: 0.55rem 1.1rem 0.5rem;
    border: 1px solid var(--wood-hi);
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, var(--wood), var(--wood-mid));
    color: var(--felt-label);
    font: var(--fw-bold) 0.92rem var(--font-ui);
    cursor: pointer;
    box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    transition:
      background var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out);
    animation:
      act-pop var(--dur-base) var(--ease-settle) backwards,
      roll-breathe 2.6s ease-in-out 0.6s infinite;
  }
  .rollzone:hover {
    background: linear-gradient(180deg, var(--wood-hi), var(--wood));
    transform: translateY(-1px) scale(1.02);
    box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.26);
  }
  .rollzone:active {
    transform: scale(0.96);
  }
  .rollzone .cup-row {
    display: inline-flex;
    gap: 5px;
  }
  .rollzone .die-face {
    font-size: 22px;
    line-height: 1;
    display: inline-block;
    filter: drop-shadow(0 1px 1px rgba(0, 0, 0, 0.4));
  }
  /* the two cups jiggle a touch to read as "shake & throw" */
  .rollzone .die-face:first-child {
    animation: cup-jiggle 2.6s ease-in-out 0.6s infinite;
  }
  .rollzone .die-face:last-child {
    animation: cup-jiggle 2.6s ease-in-out 0.75s infinite;
  }
  .rollzone .rolltext {
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  @keyframes act-pop {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.9);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }
  @keyframes roll-breathe {
    0%,
    100% {
      box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    }
    50% {
      box-shadow:
        var(--shadow-3),
        0 0 0 4px rgba(255, 232, 150, 0.16),
        inset 0 1px 0 rgba(255, 255, 255, 0.22);
    }
  }
  @keyframes cup-jiggle {
    0%,
    88%,
    100% {
      transform: rotate(0);
    }
    92% {
      transform: rotate(-12deg);
    }
    96% {
      transform: rotate(10deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .rollzone {
      animation: none;
    }
    .rollzone .die-face {
      animation: none;
    }
  }
  @keyframes reveal {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .rollzone {
      animation: none;
    }
  }
  .die {
    width: 30px;
    height: 30px;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(3, 1fr);
    gap: 1px;
    padding: 4px;
    border-radius: var(--radius-sm);
    box-sizing: border-box;
    box-shadow: var(--die-bevel), var(--shadow-1);
  }
  .die.white {
    background: linear-gradient(145deg, #fffdf7, #e6ddc8);
  }
  .die.black {
    background: linear-gradient(145deg, #3a3a3e, #161618);
  }
  .pip {
    border-radius: 50%;
    align-self: center;
    justify-self: center;
    width: 6px;
    height: 6px;
    background: transparent;
  }
  .die.white .pip.on {
    background: #2a1a0c;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.6);
  }
  .die.black .pip.on {
    background: #f3ece0;
    box-shadow: inset 0 1px 1px rgba(0, 0, 0, 0.5);
  }
  .die.rolling {
    animation: tumble var(--dur-tumble) var(--spring-land);
  }
  /* the second die lands a beat after the first (two dice thrown) */
  .die.rolling ~ .die.rolling {
    animation-delay: 55ms;
  }
  @keyframes tumble {
    0% {
      transform: scale(0.55) rotate(-210deg);
      opacity: 0;
    }
    35% {
      opacity: 1;
    }
    70% {
      transform: scale(1.12) rotate(14deg);
    }
    100% {
      transform: scale(1) rotate(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .die.rolling,
    .point.last .tri {
      animation: none;
    }
  }
</style>
