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
use engine_core::search::{best_turn_search, position_equity, Evaluator};

/// Sparring + agreement/ER tooling for benchmarking against LogasAI.
pub mod logasai;

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
    /// Search depth used to SELECT moves during self-play (1 = greedy 1-ply, the
    /// classic TD(0); >1 = stronger behaviour policy à la TD-Gammon 2.0, slower).
    pub selfplay_plies: u8,
    /// Depth of the bootstrap TARGET (1 = net(s_{t+1}), the classic TD(0) target;
    /// 2 = roll-averaged best-reply lookahead of s_{t+1} à la TD-Gammon 2.1 —
    /// distills 2-ply search values into the static eval, ~20× slower per step).
    pub target_plies: u8,
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig {
            hidden: 80,
            games: 20_000,
            lr: 0.05,
            explore: 0.05,
            seed: 0xC0FFEE,
            selfplay_plies: 1,
            target_plies: 1,
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

/// Cubeless equity of a `[win, win_mars, lose_mars]` probability vector.
#[inline]
fn vec_equity(v: [f32; 3]) -> f32 {
    2.0 * v[0] - 1.0 + v[1] - v[2]
}

/// One-roll lookahead value of `board` for `to_move` (who is about to roll), as a
/// `[win, win_mars, lose_mars]` vector from `to_move`'s perspective: the weighted
/// average over the 21 rolls of the net's probabilities for the equity-best reply
/// (exact `[1, mars, 0]` when the best reply ends the game). Used as the 2-ply
/// TD target — one true ply deeper than the plain `net(s_{t+1})` bootstrap.
fn lookahead_probs(net: &Net, board: &Board, to_move: Player, first_turn: bool) -> [f32; 3] {
    let next = to_move.opponent();
    let mut acc = [0.0f32; 3];
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let weight = if d1 == d2 { 1.0 } else { 2.0 };
            let turns = generate_turns_cfg(board, to_move, [d1, d2], first_turn, HEAD_LIMIT);
            let mut best_eq = f32::NEG_INFINITY;
            let mut best_v = [0.0f32; 3];
            for t in &turns {
                let v = match outcome(&t.board) {
                    // `to_move` just played; a finished game here is their win.
                    Outcome::Win { mars, .. } => win_target(mars),
                    _ => {
                        let p = net.evaluate_board(&t.board, next);
                        [1.0 - p.win, p.lose_mars, p.win_mars]
                    }
                };
                let eq = vec_equity(v);
                if eq > best_eq {
                    best_eq = eq;
                    best_v = v;
                }
            }
            for (a, b) in acc.iter_mut().zip(best_v) {
                *a += weight * b;
            }
        }
    }
    acc.map(|a| a / 36.0)
}

/// Play one self-play game, applying TD updates in place. Returns the game length.
/// `plies` is the move-selection search depth (1 = greedy; >1 = deeper behaviour
/// policy, which shifts training toward better state distributions). `target_plies`
/// picks the TD target: 1 = the cheap `net(s_{t+1})` bootstrap; 2 = the roll-averaged
/// best-reply lookahead of s_{t+1} ([`lookahead_probs`]), distilling search into
/// the static eval.
pub fn self_play_episode(
    net: &mut Net,
    rng: &mut Rng,
    lr: f32,
    explore: f32,
    plies: u8,
    target_plies: u8,
) -> usize {
    let mut board = Board::starting();
    let mut mover = Player::White;
    let mut first = [true, true];

    for ply in 0..MAX_PLIES {
        let x_t = encode(&board, mover);
        let dice = rng.dice();
        let fst = first[mover.index()];
        let turns = generate_turns_cfg(&board, mover, dice, fst, HEAD_LIMIT);
        first[mover.index()] = false;

        let next_board = if explore > 0.0 && rng.unit() < explore {
            turns[(rng.next_u64() as usize) % turns.len()].board.clone()
        } else if plies > 1 {
            // deeper move selection (expectiminimax) — stronger behaviour policy
            best_turn_search(&*net, &board, mover, dice, fst, HEAD_LIMIT, plies)
                .map(|(t, _)| t.board)
                .unwrap_or_else(|| turns[greedy(&*net, &turns, mover)].board.clone())
        } else {
            turns[greedy(&*net, &turns, mover)].board.clone()
        };

        match outcome(&next_board) {
            Outcome::Win { mars, .. } => {
                // The mover just won; train s_t toward the realized outcome.
                net.train_step(&x_t, &win_target(mars), lr);
                return ply + 1;
            }
            // board-only `outcome` never yields Draw (training uses Traditional rules)
            Outcome::Draw => return ply + 1,
            Outcome::Ongoing => {
                // Bootstrap: target = value of s_{t+1} viewed from the mover's side
                // (plain net output, or its one-roll lookahead at target_plies >= 2).
                let opp = mover.opponent();
                let o = if target_plies >= 2 {
                    lookahead_probs(net, &next_board, opp, first[opp.index()])
                } else {
                    let raw = net.forward(&encode(&next_board, opp));
                    [raw[0], raw[1], raw[2]]
                };
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
        total_len += self_play_episode(
            &mut net,
            &mut rng,
            cfg.lr,
            cfg.explore,
            cfg.selfplay_plies,
            cfg.target_plies,
        );
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
                let plies = cfg.selfplay_plies;
                let target_plies = cfg.target_plies;
                std::thread::spawn(move || {
                    let mut rng = Rng::new(seed);
                    for _ in 0..sync {
                        self_play_episode(&mut local, &mut rng, lr, explore, plies, target_plies);
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
/// equity from `start_mover`'s perspective. The truncation applies only once
/// BOTH sides have borne off a checker: the table's race equity is win/loss
/// only (capped at ±1), so truncating while a mars is still live would
/// silently compress ±2 outcomes — the same gate `rollout_probs` uses.
pub fn rollout_once<E: Evaluator>(
    eval: &E,
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
        if table.is_race(&board)
            && board.off[Player::White.index()] > 0
            && board.off[Player::Black.index()] > 0
        {
            if let Some(e) = table.race_equity(&board, mover) {
                return if mover == start_mover { e } else { -e };
            }
        }
        let dice = rng.dice();
        let turns = generate_turns_cfg(&board, mover, dice, false, HEAD_LIMIT);
        let idx = greedy(eval, &turns, mover);
        board = turns[idx].board.clone();
        mover = mover.opponent();
    }
    0.0
}

/// Mean equity and standard error for `mover` over `n` rollouts of a position.
pub fn rollout_equity<E: Evaluator>(
    eval: &E,
    table: &BearoffTable,
    board: &Board,
    mover: Player,
    n: usize,
    rng: &mut Rng,
) -> (f32, f32) {
    let mut sum = 0.0f32;
    let mut sumsq = 0.0f32;
    for _ in 0..n {
        let r = rollout_once(eval, table, board, mover, rng);
        sum += r;
        sumsq += r * r;
    }
    let nn = n as f32;
    let mean = sum / nn;
    let var = (sumsq / nn - mean * mean).max(0.0);
    (mean, (var / nn).sqrt())
}

// ---- Phase classification & rollout labels (engine-v2 tooling) -------------

/// Game phase of a decision, for stratifying datasets and error reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Early game: the mover still has a big head stack to unload.
    Head,
    /// The sides still interact — blocking play is possible.
    Contact,
    /// Paths fully disengaged but not all checkers home yet (running game).
    Race,
    /// Both sides have every checker in the home board (bear-off race).
    Bearoff,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Head => "head",
            Phase::Contact => "contact",
            Phase::Race => "race",
            Phase::Bearoff => "bearoff",
        }
    }
}

pub use engine_core::phase::has_contact;

/// Classify a decision position (priority: bearoff > race > head > contact).
/// `Head` uses a simple heuristic: the mover still has 8+ checkers stacked on
/// the head (own path position 24) — the unload phase of the opening.
pub fn phase_of(board: &Board, mover: Player) -> Phase {
    if board.all_home(Player::White) && board.all_home(Player::Black) {
        Phase::Bearoff
    } else if !has_contact(board) {
        Phase::Race
    } else if board.own_at(mover, 24) >= 8 {
        Phase::Head
    } else {
        Phase::Contact
    }
}

/// Roll a position out `trials` times under the net's 1-ply policy and return
/// outcome probabilities `[win_oin, win_mars, lose_oin, lose_mars]` from
/// `start_mover`'s perspective (rows sum to 1). Unlike [`rollout_once`], races
/// are truncated with the exact bear-off table only once BOTH sides have borne
/// off a checker — from there a mars is impossible, so the table's race equity
/// converts exactly to `P(win)` and the label stays unbiased.
pub fn rollout_probs<E: Evaluator>(
    eval: &E,
    table: &BearoffTable,
    start: &Board,
    start_mover: Player,
    trials: usize,
    rng: &mut Rng,
) -> [f32; 4] {
    let mut acc = [0.0f64; 4];
    for _ in 0..trials {
        let mut board = start.clone();
        let mut mover = start_mover;
        let mut settled = false;
        for _ in 0..MAX_PLIES {
            if let Outcome::Win { winner, mars, .. } = outcome(&board) {
                let k = match (winner == start_mover, mars) {
                    (true, false) => 0,
                    (true, true) => 1,
                    (false, false) => 2,
                    (false, true) => 3,
                };
                acc[k] += 1.0;
                settled = true;
                break;
            }
            if table.is_race(&board)
                && board.off[Player::White.index()] > 0
                && board.off[Player::Black.index()] > 0
            {
                if let Some(e) = table.race_equity(&board, mover) {
                    let e = if mover == start_mover { e } else { -e };
                    let p_win = (f64::from(e) + 1.0) * 0.5;
                    let p_win = p_win.clamp(0.0, 1.0);
                    acc[0] += p_win;
                    acc[2] += 1.0 - p_win;
                    settled = true;
                    break;
                }
            }
            let dice = rng.dice();
            let turns = generate_turns_cfg(&board, mover, dice, false, HEAD_LIMIT);
            let idx = greedy(eval, &turns, mover);
            board = turns[idx].board.clone();
            mover = mover.opponent();
        }
        if !settled {
            // safety-cap exhaustion (never seen in practice): call it a toss-up
            acc[0] += 0.5;
            acc[2] += 0.5;
        }
    }
    let n = trials.max(1) as f64;
    [
        (acc[0] / n) as f32,
        (acc[1] / n) as f32,
        (acc[2] / n) as f32,
        (acc[3] / n) as f32,
    ]
}

/// Cubeless points equity of a `[win_oin, win_mars, lose_oin, lose_mars]`
/// probability vector (оин = 1 point, марс = 2).
pub fn probs4_equity(p: [f32; 4]) -> f32 {
    p[0] + 2.0 * p[1] - p[2] - 2.0 * p[3]
}

// ---- Measurement tooling (shared by er / diag2 / dataset) -------------------

/// Index of the turn maximising the evaluator's equity at the given lookahead
/// (`extra_plies` beyond the resulting position; 0 = static 1-ply).
pub fn best_index<E: Evaluator>(eval: &E, turns: &[Turn], mover: Player, extra_plies: u8) -> usize {
    let opp = mover.opponent();
    let mut bi = 0;
    let mut be = f32::NEG_INFINITY;
    for (i, t) in turns.iter().enumerate() {
        let e = -position_equity(eval, &t.board, opp, extra_plies, HEAD_LIMIT);
        if e > be {
            be = e;
            bi = i;
        }
    }
    bi
}

/// Roll-averaged rollout value of the position after `mover`'s candidate move
/// (`after`), from `mover`'s perspective: for each of the 21 opponent rolls the
/// opponent makes their net-greedy best reply, and the resulting leaf is scored
/// by `leaf_trials` truncated rollouts. A candidate that already ENDS the game
/// is returned at its exact point value — expanding "replies" to a finished
/// game would let the loser bear off a checker and demote a mars to an oin
/// (or, with both off-counts at 15, even flip the recorded winner).
pub fn rollout_leaf_value<E: Evaluator>(
    eval: &E,
    table: &BearoffTable,
    after: &Board,
    mover: Player,
    leaf_trials: usize,
    rng: &mut Rng,
) -> f32 {
    if let Outcome::Win { winner, points, .. } = outcome(after) {
        return if winner == mover {
            points as f32
        } else {
            -(points as f32)
        };
    }
    let opp = mover.opponent();
    let mut val = 0.0f32;
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let w = if d1 == d2 { 1.0 } else { 2.0 };
            let replies = generate_turns_cfg(after, opp, [d1, d2], false, HEAD_LIMIT);
            let ri = best_index(eval, &replies, opp, 0);
            let leaf = &replies[ri].board;
            val += w * rollout_equity(eval, table, leaf, mover, leaf_trials, rng).0;
        }
    }
    val / 36.0
}

/// Sample `m` decision positions (>1 legal play) from the net's own 1-ply
/// greedy self-play — the position source for `er`/`diag2`.
///
/// Sampling discipline (each point fixes a measured bias):
/// * literal first turns are skipped — their head-doubles exception (3-3/4-4/
///   6-6) widens the legal turn set, but labelling/scoring regenerate turns
///   with `first=false`, so recording them would score a different decision
///   than the one faced;
/// * at most `per_game` decisions per game, thinned to ~1 in 8 — without this,
///   200 "positions" are just two consecutive games' trajectories;
/// * with `min_phase > 0`, sampling continues past `m` until every phase
///   (head/contact/race/bearoff) has at least `min_phase` decisions, so
///   per-phase error reports don't rest on n=4 buckets.
pub fn sample_decisions<E: Evaluator>(
    eval: &E,
    m: usize,
    seed: u64,
    per_game: usize,
    min_phase: usize,
) -> Vec<(Board, Player, [u8; 2])> {
    use std::collections::HashMap;
    let mut rng = Rng::new(seed);
    let mut out: Vec<(Board, Player, [u8; 2])> = Vec::with_capacity(m);
    let mut phase_counts: HashMap<Phase, usize> = HashMap::new();
    let quotas_met = |pc: &HashMap<Phase, usize>| {
        [Phase::Head, Phase::Contact, Phase::Race, Phase::Bearoff]
            .iter()
            .all(|p| pc.get(p).copied().unwrap_or(0) >= min_phase)
    };
    let mut games = 0usize;
    while (out.len() < m || (min_phase > 0 && !quotas_met(&phase_counts))) && games < 4000 {
        games += 1;
        let mut board = Board::starting();
        let mut mover = Player::White;
        let mut first = [true, true];
        let mut taken = 0usize;
        for _ in 0..600 {
            if !matches!(outcome(&board), Outcome::Ongoing) {
                break;
            }
            let dice = rng.dice();
            let fst = first[mover.index()];
            let turns = generate_turns_cfg(&board, mover, dice, fst, HEAD_LIMIT);
            first[mover.index()] = false;
            if !fst && turns.len() > 1 && taken < per_game && rng.unit() < 0.125 {
                let phase = phase_of(&board, mover);
                let need_total = out.len() < m;
                let need_phase = min_phase > 0
                    && phase_counts.get(&phase).copied().unwrap_or(0) < min_phase;
                if need_total || need_phase {
                    out.push((board.clone(), mover, dice));
                    *phase_counts.entry(phase).or_insert(0) += 1;
                    taken += 1;
                }
            }
            let idx = best_index(eval, &turns, mover, 0);
            board = turns[idx].board.clone();
            mover = mover.opponent();
        }
    }
    out
}

// ---- One-line position specs & canonical turn keys --------------------------

/// Parse `"pos:count,pos:count,…"` (the player's own 1..24 path coords) into
/// `board`. Malformed tokens are skipped.
pub fn parse_side(spec: &str, player: Player, board: &mut Board) {
    for tok in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if let Some((p, c)) = tok.split_once(':') {
            if let (Ok(pos), Ok(cnt)) = (p.trim().parse::<u8>(), c.trim().parse::<u8>()) {
                if (1..=24).contains(&pos) {
                    board.place(player, pos, cnt);
                }
            }
        }
    }
}

/// `"B"`/`"black"` (any case) → Black, anything else → White.
pub fn parse_player(s: &str) -> Player {
    if s.eq_ignore_ascii_case("b") || s.eq_ignore_ascii_case("black") {
        Player::Black
    } else {
        Player::White
    }
}

/// Canonical key of a turn: the sorted multiset of its checker moves
/// (`from>to`, bear-off as `from>0`). Distinct resulting boards always get
/// distinct keys (transposing orders are already merged by the generator),
/// unlike the human notation, whose chain-collapse can be ambiguous.
pub fn turn_key(t: &Turn) -> String {
    if t.moves.is_empty() {
        return "pass".to_string();
    }
    let mut segs: Vec<String> = t
        .moves
        .iter()
        .map(|m| format!("{}>{}", m.from, if m.bear_off { 0 } else { m.to }))
        .collect();
    segs.sort_unstable();
    segs.join(",")
}

/// Inverse of [`parse_pos_line`]: one-line position spec. The `first` flag is
/// not represented — consumers relabel with `first=false`, which is why
/// samplers must never record literal first turns.
pub fn format_pos_line(board: &Board, turn: Player, dice: Option<[u8; 2]>) -> String {
    let side = |p: Player| -> String {
        let mut segs = Vec::new();
        for pos in (1..=24u8).rev() {
            let c = board.own_at(p, pos);
            if c > 0 {
                segs.push(format!("{pos}:{c}"));
            }
        }
        segs.join(",")
    };
    let mut s = format!(
        "white={} black={} turn={}",
        side(Player::White),
        side(Player::Black),
        if turn == Player::White { "W" } else { "B" }
    );
    let (wo, bo) = (
        board.off[Player::White.index()],
        board.off[Player::Black.index()],
    );
    if wo > 0 {
        s.push_str(&format!(" white-off={wo}"));
    }
    if bo > 0 {
        s.push_str(&format!(" black-off={bo}"));
    }
    if let Some(d) = dice {
        s.push_str(&format!(" dice={},{}", d[0], d[1]));
    }
    s
}

/// Parse a one-line position spec:
///   `white=24:13,18:2 black=24:15 turn=W [dice=3,1] [white-off=N] [black-off=N] [first]`
/// Returns `None` unless at least one `white=`/`black=` token is present.
/// `dice` is optional — `dataset` rows are pre-roll states; callers that need
/// a roll (relay/agree/er --load) must demand `Some` themselves.
pub fn parse_pos_line(spec: &str) -> Option<(Board, Player, Option<[u8; 2]>, bool)> {
    let mut board = Board::empty();
    let mut turn = Player::White;
    let mut dice: Option<[u8; 2]> = None;
    let mut first = false;
    let mut saw_side = false;
    for tok in spec.split_whitespace() {
        let (k, v) = tok.split_once('=').unwrap_or((tok, ""));
        match k {
            "white" => {
                saw_side = true;
                parse_side(v, Player::White, &mut board);
            }
            "black" => {
                saw_side = true;
                parse_side(v, Player::Black, &mut board);
            }
            "white-off" => board.off[Player::White.index()] = v.parse().unwrap_or(0),
            "black-off" => board.off[Player::Black.index()] = v.parse().unwrap_or(0),
            "turn" => turn = parse_player(v),
            "dice" => {
                let d: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if d.len() == 2 {
                    dice = Some([d[0], d[1]]);
                }
            }
            "first" => first = v.is_empty() || v.eq_ignore_ascii_case("true") || v == "1",
            _ => {}
        }
    }
    if !saw_side {
        return None;
    }
    Some((board, turn, dice, first))
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
    /// 95% Wilson score interval for the true win rate — robust at the sample
    /// sizes duels actually use (hundreds of games), unlike the normal interval.
    pub fn ci95(&self) -> (f32, f32) {
        let n = self.games as f64;
        if n == 0.0 {
            return (0.0, 1.0);
        }
        let p = self.wins as f64 / n;
        let z = 1.959_964; // 97.5th percentile of the standard normal
        let z2 = z * z;
        let denom = 1.0 + z2 / n;
        let centre = (p + z2 / (2.0 * n)) / denom;
        let half = (z / denom) * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
        (
            (centre - half).max(0.0) as f32,
            (centre + half).min(1.0) as f32,
        )
    }
    /// Two-sided p-value (normal approximation) against a fair 50/50 match —
    /// "could this win rate be a coin flip?".
    pub fn p_value_vs_even(&self) -> f64 {
        let n = self.games as f64;
        if n == 0.0 {
            return 1.0;
        }
        let z = ((self.wins as f64 - 0.5 * n).abs() / (0.5 * n.sqrt())).min(40.0);
        // standard normal tail via the Abramowitz–Stegun 7.1.26 erf approximation
        let t = 1.0 / (1.0 + 0.327_591_1 * (z / std::f64::consts::SQRT_2));
        let poly = t
            * (0.254_829_592
                + t * (-0.284_496_736
                    + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
        let erf = 1.0 - poly * (-(z / std::f64::consts::SQRT_2).powi(2)).exp();
        (1.0 - erf).clamp(0.0, 1.0)
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
            selfplay_plies: 1,
            target_plies: 1,
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

    #[test]
    fn phase_classification() {
        // The starting position: full head stacks, paths interleaved → the
        // mover is in the head-unload phase and contact exists.
        let start = Board::starting();
        assert!(has_contact(&start));
        assert_eq!(phase_of(&start, Player::White), Phase::Head);

        // A disengaged running game: each side still outside its home but with
        // no opponent checker ahead on its remaining path → pure race.
        let mut race = Board::empty();
        race.place(Player::White, 8, 15);
        race.place(Player::Black, 8, 15);
        assert!(!has_contact(&race));
        assert!(!race.all_home(Player::White));
        assert_eq!(phase_of(&race, Player::White), Phase::Race);
        assert_eq!(phase_of(&race, Player::Black), Phase::Race);

        // Everything home on both sides → bear-off, whoever is on roll.
        let mut bo = Board::empty();
        bo.place(Player::White, 3, 15);
        bo.place(Player::Black, 3, 15);
        assert_eq!(phase_of(&bo, Player::White), Phase::Bearoff);
        assert_eq!(phase_of(&bo, Player::Black), Phase::Bearoff);
    }

    #[test]
    fn rollout_probs_sum_to_one_and_match_equity_scale() {
        let net = Net::standard(16, 3);
        let table = BearoffTable::build_capped(3);
        let mut board = Board::empty();
        board.place(Player::White, 2, 2);
        board.place(Player::Black, 6, 2);
        board.off = [13, 13];
        let mut rng = Rng::new(11);
        let p = rollout_probs(&net, &table, &board, Player::White, 40, &mut rng);
        let sum: f32 = p.iter().sum();
        assert!((sum - 1.0).abs() < 1e-4, "probs must sum to 1, got {sum}");
        // White bears off from 2 with 2 checkers vs black's 2 on 6: white is a
        // heavy favourite, and with both sides part-borne-off a mars is impossible.
        assert!(p[0] > 0.7, "white should be winning, got {p:?}");
        assert_eq!(p[1], 0.0);
        assert_eq!(p[3], 0.0);
        assert!(probs4_equity(p) > 0.4);
    }

    #[test]
    fn rollout_once_keeps_playing_while_a_mars_is_live() {
        // White bears off its last checker on ANY roll while Black (all home,
        // nothing borne off) is being marsed — a certain +2. The old truncation
        // called race_equity as soon as both sides were all home and capped the
        // result at +1; the mars gate (both off-counts > 0) must let the
        // rollout play to the actual end and return exactly +2.
        let net = Net::standard(16, 5);
        let table = BearoffTable::build_capped(6);
        let mut board = Board::empty();
        board.place(Player::White, 1, 1);
        board.place(Player::Black, 6, 15);
        board.off = [14, 0];
        let mut rng = Rng::new(42);
        let (mean, se) = rollout_equity(&net, &table, &board, Player::White, 8, &mut rng);
        assert_eq!(mean, 2.0, "a certain mars must be +2, got {mean}");
        assert_eq!(se, 0.0);
    }

    #[test]
    fn rollout_leaf_value_short_circuits_finished_games() {
        let net = Net::standard(16, 9);
        let table = BearoffTable::build_capped(3);

        // Black has just borne off its last checker; White has 14 off + 1 left.
        // The value for Black must be exactly +1 (oin) — no phantom White
        // "reply" that would bear off the last checker and flip the winner.
        let mut won = Board::empty();
        won.place(Player::White, 1, 1);
        won.off = [14, 15];
        let mut rng = Rng::new(1);
        let v = rollout_leaf_value(&net, &table, &won, Player::Black, 4, &mut rng);
        assert_eq!(v, 1.0, "Black's finished oin must be +1, got {v}");

        // White wins a mars (opponent all home, 0 off): exactly +2 — a phantom
        // Black reply would demote it to ~+1.
        let mut mars = Board::empty();
        mars.place(Player::Black, 6, 15);
        mars.off = [15, 0];
        let v = rollout_leaf_value(&net, &table, &mars, Player::White, 4, &mut rng);
        assert_eq!(v, 2.0, "White's finished mars must be +2, got {v}");
    }

    #[test]
    fn sample_decisions_skips_first_turns_and_spans_many_games() {
        let net = Net::standard(16, 21);
        let positions = sample_decisions(&net, 40, 7, 4, 0);
        assert_eq!(positions.len(), 40);
        // Literal first turns are excluded, so the starting position (each
        // game's only White first-turn decision) must never be recorded.
        let start = Board::starting();
        assert!(
            positions.iter().all(|(b, _, _)| *b != start),
            "first-turn decisions must be skipped"
        );
        // With a per-game cap of 4, 40 decisions need at least 10 games.
        let unique: std::collections::HashSet<_> =
            positions.iter().map(|(b, p, _)| (b.clone(), *p)).collect();
        assert!(unique.len() > 30, "sample should span many games, not one trajectory");
    }

    #[test]
    fn sample_decisions_stratified_fills_phase_quotas() {
        let net = Net::standard(16, 33);
        let positions = sample_decisions(&net, 24, 13, 6, 4);
        let mut counts: std::collections::HashMap<Phase, usize> = Default::default();
        for (b, p, _) in &positions {
            *counts.entry(phase_of(b, *p)).or_insert(0) += 1;
        }
        for phase in [Phase::Head, Phase::Contact, Phase::Race, Phase::Bearoff] {
            assert!(
                counts.get(&phase).copied().unwrap_or(0) >= 4,
                "phase {phase:?} quota unfilled: {counts:?}"
            );
        }
    }

    #[test]
    fn pos_line_round_trips_exactly() {
        let mut board = Board::empty();
        board.place(Player::White, 24, 11);
        board.place(Player::White, 13, 2);
        board.place(Player::White, 2, 1);
        board.place(Player::Black, 24, 10);
        board.place(Player::Black, 7, 3);
        board.off = [1, 2];

        // With dice.
        let line = format_pos_line(&board, Player::Black, Some([6, 2]));
        let (b2, t2, d2, f2) = parse_pos_line(&line).expect("parseable");
        assert_eq!(b2, board);
        assert_eq!(t2, Player::Black);
        assert_eq!(d2, Some([6, 2]));
        assert!(!f2);

        // Without dice (dataset rows) — must still parse, dice = None.
        let line = format_pos_line(&board, Player::White, None);
        let (b3, t3, d3, _) = parse_pos_line(&line).expect("dataset rows must be parseable");
        assert_eq!(b3, board);
        assert_eq!(t3, Player::White);
        assert_eq!(d3, None);

        // Garbage without any side token is rejected.
        assert!(parse_pos_line("dice=3,1 turn=W").is_none());
    }

    #[test]
    fn wilson_ci_and_p_value_sanity() {
        let even = BenchResult { games: 400, wins: 200, points: 0 };
        let (lo, hi) = even.ci95();
        assert!(lo < 0.5 && hi > 0.5);
        assert!(even.p_value_vs_even() > 0.9);

        let strong = BenchResult { games: 400, wins: 240, points: 80 };
        let (lo, _) = strong.ci95();
        assert!(lo > 0.5, "60% over 400 games is significantly above even");
        assert!(strong.p_value_vs_even() < 0.01);
    }
}
