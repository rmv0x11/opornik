// Saved games: full state needed to resume a игра from any position, persisted
// in localStorage, plus a portable export/import string for sharing a position.
import type { PlayerColor, PositionDto, SetupDto, VariantId } from '../engine/types';

// One finished game of the match/session — enough to rebuild the «Разбор
// матча» panel after a resume. Worst-move entries keep their board snapshots
// (positions stay viewable); the `ranked` alternatives are dropped as dead
// weight, which is why it's pinned to null here.
export interface SavedGameSummary {
  game: number;
  winnerIsHuman: boolean | null;
  points: number;
  score: [number, number];
  stats: { n: number; best: number; inacc: number; blunder: number; avg: number } | null;
  worst: {
    n: number;
    color: PlayerColor;
    dice: [number, number] | null;
    notation: string;
    win: number | null;
    loss: number | null;
    before: PositionDto;
    ranked: null;
    isHuman: boolean;
  }[];
}

export interface SavedGame {
  v: 1;
  id: string;
  name: string;
  savedAt: string; // ISO timestamp
  variant: VariantId;
  aiPly: number;
  humanColor: PlayerColor;
  matchLength: number | null;
  matchScore: [number, number];
  crawfordPlayed: boolean;
  setup: SetupDto; // board + turn + dice + cube + crawford + turn_number
  playedGames?: SavedGameSummary[]; // finished games so far (absent in old saves)
}

const KEY = 'opornik.saves.v1';
const MAX_SAVES = 50;

function read(): SavedGame[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? (arr as SavedGame[]) : [];
  } catch {
    return [];
  }
}
function write(saves: SavedGame[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(saves.slice(0, MAX_SAVES)));
  } catch {
    /* storage may be full or unavailable; saving is best-effort */
  }
}

/** Saved games, newest first. */
export function listSaves(): SavedGame[] {
  return read().sort((a, b) => b.savedAt.localeCompare(a.savedAt));
}

export function saveGame(g: SavedGame): void {
  const saves = read().filter((s) => s.id !== g.id);
  saves.unshift(g);
  write(saves);
}

export function deleteSave(id: string): void {
  write(read().filter((s) => s.id !== id));
}

export function newId(): string {
  const c = globalThis.crypto;
  if (c && 'randomUUID' in c) return c.randomUUID();
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

/** Compact, copy-pasteable export string (base64 of the JSON). */
export function exportGame(g: SavedGame): string {
  const json = JSON.stringify(g);
  try {
    return 'NARD1:' + btoa(unescape(encodeURIComponent(json)));
  } catch {
    return json;
  }
}

/** Parse an export string (base64 or raw JSON). Returns null if invalid. */
export function importGame(text: string): SavedGame | null {
  const t = text.trim();
  if (!t) return null;
  let json = t;
  const body = t.startsWith('NARD1:') ? t.slice(6) : t;
  if (!body.trim().startsWith('{')) {
    try {
      json = decodeURIComponent(escape(atob(body.trim())));
    } catch {
      return null;
    }
  }
  try {
    const g = JSON.parse(json) as SavedGame;
    const s = g?.setup;
    if (!g || !g.variant || !s || !Array.isArray(s.white) || !Array.isArray(s.black) || !s.turn) {
      return null;
    }
    // tolerate older/partial exports
    g.matchScore = Array.isArray(g.matchScore) ? g.matchScore : [0, 0];
    g.matchLength = g.matchLength ?? null;
    g.crawfordPlayed = !!g.crawfordPlayed;
    g.aiPly = g.aiPly ?? 2;
    g.humanColor = g.humanColor ?? 'white';
    g.id = g.id || newId();
    g.name = g.name || 'импорт';
    g.savedAt = g.savedAt || new Date().toISOString();
    // playedGames comes from an UNTRUSTED paste: the match-review panel
    // dereferences stats/loss without guards, so anything malformed must be
    // dropped or nulled here, and game numbers renumbered (recordGameSummary
    // numbers the next game as length + 1).
    if (Array.isArray(g.playedGames)) {
      g.playedGames = g.playedGames
        .filter((s) => s && typeof s.game === 'number' && Array.isArray(s.score))
        .map((s, i) => {
          const st = s.stats as Record<string, unknown> | null | undefined;
          const statsOk =
            st && ['n', 'best', 'inacc', 'blunder', 'avg'].every((k) => typeof st[k] === 'number');
          return {
            ...s,
            game: i + 1,
            winnerIsHuman: typeof s.winnerIsHuman === 'boolean' ? s.winnerIsHuman : null,
            points: typeof s.points === 'number' ? s.points : 0,
            stats: statsOk ? s.stats : null,
            worst: (Array.isArray(s.worst) ? s.worst : [])
              .filter(
                (w) =>
                  w &&
                  typeof w.loss === 'number' &&
                  typeof w.notation === 'string' &&
                  w.before &&
                  typeof w.before === 'object',
              )
              .map((w) => ({ ...w, dice: Array.isArray(w.dice) ? w.dice : null, ranked: null })),
          };
        });
    } else {
      delete g.playedGames;
    }
    g.v = 1;
    return g;
  } catch {
    return null;
  }
}
