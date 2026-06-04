/// <reference lib="webworker" />
// Web Worker hosting the WASM engine. Receives tagged RPC messages and replies
// with parsed JSON results so the UI thread never blocks on engine compute.

import init, { Engine } from './pkg/engine_wasm.js';
import wasmUrl from './pkg/engine_wasm_bg.wasm?url';

let engine: Engine | null = null;
let wasmReady = false;

self.onmessage = async (e: MessageEvent) => {
  const { id, cmd, args } = e.data;
  try {
    if (cmd === 'init') {
      if (!wasmReady) {
        await init(wasmUrl);
        wasmReady = true;
      }
      // (Re)create the engine for the chosen variant.
      engine = new Engine(args.variant);
      self.postMessage({ id, ok: JSON.parse(engine.getPosition()) });
      return;
    }
    const eng = engine;
    if (!eng) throw new Error('engine not initialized');
    let res: unknown;
    switch (cmd) {
      case 'setDice':
        eng.setDice(args.d1, args.d2);
        res = null;
        break;
      case 'legalTurns':
        res = JSON.parse(eng.legalTurns());
        break;
      case 'applyTurn':
        res = JSON.parse(eng.applyTurn(args.id));
        break;
      case 'bestMove':
        res = JSON.parse(eng.bestMove(args.ply));
        break;
      case 'analyze':
        res = JSON.parse(eng.analyze(args.ply));
        break;
      case 'winProbabilities':
        res = JSON.parse(eng.winProbabilities());
        break;
      case 'equity':
        res = eng.equity(args.ply);
        break;
      case 'cubeDecision':
        res = JSON.parse(eng.cubeDecision());
        break;
      case 'offerDouble':
        res = JSON.parse(eng.offerDouble());
        break;
      case 'beaver':
        res = JSON.parse(eng.beaver());
        break;
      case 'setPosition':
        res = JSON.parse(eng.setPosition(JSON.stringify(args.setup)));
        break;
      case 'getPosition':
        res = JSON.parse(eng.getPosition());
        break;
      case 'reset':
        res = JSON.parse(eng.reset());
        break;
      case 'setCrawford':
        res = JSON.parse(eng.setCrawford(args.on));
        break;
      default:
        throw new Error(`unknown cmd: ${cmd}`);
    }
    self.postMessage({ id, ok: res });
  } catch (err) {
    self.postMessage({ id, err: String(err) });
  }
};
