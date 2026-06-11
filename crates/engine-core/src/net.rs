//! The evaluation network.
//!
//! A small multilayer perceptron — input → one tanh hidden layer → three sigmoid
//! outputs — in the TD-Gammon lineage. Outputs are, from the side-to-move's
//! perspective: `P(win)`, `P(win is a mars)`, `P(loss is a mars)`.
//!
//! Everything is plain `f32` math with no dependencies, so the identical forward
//! pass runs natively (training, server analysis) and in the browser (WASM). The
//! semi-gradient TD update [`Net::train_step`] is included here too (it is just
//! arithmetic); the self-play *orchestration* lives in the `engine-train` crate.

use crate::analysis::Probabilities;
use crate::board::Board;
use crate::encoding::{encode_into, INPUT_SIZE};
use crate::player::Player;
use crate::search::Evaluator;

/// Number of network outputs: win, win-mars, lose-mars.
pub const OUTPUTS: usize = 3;

/// A one-hidden-layer perceptron with weights stored as flat row-major vectors.
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub input: usize,
    pub hidden: usize,
    pub output: usize,
    /// `hidden × input`.
    pub w1: Vec<f32>,
    pub b1: Vec<f32>,
    /// `output × hidden`.
    pub w2: Vec<f32>,
    pub b2: Vec<f32>,
}

#[inline]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Dot product with 8 independent accumulator lanes: a single scalar
/// accumulator is a loop-carried FP dependency the compiler may not reorder,
/// so it serializes; explicit lanes hand it the reassociation and the loop
/// compiles to packed FMAs (NEON/SSE). ~3-4× faster on rows 80..300 wide —
/// this is the hot path of play, search and rollout labelling alike.
#[inline]
pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let ca = a.chunks_exact(8);
    let cb = b.chunks_exact(8);
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (xa, xb) in ca.zip(cb) {
        for k in 0..8 {
            // mul_add needs hardware FMA: on wasm32 it lowers to a SOFTWARE
            // fma call (exact-rounding contract) and would cripple the
            // browser engine — plain mul+add still vectorizes everywhere.
            #[cfg(target_arch = "wasm32")]
            {
                acc[k] += xa[k] * xb[k];
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                acc[k] = xa[k].mul_add(xb[k], acc[k]);
            }
        }
    }
    let mut s = ((acc[0] + acc[4]) + (acc[1] + acc[5])) + ((acc[2] + acc[6]) + (acc[3] + acc[7]));
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

impl Net {
    /// All-zero network of the given shape (predicts 0.5 everywhere).
    pub fn zeros(input: usize, hidden: usize, output: usize) -> Net {
        Net {
            input,
            hidden,
            output,
            w1: vec![0.0; hidden * input],
            b1: vec![0.0; hidden],
            w2: vec![0.0; output * hidden],
            b2: vec![0.0; output],
        }
    }

    /// Randomly initialised network (scaled for tanh hidden units), seeded for
    /// reproducibility. Uses an internal xorshift PRNG (no external dependency).
    pub fn random(input: usize, hidden: usize, output: usize, seed: u64) -> Net {
        let mut state = seed | 1;
        let mut next = || -> f32 {
            // xorshift64* → uniform in [-1, 1)
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let u = (state >> 11) as f32 / (1u64 << 53) as f32; // [0,1)
            u * 2.0 - 1.0
        };
        let r1 = 1.0 / (input as f32).sqrt();
        let r2 = 1.0 / (hidden as f32).sqrt();
        let mut net = Net::zeros(input, hidden, output);
        for w in net.w1.iter_mut() {
            *w = next() * r1;
        }
        for w in net.w2.iter_mut() {
            *w = next() * r2;
        }
        net
    }

    /// The standard-shape network for long nardy: 196 → `hidden` → 3.
    pub fn standard(hidden: usize, seed: u64) -> Net {
        Net::random(INPUT_SIZE, hidden, OUTPUTS, seed)
    }

    /// Forward pass, returning the output activations.
    pub fn forward(&self, x: &[f32]) -> Vec<f32> {
        self.forward_full(x).1
    }

    /// Forward pass returning `(hidden activations, outputs)` for training.
    fn forward_full(&self, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
        debug_assert_eq!(x.len(), self.input);
        let mut hidden = vec![0.0f32; self.hidden];
        for (j, h) in hidden.iter_mut().enumerate() {
            let row = &self.w1[j * self.input..(j + 1) * self.input];
            *h = (self.b1[j] + dot(row, x)).tanh();
        }
        let mut out = vec![0.0f32; self.output];
        for (k, o) in out.iter_mut().enumerate() {
            let row = &self.w2[k * self.hidden..(k + 1) * self.hidden];
            *o = sigmoid(self.b2[k] + dot(row, &hidden));
        }
        (hidden, out)
    }

    /// One semi-gradient TD update: nudge the outputs toward `target`, returning
    /// the pre-update squared error. `target` is held constant (TD bootstrap).
    #[allow(clippy::needless_range_loop)] // explicit indexing reads clearer for backprop kernels
    pub fn train_step(&mut self, x: &[f32], target: &[f32], lr: f32) -> f32 {
        debug_assert_eq!(target.len(), self.output);
        let (hidden, out) = self.forward_full(x);

        let mut g2 = vec![0.0f32; self.output];
        let mut loss = 0.0;
        for k in 0..self.output {
            let e = target[k] - out[k];
            loss += 0.5 * e * e;
            g2[k] = e * out[k] * (1.0 - out[k]); // × sigmoid'(z2)
        }

        let mut g1 = vec![0.0f32; self.hidden];
        for j in 0..self.hidden {
            let mut s = 0.0;
            for k in 0..self.output {
                s += g2[k] * self.w2[k * self.hidden + j];
            }
            g1[j] = s * (1.0 - hidden[j] * hidden[j]); // × tanh'(z1)
        }

        // Gradient ascent on value-match (params move toward reducing the error).
        for k in 0..self.output {
            let base = k * self.hidden;
            for j in 0..self.hidden {
                self.w2[base + j] += lr * g2[k] * hidden[j];
            }
            self.b2[k] += lr * g2[k];
        }
        for j in 0..self.hidden {
            let base = j * self.input;
            let gj = lr * g1[j];
            for i in 0..self.input {
                self.w1[base + i] += gj * x[i];
            }
            self.b1[j] += gj;
        }
        loss
    }

    /// Evaluate a board from `mover`'s perspective into outcome probabilities.
    pub fn evaluate_board(&self, board: &Board, mover: Player) -> Probabilities {
        let mut x = vec![0.0f32; self.input];
        encode_into(board, mover, &mut x);
        let o = self.forward(&x);
        let win = o[0].clamp(0.0, 1.0);
        Probabilities {
            win,
            win_mars: o[1].clamp(0.0, win),
            lose_mars: o[2].clamp(0.0, 1.0 - win),
        }
    }

    /// Serialize to a self-describing little-endian byte blob.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for &dim in &[self.input, self.hidden, self.output] {
            out.extend_from_slice(&(dim as u32).to_le_bytes());
        }
        for v in self
            .w1
            .iter()
            .chain(&self.b1)
            .chain(&self.w2)
            .chain(&self.b2)
        {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Deserialize from [`Net::to_bytes`]. Returns `None` if malformed.
    pub fn from_bytes(bytes: &[u8]) -> Option<Net> {
        if bytes.len() < 12 {
            return None;
        }
        let rd_u32 = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let input = rd_u32(&bytes[0..4]);
        let hidden = rd_u32(&bytes[4..8]);
        let output = rd_u32(&bytes[8..12]);
        let count = hidden * input + hidden + output * hidden + output;
        let mut floats = Vec::with_capacity(count);
        let body = &bytes[12..];
        if body.len() != count * 4 {
            return None;
        }
        for chunk in body.chunks_exact(4) {
            floats.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        let mut it = floats.into_iter();
        let mut take = |n: usize| (&mut it).take(n).collect::<Vec<_>>();
        Some(Net {
            input,
            hidden,
            output,
            w1: take(hidden * input),
            b1: take(hidden),
            w2: take(output * hidden),
            b2: take(output),
        })
    }
}

impl Evaluator for Net {
    fn equity(&self, board: &Board, mover: Player) -> f32 {
        self.evaluate_board(board, mover).cubeless_equity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_is_deterministic_and_in_range() {
        let net = Net::standard(32, 42);
        let b = Board::starting();
        let p = net.evaluate_board(&b, Player::White);
        assert!((0.0..=1.0).contains(&p.win));
        assert!(p.win_mars <= p.win + 1e-6);
        // deterministic
        assert_eq!(net.evaluate_board(&b, Player::White), p);
    }

    #[test]
    fn train_step_reduces_error_toward_a_fixed_target() {
        let mut net = Net::standard(16, 7);
        let b = Board::starting();
        let x = crate::encoding::encode(&b, Player::White);
        let target = [1.0, 0.0, 0.0];
        let first = net.train_step(&x, &target, 0.1);
        for _ in 0..200 {
            net.train_step(&x, &target, 0.1);
        }
        let last = net.train_step(&x, &target, 0.1);
        assert!(last < first, "loss should decrease ({last} !< {first})");
        assert!(net.forward(&x)[0] > 0.9, "should learn to predict a win");
    }

    #[test]
    fn serialization_round_trips() {
        let net = Net::standard(24, 123);
        let bytes = net.to_bytes();
        let back = Net::from_bytes(&bytes).expect("valid");
        assert_eq!(net, back);
    }
}
