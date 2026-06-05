// Board & checker appearance themes. Each theme is a set of CSS custom-property
// overrides applied (inline) to the .board element, so the board, points, frame,
// checkers and off-tray restyle while the rest of the app chrome stays constant.

export interface ThemeChoice {
  board: string;
  checkers: string;
}

export interface NamedVars {
  id: string;
  name: string;
  vars: Record<string, string>;
}

// ---- board themes: felt + triangular points + wood frame --------------------
export const BOARD_THEMES: NamedVars[] = [
  {
    id: 'emerald',
    name: 'Изумруд',
    vars: {
      '--felt': '#1f5f43',
      '--felt-dark': '#1a5139',
      '--felt-edge': '#15482f',
      '--felt-label': '#efe6d4',
      '--point-light': '#d8b98a',
      '--point-dark': '#a9743f',
      '--wood-hi': '#b78a55',
      '--wood': '#8a5a32',
      '--wood-mid': '#6f4527',
      '--wood-dark': '#512f1b',
      '--wood-edge': '#44281a',
      '--bar-wood': '#5f3a22',
    },
  },
  {
    id: 'classic',
    name: 'Классик',
    vars: {
      // cream felt with alternating green & brown points (как на фото)
      '--felt': '#e7dcc0',
      '--felt-dark': '#ded1b0',
      '--felt-edge': '#cfc0a0',
      '--felt-label': '#f3ead6',
      '--point-light': '#9a6a44',
      '--point-dark': '#3f6b3f',
      '--wood-hi': '#5a4636',
      '--wood': '#3f3024',
      '--wood-mid': '#332619',
      '--wood-dark': '#241a11',
      '--wood-edge': '#1a120b',
      '--bar-wood': '#2f2418',
      '--inset-felt':
        'inset 0 0 30px hsl(36 30% 30% / 0.28), inset 0 1px 0 hsl(0 0% 100% / 0.25), inset 0 2px 5px hsl(36 30% 30% / 0.2)',
    },
  },
  {
    id: 'midnight',
    name: 'Полночь',
    vars: {
      '--felt': '#26303a',
      '--felt-dark': '#1f2730',
      '--felt-edge': '#171d24',
      '--felt-label': '#e2e8ee',
      '--point-light': '#48586a',
      '--point-dark': '#2f3a47',
      '--wood-hi': '#4a4f57',
      '--wood': '#363a41',
      '--wood-mid': '#2a2d33',
      '--wood-dark': '#1c1f24',
      '--wood-edge': '#141619',
      '--bar-wood': '#23262b',
    },
  },
];

// ---- checker themes: the two sides' colours (marble-ish radial gradients) ----
function grad(c0: string, c1: string, c2: string, c3: string): string {
  return `radial-gradient(circle at 32% 26%, ${c0} 0%, ${c1} 40%, ${c2} 72%, ${c3} 100%)`;
}
export const CHECKER_THEMES: NamedVars[] = [
  {
    id: 'ivory',
    name: 'Слоновая кость',
    vars: {
      '--checker-grad-white': grad('#ffffff', '#f1ece2', '#ddd5c6', '#c2b8a6'),
      '--checker-grad-black': grad('#7c7c82', '#3a3a40', '#1a1a1e', '#0a0a0c'),
      '--chip-white-edge': '#b1a78f',
      '--chip-black-edge': '#101013',
      '--chip-white-solid': '#ece5d6',
      '--chip-black-solid': '#26262b',
    },
  },
  {
    id: 'amber',
    name: 'Янтарь и лазурь',
    vars: {
      '--checker-grad-white': grad('#fff6cf', '#f0d774', '#d9b43e', '#b8902a'),
      '--checker-grad-black': grad('#bfe0ff', '#5aa0e0', '#2e6fc0', '#1d4f99'),
      '--chip-white-edge': '#b58f2c',
      '--chip-black-edge': '#1d4f99',
      '--chip-white-solid': '#e6c24e',
      '--chip-black-solid': '#3a7bd0',
    },
  },
  {
    id: 'coral',
    name: 'Коралл и оникс',
    vars: {
      '--checker-grad-white': grad('#ffd9bf', '#ec9a6a', '#d2703e', '#a8542a'),
      '--checker-grad-black': grad('#6a5a4a', '#3a2c22', '#241a14', '#120c08'),
      '--chip-white-edge': '#a8542a',
      '--chip-black-edge': '#120c08',
      '--chip-white-solid': '#df8a5a',
      '--chip-black-solid': '#2b2018',
    },
  },
  {
    id: 'azure',
    name: 'Лазурь и орех',
    vars: {
      '--checker-grad-white': grad('#bfe0ff', '#5aa0e0', '#2e6fc0', '#1d4f99'),
      '--checker-grad-black': grad('#8a6f55', '#5a4030', '#3a2818', '#221408'),
      '--chip-white-edge': '#1d4f99',
      '--chip-black-edge': '#221408',
      '--chip-white-solid': '#3a7bd0',
      '--chip-black-solid': '#4a3424',
    },
  },
];

const KEY = 'opornik.theme.v1';
const DEFAULT: ThemeChoice = { board: 'emerald', checkers: 'ivory' };

export function getTheme(): ThemeChoice {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULT };
    const t = JSON.parse(raw);
    return {
      board: BOARD_THEMES.some((b) => b.id === t.board) ? t.board : DEFAULT.board,
      checkers: CHECKER_THEMES.some((c) => c.id === t.checkers) ? t.checkers : DEFAULT.checkers,
    };
  } catch {
    return { ...DEFAULT };
  }
}

export function setTheme(t: ThemeChoice): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(t));
  } catch {
    /* best-effort */
  }
}

/** Merge the chosen board + checker themes into one CSS-variable map. */
export function themeVars(t: ThemeChoice): Record<string, string> {
  const board = BOARD_THEMES.find((b) => b.id === t.board) ?? BOARD_THEMES[0];
  const checkers = CHECKER_THEMES.find((c) => c.id === t.checkers) ?? CHECKER_THEMES[0];
  return { ...board.vars, ...checkers.vars };
}

/** As an inline `style` string for an element. */
export function themeStyle(t: ThemeChoice): string {
  return Object.entries(themeVars(t))
    .map(([k, v]) => `${k}:${v}`)
    .join(';');
}
