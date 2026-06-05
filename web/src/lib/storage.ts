// Saved games: full state needed to resume a игра from any position, persisted
// in localStorage, plus a portable export/import string for sharing a position.
import type { PlayerColor, SetupDto, VariantId } from '../engine/types';

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
    g.v = 1;
    return g;
  } catch {
    return null;
  }
}
