<script lang="ts">
  import { fade } from 'svelte/transition';
  import Home from './lib/screens/Home.svelte';
  import Play from './lib/screens/Play.svelte';
  import Rules from './lib/screens/Rules.svelte';
  import type { PlayerColor, VariantId } from './engine/types';
  import type { SavedGame } from './lib/storage';
  import { getTheme, setTheme, themeStyle, type ThemeChoice } from './lib/themes';

  const reduce =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const screenIn = { duration: reduce ? 0 : 200, delay: reduce ? 0 : 60 };
  const screenOut = { duration: reduce ? 0 : 120 };

  type Config = {
    variant: VariantId;
    aiPly: number;
    humanColor: PlayerColor;
    matchLength: number | null;
  };

  let screen = $state<'home' | 'play' | 'rules'>('home');
  let gameKey = $state(0); // bump to force a fresh Play instance
  let resume = $state<SavedGame | null>(null);
  let theme = $state<ThemeChoice>(getTheme());
  function applyTheme(t: ThemeChoice) {
    theme = t;
    setTheme(t);
  }
  let cfg = $state<Config>({
    variant: 'traditional',
    aiPly: 2,
    humanColor: 'white',
    matchLength: null,
  });

  function start(c: Config) {
    cfg = c;
    resume = null;
    gameKey += 1;
    screen = 'play';
  }
  function resumeGame(g: SavedGame) {
    cfg = {
      variant: g.variant,
      aiPly: g.aiPly,
      humanColor: g.humanColor,
      matchLength: g.matchLength,
    };
    resume = g;
    gameKey += 1;
    screen = 'play';
  }
  function exit() {
    screen = 'home';
  }
</script>

<main>
  {#if screen === 'home'}
    <div in:fade={screenIn} out:fade={screenOut}>
      <Home
        onStart={start}
        onResume={resumeGame}
        {theme}
        onTheme={applyTheme}
        onRules={() => (screen = 'rules')}
      />
    </div>
  {:else if screen === 'rules'}
    <div in:fade={screenIn} out:fade={screenOut}>
      <Rules onBack={() => (screen = 'home')} />
    </div>
  {:else}
    {#key gameKey}
      <div in:fade={screenIn} out:fade={screenOut}>
        <Play
          variant={cfg.variant}
          aiPly={cfg.aiPly}
          humanColor={cfg.humanColor}
          matchLength={cfg.matchLength}
          {resume}
          boardStyle={themeStyle(theme)}
          onExit={exit}
        />
      </div>
    {/key}
  {/if}
</main>

<style>
  /* ============================================================
     Design tokens — warm-neutral, low-chroma, restrained.
     Researched best practices (Refactoring UI / Material 3 /
     Josh Comeau / WCAG). Additive: no classes renamed.
     ============================================================ */
  :global(:root) {
    /* surfaces — warm off-whites, tinted neutrals */
    --app-bg: #f7f3ec;
    --surface: #fffdf9;
    --surface-raised: #fbf7f0;
    --surface-sunken: #efe6d5;
    --hairline: #e3d6bf;
    --hairline-soft: #ece1cd;
    /* ink ramp — warm near-black, never pure #000 */
    --ink: #2b2622;
    --ink-wood: #6f5436;
    --ink-muted: #82715a;
    --ink-faint: #93826a;
    /* wood frame — warm triad */
    --wood-hi: #b78a55;
    --wood: #8a5a32;
    --wood-mid: #6f4527;
    --wood-dark: #512f1b;
    --wood-edge: #44281a;
    --bar-wood: #5f3a22;
    /* felt — deep, soft, desaturated (not neon) */
    --felt: #1f5f43;
    --felt-dark: #1a5139;
    --felt-edge: #15482f;
    --felt-label: #efe6d4;
    /* points */
    --point-light: #d8b98a;
    --point-dark: #a9743f;
    /* highlights — muted sage, shifted off felt lightness (no vibration) */
    --hl-source: #82bf9a;
    --hl-source-dark: #5fa57e;
    --hl-selected: #4fb488;
    --hl-selected-glow: rgba(79, 180, 136, 0.45);
    --hl-dest: #5cab85;
    --hl-dest-core: #dff3e3;
    /* semantic — calm, desaturated, AA on surface */
    --ok: #1f7a4d;
    --warn: #9a6a16;
    --danger: #a83f37;
    --danger-soft: #f6e6e2;
    /* interactive wood */
    --btn-primary: #6f4527;
    --btn-primary-hi: #82542f;
    --pill-border: #c8a878;

    /* type */
    --font-ui: system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
    --font-mono: ui-monospace, 'SF Mono', 'Cascadia Code', Menlo, Consolas, monospace;
    --num-tabular: tabular-nums lining-nums;
    /* font weights (used in font: shorthands across Board/Play — must be defined
       or the whole shorthand is invalid and silently falls back) */
    --fw-medium: 500;
    --fw-semi: 600;
    --fw-bold: 700;
    --radius-sm: 6px;
    --radius-md: 10px;
    --radius-lg: 14px;
    --radius-pill: 999px;

    /* motion — easing + durations (GPU-cheap) */
    --ease-standard: cubic-bezier(0.2, 0, 0, 1);
    --ease-decelerate: cubic-bezier(0, 0, 0, 1);
    --ease-emphasized-out: cubic-bezier(0.05, 0.7, 0.1, 1);
    --ease-out: cubic-bezier(0.33, 1, 0.68, 1);
    --ease-settle: cubic-bezier(0.34, 1.3, 0.64, 1);
    --dur-instant: 90ms;
    --dur-fast: 140ms;
    --dur-quick: 200ms;
    --dur-base: 260ms;
    --dur-move: 300ms;
    --dur-bar: 420ms;
    --dur-glow: 800ms;
    /* game-feel beats (Disney principles, trimmed for restraint) */
    --ease-back-soft: cubic-bezier(0.34, 1.16, 0.64, 1); /* ~4% overshoot */
    --dur-pickup: 100ms;
    --dur-settle: 120ms;
    --dur-tumble: 360ms;
    --spring-land: linear(
      0,
      0.5 18%,
      0.84 32%,
      1.02 46%,
      1.08 56%,
      1.03 72%,
      1 100%
    ); /* bounce-on-land for dice */

    /* elevation — layered, low-opacity, warm-tinted, one light source (top-left) */
    --shadow-1: 0 1px 1px hsl(220 14% 12% / 0.1), 0 2px 3px hsl(220 14% 12% / 0.1);
    --shadow-2: 0 1px 1px hsl(220 14% 12% / 0.09), 0 3px 5px hsl(220 14% 12% / 0.09),
      0 6px 10px hsl(220 14% 12% / 0.08);
    --shadow-3: 0 1px 1px hsl(220 14% 12% / 0.08), 0 4px 6px hsl(220 14% 12% / 0.08),
      0 9px 16px hsl(220 14% 12% / 0.08);
    --shadow-board: 0 1px 2px hsl(28 45% 12% / 0.2), 0 8px 18px hsl(28 45% 12% / 0.22),
      0 18px 40px hsl(28 45% 12% / 0.2);
    --inset-felt: inset 0 0 36px hsl(150 60% 4% / 0.55), inset 0 1px 0 hsl(0 0% 100% / 0.06),
      inset 0 2px 6px hsl(150 60% 4% / 0.45);
    --bevel-wood: inset 0 1px 0 hsl(34 50% 70% / 0.55), inset 0 -2px 3px hsl(28 45% 12% / 0.45);
    --checker-bevel: inset 0 2px 3px hsl(0 0% 100% / 0.45), inset 0 -3px 5px hsl(28 45% 12% / 0.4),
      inset 0 0 0 1px hsl(0 0% 100% / 0.1);
    --checker-drop: 0 1px 1px hsl(150 60% 4% / 0.45), 0 3px 5px hsl(150 60% 4% / 0.35);
    --checker-grad-white: radial-gradient(circle at 32% 26%, #ffffff 0%, #f1ece2 42%, #ddd5c6 72%, #c2b8a6 100%);
    --checker-grad-black: radial-gradient(circle at 32% 26%, #7c7c82 0%, #3a3a40 40%, #1a1a1e 72%, #0a0a0c 100%);
    --die-bevel: inset 0 2px 2px hsl(0 0% 100% / 0.65), inset 0 -2px 3px hsl(28 45% 12% / 0.3),
      inset 1px 0 1px hsl(0 0% 100% / 0.25), inset -1px 0 1px hsl(28 45% 12% / 0.2);
    --shadow-drag: 0 6px 10px hsl(150 40% 6% / 0.4), 0 14px 28px hsl(150 40% 6% / 0.3);
    --focus-ring: 0 0 0 2px var(--surface), 0 0 0 4px var(--hl-selected);
  }
  :global(body) {
    margin: 0;
    background: var(--app-bg);
  }
  :global(:focus-visible) {
    outline: none;
    box-shadow: var(--focus-ring);
  }
  main {
    font-family: var(--font-ui);
    font-size: 1rem;
    max-width: 820px;
    margin: 1.5rem auto;
    padding: 0 1rem 3rem;
    color: var(--ink);
  }
  @media (max-width: 600px) {
    main {
      margin: 0.6rem auto;
      padding: 0 0.6rem 2rem;
    }
  }
</style>
