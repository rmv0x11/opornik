// Tiny synthesized sound effects (WebAudio, no assets — nothing to fetch under
// the GitHub Pages base path, so no 404/console-error risk). Every entry point
// is fail-silent: audio being blocked or unavailable must never break the game
// or log a console error (the preview/live smoke tests assert zero of them).

let ctx: AudioContext | null = null;
let master: GainNode | null = null;
let enabled = true;

export function setSoundEnabled(on: boolean) {
  enabled = on;
}

function ac(): AudioContext | null {
  try {
    if (!ctx) {
      const AC = window.AudioContext ?? (window as any).webkitAudioContext;
      if (!AC) return null;
      ctx = new AC();
      master = ctx.createGain();
      master.gain.value = 0.22;
      master.connect(ctx.destination);
    }
    // Autoplay policy: contexts start suspended until a user gesture; our sounds
    // always follow clicks, so a resume here is allowed (and harmless if not).
    if (ctx.state === 'suspended') void ctx.resume().catch(() => {});
    return ctx;
  } catch {
    return null;
  }
}

/** One decaying tone. `when` is an offset in seconds from now. */
function tone(freq: number, dur: number, peak: number, when = 0, type: OscillatorType = 'sine') {
  const c = ac();
  if (!c || !master) return;
  try {
    const t0 = c.currentTime + when;
    const osc = c.createOscillator();
    const g = c.createGain();
    osc.type = type;
    osc.frequency.setValueAtTime(freq, t0);
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(peak, t0 + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
    osc.connect(g).connect(master);
    osc.start(t0);
    osc.stop(t0 + dur + 0.02);
  } catch {
    /* audio unavailable — stay silent */
  }
}

/** Short band-passed noise burst — the dice "rattle" component. */
function rattle(when: number, dur: number, freq: number, peak: number) {
  const c = ac();
  if (!c || !master) return;
  try {
    const t0 = c.currentTime + when;
    const n = Math.max(1, Math.floor(c.sampleRate * dur));
    const buf = c.createBuffer(1, n, c.sampleRate);
    const data = buf.getChannelData(0);
    for (let i = 0; i < n; i++) data[i] = (Math.random() * 2 - 1) * (1 - i / n);
    const src = c.createBufferSource();
    src.buffer = buf;
    const bp = c.createBiquadFilter();
    bp.type = 'bandpass';
    bp.frequency.value = freq;
    bp.Q.value = 1.2;
    const g = c.createGain();
    g.gain.value = peak;
    src.connect(bp).connect(g).connect(master);
    src.start(t0);
  } catch {
    /* audio unavailable — stay silent */
  }
}

export const sfx = {
  /** Dice throw: two quick rattles, like dice tumbling onto felt. */
  dice() {
    if (!enabled) return;
    rattle(0, 0.05, 2600, 0.5);
    rattle(0.07, 0.07, 2100, 0.4);
  },
  /** Checker landing on a point — a soft wooden "tok". */
  move() {
    if (!enabled) return;
    tone(640, 0.06, 0.5);
    rattle(0, 0.015, 3400, 0.18);
  },
  /** Bear-off into the tray — a brighter double tap. */
  bear() {
    if (!enabled) return;
    tone(720, 0.06, 0.45);
    tone(980, 0.08, 0.4, 0.07);
  },
  /** Game won — a little rising arpeggio. */
  win() {
    if (!enabled) return;
    tone(523, 0.16, 0.4, 0, 'triangle');
    tone(659, 0.16, 0.4, 0.12, 'triangle');
    tone(784, 0.28, 0.45, 0.24, 'triangle');
  },
  /** Game lost — two muted falling notes. */
  lose() {
    if (!enabled) return;
    tone(392, 0.18, 0.35, 0, 'triangle');
    tone(294, 0.3, 0.35, 0.16, 'triangle');
  },
};
