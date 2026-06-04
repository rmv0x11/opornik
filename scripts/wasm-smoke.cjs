// Runtime smoke test of the engine-wasm wrapper (Node target): exercises the
// full JS-facing API and plays a complete game to confirm it works end-to-end.
const assert = require('node:assert');
const { Engine } = require('../crates/engine-wasm/pkg-node/engine_wasm.js');

function die() {
  return 1 + Math.floor(Math.random() * 6);
}

// 1) construction + starting position
const e = new Engine('traditional');
let pos = JSON.parse(e.getPosition());
const sum = (pts) => pts.reduce((a, p) => a + p.count, 0);
assert.strictEqual(sum(pos.white) + pos.off[0], 15, 'white has 15');
assert.strictEqual(sum(pos.black) + pos.off[1], 15, 'black has 15');
assert.strictEqual(pos.turn, 'white');
assert.strictEqual(pos.pip[0], 360);
console.log('ok: start position 15/15, pip 360');

// 2) legal turns + win probabilities
e.setDice(3, 1);
const legal = JSON.parse(e.legalTurns());
assert.ok(legal.length >= 1, 'has legal turns');
const probs = JSON.parse(e.winProbabilities());
assert.ok(probs.win >= 0 && probs.win <= 1, 'win prob in range');
console.log(`ok: 3-1 has ${legal.length} legal play(s), win ${(probs.win * 100).toFixed(1)}%`);

// 3) bestMove + analyze
const bm = JSON.parse(e.bestMove(1));
assert.ok(bm.turn.moves.length >= 1, 'best move has moves');
const ranked = JSON.parse(e.analyze(1));
assert.ok(ranked.length >= 1 && ranked[0].equity_loss === 0, 'ranked best has 0 loss');
console.log(`ok: bestMove eq ${bm.equity.toFixed(3)}, analyze ${ranked.length} ranked`);

// 4) play a full game at 1-ply for both sides; must terminate with a winner
let turns = 0;
while (pos.outcome.kind === 'ongoing' && turns < 2000) {
  e.setDice(die(), die());
  const mv = JSON.parse(e.bestMove(1));
  pos = JSON.parse(e.applyTurn(mv.turn.id));
  turns += 1;
}
assert.strictEqual(pos.outcome.kind, 'win', 'game finished');
console.log(`ok: full game finished in ${turns} turns — ${pos.outcome.winner} wins (${pos.outcome.mars ? 'mars' : 'oin'}, ${pos.outcome.points})`);

// 5) cube decision on a money variant
const h = new Engine('hachapuri');
const cd = JSON.parse(h.cubeDecision());
assert.ok(['no_double', 'double_take', 'double_pass', 'too_good'].includes(cd.action), 'cube action valid');
console.log(`ok: hachapuri cube decision = ${cd.action} (${cd.note})`);

// 6) editor: set an arbitrary valid position
const setup = {
  variant: 'traditional',
  turn: 'white',
  white: [{ pos: 6, player: 'white', count: 15 }],
  black: [{ pos: 6, player: 'black', count: 15 }],
  off: [0, 0],
  dice: [6, 5],
  cube: null,
  crawford: null,
};
const sp = JSON.parse(e.setPosition(JSON.stringify(setup)));
assert.strictEqual(sp.pip[0], 90, 'all-on-6 pip = 90');
console.log('ok: setPosition (editor) accepted a valid 15/15 layout');

console.log('\nALL WASM SMOKE TESTS PASSED');
