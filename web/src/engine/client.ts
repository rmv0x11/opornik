// Promise-per-id RPC wrapper around the engine Web Worker. Exposes a typed async
// API to the Svelte app; all engine compute happens off the UI thread.

import type {
  BestMoveDto,
  CubeDecisionDto,
  PlayerColor,
  PositionDto,
  ProbabilitiesDto,
  RankedTurnDto,
  SequenceDto,
  SetupDto,
  TurnDto,
} from './types';

type Pending = { resolve: (v: unknown) => void; reject: (e: unknown) => void };

class EngineClient {
  private worker: Worker;
  private seq = 0;
  private pending = new Map<number, Pending>();

  constructor() {
    this.worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    this.worker.onmessage = (e: MessageEvent) => {
      const { id, ok, err } = e.data;
      const p = this.pending.get(id);
      if (!p) return;
      this.pending.delete(id);
      if (err !== undefined) p.reject(new Error(err));
      else p.resolve(ok);
    };
  }

  private call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const id = ++this.seq;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ id, cmd, args });
    });
  }

  init(variant: string) {
    return this.call<PositionDto>('init', { variant });
  }
  setDice(d1: number, d2: number) {
    return this.call<null>('setDice', { d1, d2 });
  }
  legalTurns() {
    return this.call<TurnDto[]>('legalTurns');
  }
  legalSequences() {
    return this.call<SequenceDto[]>('legalSequences');
  }
  applyTurn(id: number) {
    return this.call<PositionDto>('applyTurn', { id });
  }
  bestMove(ply: number) {
    return this.call<BestMoveDto>('bestMove', { ply });
  }
  analyze(ply: number) {
    return this.call<RankedTurnDto[]>('analyze', { ply });
  }
  winProbabilities() {
    return this.call<ProbabilitiesDto>('winProbabilities');
  }
  equity(ply: number) {
    return this.call<number>('equity', { ply });
  }
  cubeDecision() {
    return this.call<CubeDecisionDto>('cubeDecision');
  }
  offerDouble() {
    return this.call<PositionDto>('offerDouble');
  }
  beaver() {
    return this.call<PositionDto>('beaver');
  }
  setPosition(setup: SetupDto) {
    // Strip any Svelte $state proxies so the object is structured-cloneable
    // across the worker boundary.
    return this.call<PositionDto>('setPosition', { setup: JSON.parse(JSON.stringify(setup)) });
  }
  getPosition() {
    return this.call<PositionDto>('getPosition');
  }
  reset() {
    return this.call<PositionDto>('reset');
  }
  setTurn(color: PlayerColor) {
    return this.call<PositionDto>('setTurn', { color });
  }
  setCrawford(on: boolean) {
    return this.call<PositionDto>('setCrawford', { on });
  }
}

export const engine = new EngineClient();
