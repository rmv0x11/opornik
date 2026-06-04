// Geometry: TS mirror of the engine's phys/pos mapping + screen-slot layout.
// The board view works in physical cell space (0..23) for rendering and path
// positions (1..24) for moves. Orientation is reasoned about only here.

export type PlayerColor = 'white' | 'black';

export const N_POINTS = 24;
export const HEAD_POS = 24;
export const HOME_HIGH = 6;
export const N_CHECKERS = 15;

/** Physical cell (0..23) of `player`'s path position `pos` (1..24). */
export function phys(p: PlayerColor, pos: number): number {
  const base = p === 'white' ? 24 : 36;
  return (((base - pos) % 24) + 24) % 24;
}

/** Inverse of phys: path position (1..24) of a physical cell for `player`. */
export function posOfPhys(p: PlayerColor, cell: number): number {
  const base = p === 'white' ? 24 : 36;
  const v = (((base - cell) % 24) + 24) % 24;
  return v === 0 ? HEAD_POS : v;
}

/** slot (0..23) -> physical cell, for a given viewer orientation. Slot 0 is the
 *  oriented player's 1-point (bottom-right), slot 23 their head (top-right). */
export function buildSlotToCell(orientation: PlayerColor): number[] {
  return Array.from({ length: 24 }, (_, s) => phys(orientation, s + 1));
}
