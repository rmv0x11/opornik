<script lang="ts">
  import type { PlayerColor, VariantId } from '../../engine/types';

  let {
    onStart,
  }: {
    onStart: (cfg: {
      variant: VariantId;
      aiPly: number;
      humanColor: PlayerColor;
      matchLength: number | null;
    }) => void;
  } = $props();

  let variant = $state<VariantId>('traditional');
  let aiPly = $state(2);
  let humanColor = $state<PlayerColor>('white');
  let matchLength = $state<number | null>(null);

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
</script>

<div class="home">
  <h1>Опорник</h1>
  <p class="sub">Длинные нарды против движка (нейросеть + поиск)</p>

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

  <button class="start" onclick={() => onStart({ variant, aiPly, humanColor, matchLength })}>
    {matchLength === null ? 'Начать партию' : `Начать матч до ${matchLength}`}
  </button>
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
    color: #666;
    margin: 0.2rem 0 1.5rem;
  }
  h2 {
    font-size: 1rem;
    color: #6b4423;
    margin: 1.2rem 0 0.5rem;
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
    border: 2px solid #ddd;
    border-radius: 10px;
    background: #fff;
    cursor: pointer;
    text-align: left;
  }
  .card.sel {
    border-color: #6b4423;
    background: #f8efe2;
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
    padding: 0.45rem 0.9rem;
    border: 2px solid #ddd;
    border-radius: 999px;
    background: #fff;
    cursor: pointer;
    font: inherit;
  }
  .pill.sel {
    border-color: #6b4423;
    background: #f8efe2;
    font-weight: 600;
  }
  .start {
    margin-top: 1.8rem;
    font-size: 1.05rem;
    padding: 0.7rem 1.6rem;
    border: none;
    border-radius: 10px;
    background: #6b4423;
    color: #fff;
    cursor: pointer;
  }
</style>
