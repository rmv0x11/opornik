<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { PositionDto } from '../../engine/types';
  import { buildSlotToCell, phys, type PlayerColor } from './coords';

  let {
    position,
    orientation = 'white',
    interactive = false,
    sources = [],
    dests = [],
    selected = null,
    lastCells = [],
    onPointClick,
  }: {
    position: PositionDto;
    orientation?: PlayerColor;
    interactive?: boolean;
    sources?: number[];
    dests?: number[];
    selected?: number | null;
    lastCells?: number[]; // physical cells touched by the most recent move (pulse)
    onPointClick?: (cell: number) => void;
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
  const sourceSet = $derived(new Set(sources));
  const destSet = $derived(new Set(dests));
  const lastSet = $derived(new Set(lastCells));

  const topSlots = [12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23];
  const bottomSlots = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];

  function discs(count: number): number {
    return Math.min(count, 5);
  }
  function click(cell: number) {
    if (interactive) onPointClick?.(cell);
  }

  // ---- drag (select source on press-move, apply on release over a destination) ----
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
    if (!dragging && dx * dx + dy * dy > 36) {
      dragging = true;
      dragColor = occupancy[downCell].color;
      if (selected !== downCell) onPointClick?.(downCell); // select source → show dests
    }
    if (dragging) dragXY = { x: e.clientX, y: e.clientY };
  }
  function onWinUp(e: PointerEvent) {
    window.removeEventListener('pointermove', onWinMove);
    window.removeEventListener('pointerup', onWinUp);
    if (dragging) {
      const t = cellFromEl(document.elementFromPoint(e.clientX, e.clientY));
      if (t != null && destSet.has(t)) onPointClick?.(t);
    }
    dragging = false;
    dragColor = null;
    downCell = null;
  }
  onDestroy(() => {
    window.removeEventListener('pointermove', onWinMove);
    window.removeEventListener('pointerup', onWinUp);
  });
</script>

<div
  class="board"
  class:interactive
  role="application"
  aria-label="Игровая доска"
  onpointerdown={onBoardPointerDown}
>
  <div class="row top">
    {#each topSlots as slot, i}
      {#if i === 6}<div class="bar"></div>{/if}
      {@const cell = slotToCell[slot]}
      {@const c = occupancy[cell]}
      <button
        type="button"
        class="point up"
        class:dark={slot % 2 === 0}
        class:source={sourceSet.has(cell)}
        class:dest={destSet.has(cell)}
        class:selected={selected === cell}
        class:last={lastSet.has(cell)}
        data-cell={cell}
        onclick={() => click(cell)}
      >
        {#if destSet.has(cell)}<span class="dot"></span>{/if}
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

  <div class="midbar">
    <span>дом ↓ {orientation === 'white' ? '⚪' : '⚫'}</span>
    <span class="dicewrap">
      {#key position.dice}
        <span class="dice rolling">
          {#if position.dice}🎲 {position.dice[0]}–{position.dice[1]}{:else}—{/if}
        </span>
      {/key}
    </span>
    <span>сброс: ⚪{position.off[0]} · ⚫{position.off[1]}</span>
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
        class:source={sourceSet.has(cell)}
        class:dest={destSet.has(cell)}
        class:selected={selected === cell}
        class:last={lastSet.has(cell)}
        data-cell={cell}
        onclick={() => click(cell)}
      >
        {#if destSet.has(cell)}<span class="dot"></span>{/if}
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
</div>

{#if dragging && dragColor}
  <div
    class="ghost checker {dragColor}"
    style="left: {dragXY.x}px; top: {dragXY.y}px"
  ></div>
{/if}

<style>
  .board {
    --pt: clamp(28px, 6vw, 56px);
    background: #b5895c;
    border: 8px solid #6b4423;
    border-radius: 6px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    user-select: none;
    touch-action: none;
  }
  .row {
    display: flex;
    gap: 2px;
    height: calc(var(--pt) * 4.2);
  }
  .point {
    width: var(--pt);
    background: #e8d3b0;
    position: relative;
    display: flex;
    padding: 0;
    margin: 0;
    border: 2px solid transparent;
    border-radius: 0;
    cursor: default;
    font: inherit;
  }
  .board.interactive .point {
    cursor: pointer;
  }
  .point.dark {
    background: #c9a06a;
  }
  .point.source {
    box-shadow: inset 0 0 0 3px rgba(40, 160, 90, 0.55);
  }
  .point.selected {
    box-shadow: inset 0 0 0 3px #2a6;
  }
  .point.last {
    animation: lastpulse 1s ease;
  }
  @keyframes lastpulse {
    0% {
      outline: 3px solid rgba(255, 200, 60, 0.95);
      outline-offset: -3px;
    }
    100% {
      outline: 3px solid rgba(255, 200, 60, 0);
      outline-offset: -3px;
    }
  }
  .dot {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 38%;
    aspect-ratio: 1;
    transform: translate(-50%, -50%);
    background: rgba(40, 160, 90, 0.6);
    border-radius: 50%;
    z-index: 3;
    pointer-events: none;
  }
  .stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    gap: 1px;
    padding: 2px 0;
    pointer-events: none;
  }
  .stack.up {
    justify-content: flex-end;
  }
  .stack.down {
    justify-content: flex-start;
  }
  .checker {
    width: calc(var(--pt) * 0.78);
    height: calc(var(--pt) * 0.78);
    border-radius: 50%;
    flex: 0 0 auto;
  }
  .checker.white {
    background: radial-gradient(circle at 35% 30%, #fff, #d8d8d8);
    border: 1px solid #999;
  }
  .checker.black {
    background: radial-gradient(circle at 35% 30%, #555, #111);
    border: 1px solid #000;
  }
  .ghost {
    position: fixed;
    width: clamp(24px, 5vw, 46px);
    height: clamp(24px, 5vw, 46px);
    transform: translate(-50%, -50%);
    z-index: 100;
    pointer-events: none;
    box-shadow: 0 4px 10px rgba(0, 0, 0, 0.4);
  }
  .badge {
    position: absolute;
    top: 2px;
    left: 50%;
    transform: translateX(-50%);
    font: 600 12px system-ui;
    color: #fff;
    background: rgba(0, 0, 0, 0.6);
    border-radius: 8px;
    padding: 0 5px;
    z-index: 4;
  }
  .point.down .badge {
    top: auto;
    bottom: 2px;
  }
  .bar {
    width: calc(var(--pt) * 0.5);
    background: #6b4423;
    border-radius: 3px;
  }
  .midbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font: 600 13px system-ui;
    color: #3a2410;
    padding: 2px 6px;
  }
  .dicewrap {
    display: inline-block;
  }
  .dice {
    font-size: 15px;
    display: inline-block;
  }
  .dice.rolling {
    animation: roll 0.4s ease;
  }
  @keyframes roll {
    0% {
      transform: scale(0.6) rotate(-10deg);
      opacity: 0.3;
    }
    60% {
      transform: scale(1.15) rotate(6deg);
    }
    100% {
      transform: scale(1) rotate(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dice.rolling,
    .point.last {
      animation: none;
    }
  }
</style>
