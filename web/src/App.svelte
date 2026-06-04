<script lang="ts">
  import Home from './lib/screens/Home.svelte';
  import Play from './lib/screens/Play.svelte';
  import type { PlayerColor, VariantId } from './engine/types';

  type Config = {
    variant: VariantId;
    aiPly: number;
    humanColor: PlayerColor;
    matchLength: number | null;
  };

  let screen = $state<'home' | 'play'>('home');
  let cfg = $state<Config>({
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
  });

  function start(c: Config) {
    cfg = c;
    screen = 'play';
  }
  function exit() {
    screen = 'home';
  }
</script>

<main>
  {#if screen === 'home'}
    <Home onStart={start} />
  {:else}
    {#key cfg}
      <Play
        variant={cfg.variant}
        aiPly={cfg.aiPly}
        humanColor={cfg.humanColor}
        matchLength={cfg.matchLength}
        onExit={exit}
      />
    {/key}
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    background: #faf7f2;
  }
  main {
    font-family: system-ui, -apple-system, sans-serif;
    max-width: 820px;
    margin: 1.5rem auto;
    padding: 0 1rem 3rem;
    color: #1c1c1c;
  }
</style>
