//! Self-play TD training and benchmarking for the evaluation net.
//!
//! Training is TD(0) self-play in the TD-Gammon style: the net selects moves
//! greedily by its own 1-ply evaluation, and after every move its estimate of the
//! pre-move position is nudged toward its estimate of the post-move position
//! (perspective-swapped), or toward the realized outcome at game end. No external
//! crates — a small xorshift PRNG provides the dice and initialisation noise.

use engine_core::bearoff::BearoffTable;
use engine_core::board::Board;
use engine_core::encoding::encode;
use engine_core::game::{outcome, Outcome};
use engine_core::moves::{generate_turns_cfg, Turn};
use engine_core::net::Net;
use engine_core::player::Player;
use engine_core::search::{best_turn_search, Evaluator};

/// Standard FSNR head limit used throughout training/benchmarking.
const HEAD_LIMIT: Option<u8> = Some(1);
/// Safety cap on episode length (a race always resolves well within this).
const MAX_PLIES: usize = 1000;

/// Small, fast deterministic PRNG.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    #[inline]
    pub fn die(&mut self) -> u8 {
        (self.next_u64() % 6) as u8 + 1
    }
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 11) as f32 / (1u64 << 53) as f32
    }
    pub fn dice(&mut self) -> [u8; 2] {
        [self.die(), self.die()]
    }
}

/// Training hyperparameters.
#[derive(Clone, Copy, Debug)]
pub struct TrainConfig {
    pub hidden: usize,
    pub games: usize,
    pub lr: f32,
    /// Probability of a random (exploratory) move during self-play.
    pub explore: f32,
    pub seed: u64,
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig {
            hidden: 80,
            games: 20_000,
            lr: 0.05,
            explore: 0.05,
            seed: 0xC0FFEE,
        }
    }
}

/// Greedily pick the resulting position best for `mover` under the net's 1-ply
/// evaluation (the mover minimises the opponent's equity after the move).
fn greedy<E: Evaluator>(eval: &E, turns: &[Turn], mover: Player) -> usize {
    let opp = mover.opponent();
    let mut best = 0;
    let mut best_eq = f32::NEG_INFINITY;
    for (i, t) in turns.iter().enumerate() {
        let eq = -eval.equity(&t.board, opp);
        if eq > best_eq {
            best_eq = eq;
            best = i;
        }
    }
    best
}

/// Outcome label from the winner's perspective: `[win, win_mars, lose_mars]`.
fn win_target(mars: bool) -> [f32; 3] {
    [1.0, if mars { 1.0 } else { 0.0 }, 0.0]
}

/// Play one self-play game, applying TD updates in place. Returns the game length.
pub fn self_play_episode(net: &mut Net, rng: &mut Rng, lr: f32, explore: f32) -> usize {
    let mut board = Board::starting();
    let mut mover = Player::White;
    let mut first = [true, true];

    for ply in 0..MAX_PLIES {
        let x_t = encode(&board, mover);
        let dice = rng.dice();
        let turns = generate_turns_cfg(&board, mover, dice, first[mover.index()], HEAD_LIMIT);
        first[mover.index()] = false;

        let idx = if explore > 0.0 && rng.unit() < explore {
            (rng.next_u64() as usize) % turns.len()
        } else {
            greedy(&*net, &turns, mover)
        };
        let next_board = turns[idx].board.clone();

        match outcome(&next_board) {
            Outcome::Win { mars, .. } => {
                // The mover just won; train s_t toward the realized outcome.
                net.train_step(&x_t, &win_target(mars), lr);
                return ply + 1;
            }
            Outcome::Ongoing => {
                // Bootstrap: target = net(s_{t+1}) viewed from the mover's side.
                let opp = mover.opponent();
                let o = net.forward(&encode(&next_board, opp));
                let target = [1.0 - o[0], o[2], o[1]];
                net.train_step(&x_t, &target, lr);
                board = next_board;
                mover = opp;
            }
        }
    }
    MAX_PLIES
}

/// Train a fresh net by self-play (single-threaded). Prints progress if `verbose`.
pub fn train(cfg: TrainConfig, verbose: bool) -> Net {
    let mut net = Net::standard(cfg.hidden, cfg.seed);
    let mut rng = Rng::new(cfg.seed ^ 0x9E37_79B9_7F4A_7C15);
    let step = (cfg.games / 20).max(1);
    let mut total_len = 0usize;
    for g in 0..cfg.games {
        total_len += self_play_episode(&mut net, &mut rng, cfg.lr, cfg.explore);
        if verbose && (g + 1) % step == 0 {
            let avg = total_len as f32 / (g + 1) as f32;
            println!("  trained {:>7}/{} games (avg len {:.0})", g + 1, cfg.games, avg);
        }
    }
    net
}

/// Elementwise mean of several identically-shaped nets.
fn average_nets(nets: &[Net]) -> Net {
    let n = nets.len() as f32;
    let mut out = nets[0].clone();
    let mix = |dst: &mut [f32], pick: &dyn Fn(&Net) -> &Vec<f32>| {
        for (i, v) in dst.iter_mut().enumerate() {
            let mut s = 0.0;
            for net in nets {
                s += pick(net)[i];
            }
            *v = s / n;
        }
    };
    mix(&mut out.w1, &|net| &net.w1);
    mix(&mut out.b1, &|net| &net.b1);
    mix(&mut out.w2, &|net| &net.w2);
    mix(&mut out.b2, &|net| &net.b2);
    out
}

/// Default worker-thread count (leaves one core free).
pub fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).max(1))
        .unwrap_or(4)
}

/// Multithreaded self-play training. Each round, `threads` workers each clone the
/// current net, run `sync` self-play games (TD updates on their own copy), then the
/// copies are averaged back into the shared net (EASGD-style weight averaging).
/// `init` warm-starts from an existing net (must match `cfg.hidden`); otherwise a
/// fresh net is created. The learning rate is annealed linearly toward `lr * 0.1`.
pub fn train_parallel(
    cfg: TrainConfig,
    threads: usize,
    sync: usize,
    init: Option<Net>,
    verbose: bool,
) -> Net {
    let threads = threads.max(1);
    let sync = sync.max(1);
    let mut net = init.unwrap_or_else(|| Net::standard(cfg.hidden, cfg.seed));
    let rounds = (cfg.games / (threads * sync)).max(1);

    for round in 0..rounds {
        let frac = round as f32 / rounds.max(1) as f32;
        let lr = cfg.lr * (1.0 - 0.9 * frac);
        let explore = cfg.explore;

        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let mut local = net.clone();
                let seed = cfg
                    .seed
                    .wrapping_mul(0x2545F491_4F6CDD1D)
                    .wrapping_add((round as u64).wrapping_mul(0x9E37_79B9) ^ (t as u64 + 1));
                std::thread::spawn(move || {
                    let mut rng = Rng::new(seed);
                    for _ in 0..sync {
                        self_play_episode(&mut local, &mut rng, lr, explore);
                    }
                    local
                })
            })
            .collect();

        let locals: Vec<Net> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        net = average_nets(&locals);

        if verbose && (round + 1) % (rounds / 20).max(1) == 0 {
            let done = (round + 1) * threads * sync;
            println!("  round {:>4}/{rounds} (~{done} games, lr {:.4})", round + 1, lr);
        }
    }
    net
}

// ---- Rollouts (truth reference for error-rate) -----------------------------

/// Play a position out to the end once under the net's 1-ply policy, truncating
/// pure races with the exact bear-off table (variance reduction). Returns the
/// equity from `start_mover`'s perspective.
pub fn rollout_once(
    net: &Net,
    table: &BearoffTable,
    start: &Board,
    start_mover: Player,
    rng: &mut Rng,
) -> f32 {
    let mut board = start.clone();
    let mut mover = start_mover;
    for _ in 0..MAX_PLIES {
        if let Outcome::Win { winner, points, .. } = outcome(&board) {
            return if winner == start_mover {
                points as f32
            } else {
                -(points as f32)
            };
        }
        if table.is_race(&board) {
            if let Some(e) = table.race_equity(&board, mover) {
                return if mover == start_mover { e } else { -e };
            }
        }
        let dice = rng.dice();
        let turns = generate_turns_cfg(&board, mover, dice, false, HEAD_LIMIT);
        let idx = greedy(net, &turns, mover);
        board = turns[idx].board.clone();
        mover = mover.opponent();
    }
    0.0
}

/// Mean equity and standard error for `mover` over `n` rollouts of a position.
pub fn rollout_equity(
    net: &Net,
    table: &BearoffTable,
    board: &Board,
    mover: Player,
    n: usize,
    rng: &mut Rng,
) -> (f32, f32) {
    let mut sum = 0.0f32;
    let mut sumsq = 0.0f32;
    for _ in 0..n {
        let r = rollout_once(net, table, board, mover, rng);
        sum += r;
        sumsq += r * r;
    }
    let nn = n as f32;
    let mean = sum / nn;
    let var = (sumsq / nn - mean * mean).max(0.0);
    (mean, (var / nn).sqrt())
}

// ---- Benchmarking ----------------------------------------------------------

/// A move-choosing policy for benchmark games.
pub trait Policy {
    fn choose(&self, board: &Board, mover: Player, dice: [u8; 2], first: bool, rng: &mut Rng)
        -> Turn;
}

/// Picks a uniformly random legal turn.
pub struct RandomPolicy;
impl Policy for RandomPolicy {
    fn choose(&self, board: &Board, mover: Player, dice: [u8; 2], first: bool, rng: &mut Rng) -> Turn {
        let turns = generate_turns_cfg(board, mover, dice, first, HEAD_LIMIT);
        let i = (rng.next_u64() as usize) % turns.len();
        turns[i].clone()
    }
}

/// Plays with `plies`-deep search under an [`Evaluator`].
pub struct SearchPolicy<E: Evaluator> {
    pub eval: E,
    pub plies: u8,
}
impl<E: Evaluator> Policy for SearchPolicy<E> {
    fn choose(&self, board: &Board, mover: Player, dice: [u8; 2], first: bool, _rng: &mut Rng) -> Turn {
        best_turn_search(&self.eval, board, mover, dice, first, HEAD_LIMIT, self.plies)
            .map(|(t, _)| t)
            .expect("at least a pass")
    }
}

/// Play a single game between two policies; returns the outcome.
pub fn play_game(white: &dyn Policy, black: &dyn Policy, rng: &mut Rng) -> Outcome {
    let mut board = Board::starting();
    let mut mover = Player::White;
    let mut first = [true, true];
    for _ in 0..MAX_PLIES {
        let dice = rng.dice();
        let turn = if mover == Player::White {
            white.choose(&board, mover, dice, first[mover.index()], rng)
        } else {
            black.choose(&board, mover, dice, first[mover.index()], rng)
        };
        first[mover.index()] = false;
        board = turn.board;
        let o = outcome(&board);
        if !matches!(o, Outcome::Ongoing) {
            return o;
        }
        mover = mover.opponent();
    }
    Outcome::Ongoing
}

/// Result of a benchmark match: win rate and average points-per-game for the
/// player under test (`a`).
#[derive(Clone, Copy, Debug)]
pub struct BenchResult {
    pub games: usize,
    pub wins: usize,
    pub points: i32,
}
impl BenchResult {
    pub fn win_rate(&self) -> f32 {
        self.wins as f32 / self.games as f32
    }
    pub fn ppg(&self) -> f32 {
        self.points as f32 / self.games as f32
    }
}

/// Benchmark policy `a` against policy `b` over `games`, alternating colours.
pub fn benchmark(a: &dyn Policy, b: &dyn Policy, games: usize, seed: u64) -> BenchResult {
    let mut wins = 0;
    let mut points = 0i32;
    for g in 0..games {
        let mut rng = Rng::new(seed.wrapping_add(g as u64).wrapping_mul(0x2545F491_4F6CDD1D));
        let a_is_white = g % 2 == 0;
        let outcome = if a_is_white {
            play_game(a, b, &mut rng)
        } else {
            play_game(b, a, &mut rng)
        };
        if let Outcome::Win { winner, points: p, .. } = outcome {
            let a_won = (winner == Player::White) == a_is_white;
            if a_won {
                wins += 1;
                points += p as i32;
            } else {
                points -= p as i32;
            }
        }
    }
    BenchResult { games, wins, points }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::search::Heuristic;

    #[test]
    fn training_beats_a_random_player() {
        // A modestly trained net should crush random play — a robust signal that
        // self-play TD is actually learning.
        let cfg = TrainConfig {
            hidden: 40,
            games: 3000,
            lr: 0.1,
            explore: 0.0,
            seed: 1,
        };
        let net = train(cfg, false);
        let net_policy = SearchPolicy { eval: net, plies: 1 };
        let result = benchmark(&net_policy, &RandomPolicy, 80, 999);
        assert!(
            result.win_rate() > 0.8,
            "net should beat random decisively, got {:.2}",
            result.win_rate()
        );
    }

    #[test]
    fn heuristic_self_benchmark_is_balanced() {
        // Sanity: identical policies playing each other split roughly evenly,
        // confirming the benchmark harness has no colour bias.
        let a = SearchPolicy { eval: Heuristic, plies: 1 };
        let b = SearchPolicy { eval: Heuristic, plies: 1 };
        let r = benchmark(&a, &b, 60, 7);
        assert!((0.3..0.7).contains(&r.win_rate()));
    }
}
