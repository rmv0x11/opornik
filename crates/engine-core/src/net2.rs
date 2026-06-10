//! Evaluation network **v2**: multi-layer MLP with a 4-way softmax head.
//!
//! Architecture (wildbg-style): input → ReLU hidden layers (default
//! 300-250-200) → softmax over the 4 outcome classes
//! `[win_oin, win_mars, lose_oin, lose_mars]`, all from the side-to-move's
//! perspective. Trained in PyTorch on rollout soft-labels (CrossEntropy);
//! this module only needs the forward pass — plus (de)serialization shared
//! byte-for-byte with the Python exporter.
//!
//! # Binary format (little-endian)
//!
//! ```text
//! NetV2:   b"NNV2"  u32 version=1  u32 n_layers
//!          (u32 in, u32 out) × n_layers
//!          per layer: f32 weights[out×in] (row-major), f32 bias[out]
//! PhaseNets: b"NV2P" u32 version=1
//!            u32 contact_len, NetV2 bytes (contact)
//!            u32 race_len,    NetV2 bytes (race)
//! ```
//!
//! `train/train_v2.py` writes exactly this layout — change them together.

use crate::analysis::Probabilities;
use crate::board::Board;
use crate::encoding2::{encode_contact_into, encode_race_into, CONTACT_INPUTS, RACE_INPUTS};
use crate::phase::has_contact;
use crate::player::Player;
use crate::search::Evaluator;

/// Outcome classes: win-oin, win-mars, lose-oin, lose-mars.
pub const OUTPUTS_V2: usize = 4;

const NET_MAGIC: &[u8; 4] = b"NNV2";
const PAIR_MAGIC: &[u8; 4] = b"NV2P";
const VERSION: u32 = 1;

/// One dense layer: `out × in` row-major weights + bias.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub input: usize,
    pub output: usize,
    pub w: Vec<f32>,
    pub b: Vec<f32>,
}

/// A multi-layer perceptron: ReLU hidden layers, softmax output.
#[derive(Clone, Debug, PartialEq)]
pub struct NetV2 {
    pub layers: Vec<Layer>,
}

impl NetV2 {
    /// Input dimension (of the first layer).
    pub fn input(&self) -> usize {
        self.layers.first().map_or(0, |l| l.input)
    }

    /// Output dimension (of the last layer).
    pub fn output(&self) -> usize {
        self.layers.last().map_or(0, |l| l.output)
    }

    /// Randomly initialised net (He-scaled for ReLU) — useful for shape tests
    /// and speed benchmarks; real weights come from PyTorch.
    pub fn random(dims: &[usize], seed: u64) -> NetV2 {
        let mut state = seed | 1;
        let mut next = || -> f32 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 11) as f32 / (1u64 << 53) as f32) * 2.0 - 1.0
        };
        let layers = dims
            .windows(2)
            .map(|d| {
                let (input, output) = (d[0], d[1]);
                let r = (2.0 / input as f32).sqrt();
                Layer {
                    input,
                    output,
                    w: (0..input * output).map(|_| next() * r).collect(),
                    b: vec![0.0; output],
                }
            })
            .collect();
        NetV2 { layers }
    }

    /// Forward pass → softmax probabilities (`output()` floats, sum to 1).
    pub fn forward(&self, x: &[f32]) -> Vec<f32> {
        debug_assert_eq!(x.len(), self.input());
        let last = self.layers.len() - 1;
        let mut cur: Vec<f32> = x.to_vec();
        for (li, layer) in self.layers.iter().enumerate() {
            let mut next = vec![0.0f32; layer.output];
            for (o, out) in next.iter_mut().enumerate() {
                let row = &layer.w[o * layer.input..(o + 1) * layer.input];
                let mut z = layer.b[o];
                for (wi, xi) in row.iter().zip(&cur) {
                    z += wi * xi;
                }
                *out = if li < last { z.max(0.0) } else { z };
            }
            cur = next;
        }
        // softmax (max-shifted for numerical stability)
        let m = cur.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        let mut sum = 0.0f32;
        for v in cur.iter_mut() {
            *v = (*v - m).exp();
            sum += *v;
        }
        for v in cur.iter_mut() {
            *v /= sum;
        }
        cur
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(NET_MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.layers.len() as u32).to_le_bytes());
        for l in &self.layers {
            out.extend_from_slice(&(l.input as u32).to_le_bytes());
            out.extend_from_slice(&(l.output as u32).to_le_bytes());
        }
        for l in &self.layers {
            for v in l.w.iter().chain(&l.b) {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<NetV2> {
        let rd_u32 = |b: &[u8]| -> Option<u32> {
            Some(u32::from_le_bytes(b.get(0..4)?.try_into().ok()?))
        };
        if bytes.get(0..4)? != NET_MAGIC || rd_u32(&bytes[4..])? != VERSION {
            return None;
        }
        let n = rd_u32(&bytes[8..])? as usize;
        if n == 0 || n > 64 {
            return None;
        }
        let mut dims = Vec::with_capacity(n);
        let mut off = 12;
        for _ in 0..n {
            let input = rd_u32(bytes.get(off..)?)? as usize;
            let output = rd_u32(bytes.get(off + 4..)?)? as usize;
            if input == 0 || output == 0 || input > 4096 || output > 4096 {
                return None;
            }
            dims.push((input, output));
            off += 8;
        }
        // consecutive layers must chain
        if dims.windows(2).any(|d| d[0].1 != d[1].0) {
            return None;
        }
        let total: usize = dims.iter().map(|&(i, o)| i * o + o).sum();
        if bytes.len() != off + total * 4 {
            return None;
        }
        let mut layers = Vec::with_capacity(n);
        for (input, output) in dims {
            let mut take = |count: usize| -> Vec<f32> {
                let v = bytes[off..off + count * 4]
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                off += count * 4;
                v
            };
            let w = take(input * output);
            let b = take(output);
            layers.push(Layer { input, output, w, b });
        }
        Some(NetV2 { layers })
    }
}

/// The v2 evaluator: contact net / race net, dispatched by [`has_contact`].
/// (When both sides are all home the exact bear-off table applies — that
/// dispatch stays in [`crate::search::Composite`]-style wrappers; the race net
/// still covers all-home positions as a fallback.)
#[derive(Clone, Debug, PartialEq)]
pub struct PhaseNets {
    pub contact: NetV2,
    pub race: NetV2,
}

impl PhaseNets {
    /// Validate shapes: contact 217→…→4, race 196→…→4.
    pub fn new(contact: NetV2, race: NetV2) -> Option<PhaseNets> {
        if contact.input() != CONTACT_INPUTS
            || race.input() != RACE_INPUTS
            || contact.output() != OUTPUTS_V2
            || race.output() != OUTPUTS_V2
        {
            return None;
        }
        Some(PhaseNets { contact, race })
    }

    /// Outcome probabilities `[win_oin, win_mars, lose_oin, lose_mars]` from
    /// `mover`'s perspective.
    pub fn probs(&self, board: &Board, mover: Player) -> [f32; 4] {
        let out = if has_contact(board) {
            let mut x = vec![0.0f32; CONTACT_INPUTS];
            encode_contact_into(board, mover, &mut x);
            self.contact.forward(&x)
        } else {
            let mut x = vec![0.0f32; RACE_INPUTS];
            encode_race_into(board, mover, &mut x);
            self.race.forward(&x)
        };
        [out[0], out[1], out[2], out[3]]
    }

    /// Map to the 3-output [`Probabilities`] the UI/analysis layer expects.
    pub fn evaluate_board(&self, board: &Board, mover: Player) -> Probabilities {
        let p = self.probs(board, mover);
        Probabilities {
            win: (p[0] + p[1]).clamp(0.0, 1.0),
            win_mars: p[1],
            lose_mars: p[3],
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let (c, r) = (self.contact.to_bytes(), self.race.to_bytes());
        let mut out = Vec::with_capacity(12 + c.len() + r.len());
        out.extend_from_slice(PAIR_MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(c.len() as u32).to_le_bytes());
        out.extend_from_slice(&c);
        out.extend_from_slice(&(r.len() as u32).to_le_bytes());
        out.extend_from_slice(&r);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<PhaseNets> {
        if bytes.get(0..4)? != PAIR_MAGIC {
            return None;
        }
        let rd_u32 = |b: &[u8]| -> Option<u32> {
            Some(u32::from_le_bytes(b.get(0..4)?.try_into().ok()?))
        };
        if rd_u32(&bytes[4..])? != VERSION {
            return None;
        }
        let clen = rd_u32(&bytes[8..])? as usize;
        let contact = NetV2::from_bytes(bytes.get(12..12usize.checked_add(clen)?)?)?;
        let roff = 12 + clen;
        let rlen = rd_u32(bytes.get(roff..)?)? as usize;
        if bytes.len() != roff + 4 + rlen {
            return None;
        }
        let race = NetV2::from_bytes(bytes.get(roff + 4..)?)?;
        PhaseNets::new(contact, race)
    }
}

/// Cubeless points equity of the 4-class probability vector.
#[inline]
pub fn probs4_equity(p: [f32; 4]) -> f32 {
    p[0] + 2.0 * p[1] - p[2] - 2.0 * p[3]
}

impl Evaluator for PhaseNets {
    fn equity(&self, board: &Board, mover: Player) -> f32 {
        probs4_equity(self.probs(board, mover))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pair() -> PhaseNets {
        PhaseNets::new(
            NetV2::random(&[CONTACT_INPUTS, 300, 250, 200, OUTPUTS_V2], 11),
            NetV2::random(&[RACE_INPUTS, 300, 250, 200, OUTPUTS_V2], 22),
        )
        .expect("valid shapes")
    }

    #[test]
    fn forward_is_a_distribution() {
        let nets = test_pair();
        let b = Board::starting();
        let p = nets.probs(&b, Player::White);
        let sum: f32 = p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "softmax must sum to 1, got {sum}");
        assert!(p.iter().all(|&v| (0.0..=1.0).contains(&v)));
        // deterministic
        assert_eq!(nets.probs(&b, Player::White), p);
    }

    #[test]
    fn dispatch_uses_race_net_when_disengaged() {
        let nets = test_pair();
        let mut race = Board::empty();
        race.place(Player::White, 8, 15);
        race.place(Player::Black, 8, 15);
        // Same board through the race net directly must equal the dispatcher.
        let mut x = vec![0.0f32; RACE_INPUTS];
        encode_race_into(&race, Player::White, &mut x);
        let direct = nets.race.forward(&x);
        assert_eq!(nets.probs(&race, Player::White).to_vec(), direct);
        // And the contact board must NOT take that path (different input dims
        // would panic in debug if misrouted; check probs differ from race net's
        // view of an engaged board only via the public API).
        assert!(crate::phase::has_contact(&Board::starting()));
    }

    #[test]
    fn serialization_round_trips() {
        let nets = test_pair();
        let solo = NetV2::from_bytes(&nets.contact.to_bytes()).expect("netv2 round-trip");
        assert_eq!(solo, nets.contact);
        let pair = PhaseNets::from_bytes(&nets.to_bytes()).expect("pair round-trip");
        assert_eq!(pair, nets);
        // corrupt: truncated and wrong magic
        assert!(NetV2::from_bytes(&nets.contact.to_bytes()[..40]).is_none());
        assert!(PhaseNets::from_bytes(b"XXXXjunk").is_none());
    }

    #[test]
    fn evaluator_equity_is_consistent_with_probs() {
        let nets = test_pair();
        let b = Board::starting();
        let p = nets.probs(&b, Player::White);
        let eq = probs4_equity(p);
        assert!((Evaluator::equity(&nets, &b, Player::White) - eq).abs() < 1e-6);
        assert!((-2.0..=2.0).contains(&eq));
    }
}
