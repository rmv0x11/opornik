// TypeScript mirror of the engine-wasm DTOs (crates/engine-wasm/src/dto.rs).
// All structured values cross the worker boundary as these shapes.

export type PlayerColor = 'white' | 'black';
export type VariantId = 'traditional' | 'nardegammon' | 'classic' | 'hachapuri';
export type CubeActionId = 'no_double' | 'double_take' | 'double_pass' | 'too_good';

export interface PointDto {
  pos: number; // 1..24, 24 = head, 1..6 = home
  player: PlayerColor;
  count: number;
}

export interface CubeDto {
  value: number;
  owner: PlayerColor | null;
  turned: boolean;
}

export interface OutcomeDto {
  kind: 'ongoing' | 'win';
  winner: PlayerColor | null;
  mars: boolean | null;
  points: number | null;
}

export interface PositionDto {
  variant: VariantId;
  turn: PlayerColor;
  dice: [number, number] | null;
  white: PointDto[];
  black: PointDto[];
  off: [number, number];
  pip: [number, number];
  first_turn_done: [boolean, boolean];
  turn_number: number;
  cube: CubeDto;
  crawford: boolean;
  outcome: OutcomeDto;
}

export interface CheckerMoveDto {
  from: number;
  die: number;
  to: number; // 0 = bear off
  bear_off: boolean;
}

export interface TurnDto {
  id: number;
  is_pass: boolean;
  moves: CheckerMoveDto[];
}

export interface ProbabilitiesDto {
  win: number;
  win_mars: number;
  lose: number;
  lose_mars: number;
}

export interface RankedTurnDto {
  turn: TurnDto;
  equity: number;
  win: number;
  equity_loss: number;
}

export interface BestMoveDto {
  turn: TurnDto;
  equity: number;
  probs: ProbabilitiesDto;
}

export interface CubeDecisionDto {
  action: CubeActionId;
  equity: number;
  opponent_should_take: boolean;
  recommend_beaver: boolean;
  note: string;
  probs: ProbabilitiesDto;
}

export interface SetupDto {
  variant: VariantId;
  turn: PlayerColor;
  white: PointDto[];
  black: PointDto[];
  off: [number, number];
  dice: [number, number] | null;
  cube: CubeDto | null;
  crawford: boolean | null;
}
