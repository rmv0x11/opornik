<script lang="ts">
  import { onMount } from 'svelte';
  import type { PlayerColor, VariantId } from '../../engine/types';
  import { deleteSave, importGame, listSaves, type SavedGame } from '../storage';
  import {
    BOARD_THEMES,
    CHECKER_THEMES,
    themeStyle,
    type ThemeChoice,
  } from '../themes';

  let {
    onStart,
    onResume,
    theme,
    onTheme,
    onRules,
  }: {
    onStart: (cfg: {
      variant: VariantId;
      aiPly: number;
      humanColor: PlayerColor;
      matchLength: number | null;
    }) => void;
    onResume: (g: SavedGame) => void;
    theme: ThemeChoice;
    onTheme: (t: ThemeChoice) => void;
    onRules: () => void;
  } = $props();

  let variant = $state<VariantId>('traditional');
  let aiPly = $state(2);
  let humanColor = $state<PlayerColor>('white');
  let matchLength = $state<number | null>(null);

  let saves = $state<SavedGame[]>([]);
  let importText = $state('');
  let importError = $state('');

  onMount(() => {
    saves = listSaves();
  });

  const matchLengths = [7, 9, 11, 13, 15, 17];

  const variants: { id: VariantId; name: string; note: string }[] = [
    { id: 'traditional', name: 'Традиционные', note: 'правило головы, без куба' },
    { id: 'nardegammon', name: 'Нардегаммон', note: 'куб удвоений + Кроуфорд' },
    { id: 'classic', name: 'Классика', note: 'возможна ничья' },
    { id: 'hachapuri', name: 'Хачапури', note: '4 на голове, без правила головы' },
  ];
  const diffs = [
    { ply: 1, name: 'Новичок' },
    { ply: 2, name: 'Сильный' },
    { ply: 3, name: 'Эксперт' },
  ];
  const variantName: Record<VariantId, string> = {
    traditional: 'Традиционные',
    nardegammon: 'Нардегаммон',
    classic: 'Классика',
    hachapuri: 'Хачапури',
  };

  function removeSave(id: string) {
    deleteSave(id);
    saves = listSaves();
  }
  function doImport() {
    importError = '';
    const g = importGame(importText);
    if (!g) {
      importError = 'Не удалось распознать позицию — проверьте строку.';
      return;
    }
    onResume(g);
  }
  function saveSummary(g: SavedGame): string {
    const score =
      g.matchLength != null
        ? `матч ${g.matchScore[0]}:${g.matchScore[1]}→${g.matchLength}`
        : `${g.matchScore[0]}:${g.matchScore[1]}`;
    const turn = g.setup.turn === g.humanColor ? 'ваш ход' : 'ход движка';
    const when = new Date(g.savedAt).toLocaleString('ru-RU', {
      day: '2-digit',
      month: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    });
    return `${variantName[g.variant]} · ${score} · ход №${g.setup.turn_number ?? 0} (${turn}) · ${when}`;
  }
</script>

<div class="home">
  <h1>Опорник</h1>
  <p class="sub">
    Длинные нарды против движка (нейросеть + поиск) ·
    <button class="rules-link" onclick={onRules}>📖 Правила</button>
  </p>

  {#if saves.length}
    <h2>Продолжить партию</h2>
    <ul class="saves">
      {#each saves as g (g.id)}
        <li class="save">
          <button class="save-go" onclick={() => onResume(g)}>
            <span class="save-name">▶ {g.name}</span>
            <span class="save-meta">{saveSummary(g)}</span>
          </button>
          <button class="save-del" title="Удалить" onclick={() => removeSave(g.id)}>✕</button>
        </li>
      {/each}
    </ul>
  {/if}

  <h2>Вариант</h2>
  <div class="cards">
    {#each variants as v}
      <button class="card" class:sel={variant === v.id} onclick={() => (variant = v.id)}>
        <span class="cn">{v.name}</span>
        <span class="cnote">{v.note}</span>
      </button>
    {/each}
  </div>

  <h2>Сила движка</h2>
  <div class="row">
    {#each diffs as d}
      <button class="pill" class:sel={aiPly === d.ply} onclick={() => (aiPly = d.ply)}>
        {d.name}
      </button>
    {/each}
  </div>

  <h2>Ваш цвет</h2>
  <div class="row">
    <button class="pill" class:sel={humanColor === 'white'} onclick={() => (humanColor = 'white')}>
      ⚪ Белые (первый ход)
    </button>
    <button class="pill" class:sel={humanColor === 'black'} onclick={() => (humanColor = 'black')}>
      ⚫ Чёрные
    </button>
  </div>

  <h2>Формат</h2>
  <div class="row">
    <button class="pill" class:sel={matchLength === null} onclick={() => (matchLength = null)}>
      Деньги (без матча)
    </button>
    {#each matchLengths as n}
      <button class="pill" class:sel={matchLength === n} onclick={() => (matchLength = n)}>
        Матч до {n}
      </button>
    {/each}
  </div>

  <h2>Оформление</h2>
  <div class="theme-preview" style={themeStyle(theme)} aria-hidden="true">
    <span class="pv-row">
      <span class="pv-tri up light"></span><span class="pv-tri up dark"></span><span
        class="pv-tri up light"
      ></span><span class="pv-tri up dark"></span>
    </span>
    <span class="pv-chips">
      <span class="pv-chip w"></span><span class="pv-chip b"></span>
    </span>
  </div>
  <div class="row">
    {#each BOARD_THEMES as b}
      <button class="pill" class:sel={theme.board === b.id} onclick={() => onTheme({ ...theme, board: b.id })}>
        {b.name}
      </button>
    {/each}
  </div>
  <div class="row">
    {#each CHECKER_THEMES as c}
      <button
        class="pill swatch-pill"
        class:sel={theme.checkers === c.id}
        onclick={() => onTheme({ ...theme, checkers: c.id })}
      >
        <span class="sw" style="background: {c.vars['--checker-grad-white']}"></span>
        <span class="sw" style="background: {c.vars['--checker-grad-black']}"></span>
        {c.name}
      </button>
    {/each}
  </div>

  <button class="start" onclick={() => onStart({ variant, aiPly, humanColor, matchLength })}>
    {matchLength === null ? 'Начать партию' : `Начать матч до ${matchLength}`}
  </button>

  <details class="import">
    <summary>📥 Импорт позиции</summary>
    <p class="import-help">
      Вставьте строку партии (экспорт из игры) и продолжите с этой позиции.
    </p>
    <textarea
      class="import-box"
      bind:value={importText}
      placeholder="NARD1:…"
      rows="3"
    ></textarea>
    {#if importError}<p class="import-err">{importError}</p>{/if}
    <button class="ghost" onclick={doImport} disabled={!importText.trim()}>
      Продолжить с этой позиции
    </button>
  </details>

  <footer class="ver">Опорник · v{__APP_VERSION__}</footer>
</div>

<style>
  .home {
    max-width: 640px;
    margin: 0 auto;
  }
  h1 {
    font-size: 1.9rem;
    margin: 0;
  }
  .sub {
    color: var(--ink-muted);
    margin: 0.2rem 0 1.5rem;
  }
  .rules-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--ink-wood);
    font: inherit;
    font-weight: var(--fw-semi);
    cursor: pointer;
    text-decoration: underline;
  }
  h2 {
    font-size: 0.82rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ink-wood);
    margin: 1.4rem 0 0.6rem;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 0.6rem;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.2rem;
    padding: 0.7rem 0.9rem;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    background: var(--surface);
    box-shadow: var(--shadow-1);
    cursor: pointer;
    text-align: left;
    transition:
      border-color var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out);
  }
  .card:hover {
    box-shadow: var(--shadow-2);
  }
  .card:active {
    transform: scale(0.99);
  }
  .card.sel {
    border-color: var(--ink-wood);
    background: var(--surface-raised);
  }
  .cn {
    font-weight: 700;
  }
  .cnote {
    font-size: 0.8rem;
    color: #777;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .pill {
    padding: 0.5rem 0.95rem;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-pill);
    background: var(--surface);
    cursor: pointer;
    font: inherit;
    transition:
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out);
  }
  .pill:hover {
    background: var(--surface-raised);
  }
  .pill:active {
    transform: scale(0.97);
  }
  .pill.sel {
    border-color: var(--ink-wood);
    background: var(--surface-raised);
    font-weight: var(--fw-semi);
  }
  .start {
    margin-top: 1.8rem;
    font-size: 1.05rem;
    font-weight: var(--fw-semi);
    padding: 0.75rem 1.6rem;
    border: none;
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, var(--btn-primary-hi), var(--btn-primary));
    color: #fff;
    cursor: pointer;
    box-shadow: var(--shadow-2), inset 0 1px 0 rgba(255, 255, 255, 0.18);
    transition:
      filter var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out),
      transform var(--dur-instant) var(--ease-out);
  }
  .start:hover {
    filter: brightness(1.06);
    box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.22);
  }
  .start:active {
    transform: scale(0.98);
  }
  @media (max-width: 600px) {
    .start {
      width: 100%;
    }
    .pill {
      min-height: 44px;
      display: inline-flex;
      align-items: center;
    }
  }
  .theme-preview {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 64px;
    margin-bottom: 0.6rem;
    padding: 0 14px;
    border-radius: var(--radius-md);
    background: var(--felt);
    border: 4px solid var(--wood-mid);
    box-shadow: var(--shadow-1), var(--inset-felt);
    overflow: hidden;
  }
  .pv-row {
    display: flex;
    align-self: flex-start;
    gap: 0;
  }
  .pv-tri.up {
    width: 22px;
    height: 42px;
    clip-path: polygon(2% 0, 98% 0, 50% 92%);
  }
  .pv-tri.light {
    background: var(--point-light);
  }
  .pv-tri.dark {
    background: var(--point-dark);
  }
  .pv-chips {
    display: flex;
    gap: 6px;
  }
  .pv-chip {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    box-shadow: var(--checker-bevel), var(--checker-drop);
  }
  .pv-chip.w {
    background: var(--checker-grad-white);
  }
  .pv-chip.b {
    background: var(--checker-grad-black);
  }
  .swatch-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .sw {
    width: 15px;
    height: 15px;
    border-radius: 50%;
    box-shadow: inset 0 1px 1px rgba(255, 255, 255, 0.4);
  }
  .ver {
    margin-top: 2rem;
    text-align: center;
    font-size: 0.75rem;
    color: var(--ink-faint);
    letter-spacing: 0.04em;
  }
  .saves {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .save {
    display: flex;
    gap: 0.4rem;
    align-items: stretch;
  }
  .save-go {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    padding: 0.55rem 0.8rem;
    border: 2px solid #d8c4a6;
    border-radius: 10px;
    background: #fffdf9;
    cursor: pointer;
    text-align: left;
  }
  .save-go:hover {
    border-color: #6b4423;
    background: #f8efe2;
  }
  .save-name {
    font-weight: 700;
    color: #6b4423;
  }
  .save-meta {
    font-size: 0.78rem;
    color: #888;
  }
  .save-del {
    border: 2px solid #e0cccc;
    border-radius: 10px;
    background: #fff;
    color: #b22;
    cursor: pointer;
    padding: 0 0.7rem;
    font-size: 0.9rem;
  }
  .save-del:hover {
    background: #fbeeee;
  }
  .import {
    margin-top: 1.6rem;
    border-top: 1px solid #e7ddcd;
    padding-top: 0.8rem;
  }
  .import > summary {
    cursor: pointer;
    color: #6b4423;
    font-weight: 600;
  }
  .import-help {
    font-size: 0.82rem;
    color: #777;
    margin: 0.5rem 0;
  }
  .import-box {
    width: 100%;
    box-sizing: border-box;
    font: 0.82rem ui-monospace, monospace;
    border: 1px solid #d8c4a6;
    border-radius: 8px;
    padding: 0.5rem;
    resize: vertical;
  }
  .import-err {
    color: #b22;
    font-size: 0.82rem;
    margin: 0.4rem 0;
  }
  .ghost {
    margin-top: 0.5rem;
    border: 1px solid #b5895c;
    border-radius: 8px;
    background: #fff;
    color: #6b4423;
    padding: 0.5rem 1rem;
    cursor: pointer;
    font: inherit;
  }
  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
