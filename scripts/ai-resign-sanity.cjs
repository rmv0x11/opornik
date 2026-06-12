// Sanity check for the AI-resign thresholds in Play.svelte (aiResignIfHopeless):
//   оин resign:  mover has ≥1 checker off  && win < 0.005
//   марс resign: mover has 0 checkers off && win < 0.001 && lose_mars > 0.998
// Verifies the engine's winProbabilities() (mover's perspective) crosses the
// thresholds in genuinely dead positions and stays clear of them in alive ones.
const assert = require('node:assert');
const { Engine } = require('../crates/engine-wasm/pkg-node/engine_wasm.js');

function probsFor(setup) {
  const e = new Engine(setup.variant);
  e.setPosition(JSON.stringify(setup));
  return JSON.parse(e.winProbabilities());
}
const base = { variant: 'traditional', off: [0, 0], dice: null, cube: null, crawford: null };

// 1) Dead race, mars impossible (mover black has 1 off): white is one trivial
//    roll from finishing; black still has 10 checkers far from home.
let p = probsFor({
  ...base,
  turn: 'black',
  white: [{ pos: 1, player: 'white', count: 1 }],
  black: [{ pos: 20, player: 'black', count: 10 }],
  off: [14, 5],
});
console.log(`dead-oin   : win=${p.win.toFixed(5)} lose_mars=${p.lose_mars.toFixed(5)}`);
assert.ok(p.win < 0.005, `dead oin position must cross the resign threshold (win=${p.win})`);

// 2) Mars-certain: black (mover) has NOTHING off and all checkers at the start;
//    white bears its last checker off on its next roll, guaranteed.
p = probsFor({
  ...base,
  turn: 'black',
  white: [{ pos: 1, player: 'white', count: 1 }],
  black: [{ pos: 24, player: 'black', count: 15 }],
  off: [14, 0],
});
console.log(`dead-mars  : win=${p.win.toFixed(5)} lose_mars=${p.lose_mars.toFixed(5)}`);
assert.ok(p.win < 0.001 && p.lose_mars > 0.998, `certain mars must cross the mars-resign threshold`);

// 3) Mars threatened but SAVABLE: black has none off yet, but its checkers are
//    on the bear-off doorstep while white still needs several rolls. The AI
//    must NOT concede a mars here.
p = probsFor({
  ...base,
  turn: 'black',
  white: [{ pos: 3, player: 'white', count: 4 }],
  black: [{ pos: 1, player: 'black', count: 15 }],
  off: [11, 0],
});
console.log(`savable    : win=${p.win.toFixed(5)} lose_mars=${p.lose_mars.toFixed(5)}`);
assert.ok(!(p.win < 0.001 && p.lose_mars > 0.998), 'savable mars must NOT trigger mars resign');

// 4) Losing but alive race (~couple of rolls behind): must NOT trigger оин resign.
p = probsFor({
  ...base,
  turn: 'black',
  white: [{ pos: 2, player: 'white', count: 6 }],
  black: [{ pos: 3, player: 'black', count: 6 }],
  off: [9, 9],
});
console.log(`behind-race: win=${p.win.toFixed(5)} lose_mars=${p.lose_mars.toFixed(5)}`);
assert.ok(p.win >= 0.005, `losing-but-alive race must stay above the oin threshold (win=${p.win})`);

// 5) Starting position: nowhere near any threshold.
const e = new Engine('traditional');
p = JSON.parse(e.winProbabilities());
console.log(`start      : win=${p.win.toFixed(5)} lose_mars=${p.lose_mars.toFixed(5)}`);
assert.ok(p.win > 0.3 && p.win < 0.7, 'start position is balanced');

console.log('\nAI-RESIGN SANITY: ALL PASSED');
