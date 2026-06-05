//! Profiling bench for the n-ply expectiminimax search.
//!
//! Times `best_turn_search` at ply=2 vs ply=3 on a busy midgame position with
//! many legal turns, counts leaf evaluations (net forward passes), and reports
//! the per-roll legal-turn branching factor.
//!
//! Run with: `cargo run -p engine-core --release --example search_bench`

use std::cell::Cell;
use std::time::Instant;

use engine_core::{
    best_turn_search, best_turn_search_width, generate_turns_cfg, Board, Evaluator, Net, Player,
    Turn, ROOT_WIDTH, SEARCH_WIDTH,
};

/// Wraps an evaluator and counts how many times the static eval (a leaf) is hit.
struct Counting<'a, E: Evaluator> {
    inner: &'a E,
    calls: Cell<u64>,
}
impl<'a, E: Evaluator> Counting<'a, E> {
    fn new(inner: &'a E) -> Self {
        Counting { inner, calls: Cell::new(0) }
    }
}
impl<E: Evaluator> Evaluator for Counting<'_, E> {
    fn equity(&self, b: &Board, m: Player) -> f32 {
        self.calls.set(self.calls.get() + 1);
        self.inner.equity(b, m)
    }
}

const NET_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/nardy-net.bin"));

/// A busy, contact-heavy midgame position: both sides spread across the outfield
/// with checkers still on the head. Lots of landing spots -> many legal turns,
/// no early bear-off short-circuit. (Total 15 per side; validated below.)
fn busy_midgame() -> Board {
    let mut b = Board::empty();
    // White: spread across the board, some still on head (disjoint cells from Black).
    b.place(Player::White, 24, 4); // head
    b.place(Player::White, 21, 2);
    b.place(Player::White, 19, 2);
    b.place(Player::White, 18, 1);
    b.place(Player::White, 15, 2);
    b.place(Player::White, 13, 2);
    b.place(Player::White, 9, 2);
    // Black: mirrored-ish spread, also still developing.
    b.place(Player::Black, 24, 4); // head
    b.place(Player::Black, 23, 2);
    b.place(Player::Black, 20, 2);
    b.place(Player::Black, 16, 2);
    b.place(Player::Black, 14, 1);
    b.place(Player::Black, 10, 2);
    b.place(Player::Black, 5, 2);
    b
}

fn branching_stats(board: &Board, mover: Player, head_limit: Option<u8>) {
    let mut total = 0usize;
    let mut max = 0usize;
    let mut weighted = 0.0f64; // expected #turns weighted by roll probability
    for d1 in 1..=6u8 {
        for d2 in d1..=6u8 {
            let w = if d1 == d2 { 1.0 } else { 2.0 };
            let n = generate_turns_cfg(board, mover, [d1, d2], false, head_limit).len();
            total += n;
            max = max.max(n);
            weighted += w * n as f64;
        }
    }
    println!(
        "  per-roll legal turns: sum over 21 distinct rolls = {total}, max single roll = {max}, \
         prob-weighted mean = {:.1}",
        weighted / 36.0
    );
}

#[allow(clippy::too_many_arguments)]
fn run(
    label: &str,
    net: &Net,
    board: &Board,
    mover: Player,
    dice: [u8; 2],
    head_limit: Option<u8>,
    ply: u8,
    deep_width: Option<usize>,
    root_width: Option<usize>,
) -> (Turn, f32) {
    let counting = Counting::new(net);
    let t0 = Instant::now();
    let (turn, eq) = best_turn_search_width(
        &counting, board, mover, dice, false, head_limit, ply, deep_width, root_width,
    )
    .unwrap();
    let dt = t0.elapsed();
    let calls = counting.calls.get();
    println!(
        "  {label:<22} {:>9.2} ms   leaf evals = {:>12}   equity = {:+.3}",
        dt.as_secs_f64() * 1000.0,
        calls,
        eq,
    );
    (turn, eq)
}

// Reference to the default play path so the bench mirrors production exactly.
#[allow(dead_code)]
fn default_path(net: &Net, board: &Board, mover: Player, dice: [u8; 2], head_limit: Option<u8>) {
    let _ = best_turn_search(net, board, mover, dice, false, head_limit, 3);
}

fn main() {
    let net = Net::from_bytes(NET_BYTES).expect("bundled net");
    let board = busy_midgame();
    assert!(board.is_valid(), "bench position must have 15 checkers per side");
    let head_limit = Some(1);
    let mover = Player::White;

    println!("Busy midgame position, White to move (head limit 1):");
    println!(
        "  White pip {}  Black pip {}",
        board.pip(Player::White),
        board.pip(Player::Black)
    );
    branching_stats(&board, mover, head_limit);

    // Use a worst-case-ish roll for the *root* (a double => often the most turns,
    // and the root's chosen dice are fixed, so a fat root multiplies everything).
    // We bench a representative non-double and a double.
    for dice in [[6, 5], [3, 3]] {
        let root_turns = generate_turns_cfg(&board, mover, dice, false, head_limit).len();
        println!("\nRoot dice {dice:?}  (root legal turns = {root_turns}):");
        run("ply=1", &net, &board, mover, dice, head_limit, 1, None, None);
        run("ply=2 full", &net, &board, mover, dice, head_limit, 2, None, None);
        let (t_full, e_full) = run(
            "ply=3 FULL (old)",
            &net,
            &board,
            mover,
            dice,
            head_limit,
            3,
            None,
            None,
        );
        let (t_prune, e_prune) = run(
            &format!("ply=3 root{ROOT_WIDTH}/deep{SEARCH_WIDTH}"),
            &net,
            &board,
            mover,
            dice,
            head_limit,
            3,
            Some(SEARCH_WIDTH),
            Some(ROOT_WIDTH),
        );
        let same = t_full.board == t_prune.board;
        println!(
            "  -> pruned picks {} move; full eq {:+.3} vs pruned eq {:+.3} (|Δ|={:.4})",
            if same { "the SAME" } else { "a DIFFERENT" },
            e_full,
            e_prune,
            (e_full - e_prune).abs(),
        );
    }
}
