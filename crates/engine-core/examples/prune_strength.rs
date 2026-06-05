//! Strength validation for the forward-pruning fix.
//!
//! Plays a self-play game (ply-1 net) to visit realistic positions, and at each
//! decision compares the FULL ply-3 search (no pruning) against the BOUNDED
//! ply-3 search (root=ROOT_WIDTH, deep=SEARCH_WIDTH):
//!   * how often the bounded search returns the SAME move, and
//!   * the equity gap (full-best minus the bounded-pick's full equity) when it
//!     differs — i.e. the real strength cost, measured in equity (points).
//!
//! Run: `cargo run -p engine-core --release --example prune_strength`

use engine_core::{
    best_turn_search_width, generate_turns_cfg, position_equity_width, GameState, Net, Rules,
    ROOT_WIDTH, SEARCH_WIDTH,

};

const NET_BYTES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/nardy-net.bin"));

struct Rng(u64);
impl Rng {
    fn die(&mut self) -> u8 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x % 6) as u8 + 1
    }
}

fn main() {
    let net = Net::from_bytes(NET_BYTES).expect("net");
    let rules = Rules::default();
    let mut rng = Rng(0x5EED_1234_ABCD);
    let mut game = GameState::new(rules);

    let mut decisions = 0u32;
    let mut same = 0u32;
    let mut total_loss = 0.0f64; // sum of equity gap (full-best minus bounded-pick, in full eval)
    let mut max_loss = 0.0f32;
    let mut moves_made = 0u32;
    let mut skipped_huge = 0u32;

    // The FULL (unpruned) ply-3 baseline is what is expensive: a single
    // high-branching double can be minutes. We bound the experiment by skipping
    // the FULL comparison when the root branching exceeds `MAX_FULL_TURNS` (the
    // dedicated search_bench already covers those worst cases exactly), and we
    // sample a manageable number of decisions across the rest, which still
    // includes plenty of moderate doubles — exactly where pruning could differ.
    const MAX_FULL_TURNS: usize = 30;
    const SAMPLE: u32 = 60;

    while decisions < SAMPLE && moves_made < 4000 {
        if game.is_over() {
            game = GameState::new(rules);
        }
        let dice = [rng.die(), rng.die()];
        game.set_dice(dice[0], dice[1]);
        let first = game.is_first_turn() && game.rules.head_doubles_exception;
        let mover = game.turn;
        let hl = game.rules.head_limit;
        let turns = generate_turns_cfg(&game.board, mover, dice, first, hl);

        if turns.len() > MAX_FULL_TURNS {
            skipped_huge += 1;
        }
        if turns.len() > 1 && turns.len() <= MAX_FULL_TURNS {
            // FULL ply-3 best (no pruning at any node).
            let (full_turn, full_eq) =
                best_turn_search_width(&net, &game.board, mover, dice, first, hl, 3, None, None)
                    .unwrap();
            // BOUNDED ply-3 best (production widths).
            let (pruned_turn, _) = best_turn_search_width(
                &net,
                &game.board,
                mover,
                dice,
                first,
                hl,
                3,
                Some(SEARCH_WIDTH),
                Some(ROOT_WIDTH),
            )
            .unwrap();

            decisions += 1;
            if pruned_turn.board == full_turn.board {
                same += 1;
                println!("  [{decisions:>2}/{SAMPLE}] dice {dice:?} turns {:>2}: SAME", turns.len());
            } else {
                // Score the bounded pick under the SAME full ply-3 eval to get the
                // true equity given up by playing it instead of the full best.
                let pruned_full_eq = -position_equity_width(
                    &net,
                    &pruned_turn.board,
                    mover.opponent(),
                    2,
                    hl,
                    None,
                );
                let loss = full_eq - pruned_full_eq;
                total_loss += loss as f64;
                if loss > max_loss {
                    max_loss = loss;
                }
                println!(
                    "  [{decisions:>2}/{SAMPLE}] dice {dice:?} turns {:>2}: DIFFERENT, eq loss {loss:.5}",
                    turns.len()
                );
            }
        }

        // Advance with a cheap ply-1 move to keep the game flowing.
        let (turn, _) =
            best_turn_search_width(&net, &game.board, mover, dice, first, hl, 1, None, None)
                .unwrap();
        game.apply_turn(&turn);
        moves_made += 1;
    }

    println!("\n=== ply-3 bounded (root={ROOT_WIDTH}, deep={SEARCH_WIDTH}) vs FULL ply-3 ===");
    println!(
        "decisions sampled (2..={MAX_FULL_TURNS} legal turns): {decisions}   \
         (skipped {skipped_huge} huge-branching decisions whose FULL ply-3 is minutes each)"
    );
    println!(
        "same move chosen: {same}/{decisions} = {:.1}%",
        100.0 * same as f64 / decisions as f64
    );
    let diff = decisions - same;
    println!("differed: {diff}");
    if diff > 0 {
        println!(
            "mean equity loss over ALL decisions: {:.5} pts",
            total_loss / decisions as f64
        );
        println!(
            "mean equity loss over the DIFFERING decisions: {:.5} pts",
            total_loss / diff as f64
        );
        println!("max single equity loss: {max_loss:.5} pts");
    } else {
        println!("no differences: bounded search matched full ply-3 on every sampled decision");
    }
}
