//! End-to-end self-play smoke tests: play full games with a seeded PRNG and the
//! baseline AI, asserting every game terminates with a valid outcome and that
//! checkers are conserved on every turn.

use engine_core::{best_turn, GameState, Outcome, Player, Rules, N_CHECKERS};

/// Tiny deterministic xorshift PRNG (keeps the crate dependency-free).
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn die(&mut self) -> u8 {
        (self.next_u64() % 6) as u8 + 1
    }
}

fn play_one(seed: u64) -> (Outcome, u32) {
    let mut rng = Rng(seed);
    let mut game = GameState::new(Rules::default());

    for _ in 0..2000 {
        if game.is_over() {
            break;
        }
        game.set_dice(rng.die(), rng.die());
        let turn = best_turn(&game).expect("generation always yields at least a pass");

        // Invariant: a turn never changes the total checker count of the mover.
        let mover = game.turn;
        let before = game.board.checkers_on_board(mover) + game.board.off[mover.index()];
        assert_eq!(before, N_CHECKERS);

        game.apply_turn(&turn);

        let after =
            turn.board.checkers_on_board(mover) + turn.board.off[mover.index()];
        assert_eq!(after, N_CHECKERS, "checkers conserved for {mover:?}");
    }

    (game.outcome(), game.turn_number)
}

#[test]
fn self_play_games_terminate_with_a_winner() {
    let mut total_turns = 0u32;
    let mut whites = 0;
    let mut blacks = 0;
    let mut marses = 0;
    let games: u64 = 50;

    for seed in 1..=games {
        let (outcome, turns) = play_one(seed.wrapping_mul(0x9E3779B97F4A7C15));
        match outcome {
            Outcome::Win { winner, mars, points } => {
                assert!(points == 1 || points == 2);
                match winner {
                    Player::White => whites += 1,
                    Player::Black => blacks += 1,
                }
                if mars {
                    marses += 1;
                }
                total_turns += turns;
            }
            Outcome::Draw => total_turns += turns,
            Outcome::Ongoing => panic!("game {seed} did not finish"),
        }
    }

    // Sanity: both sides win some games and the average game is a sane length.
    assert!(whites > 0 && blacks > 0, "one side never won (W:{whites} B:{blacks})");
    let avg = total_turns as f32 / games as f32;
    assert!(
        (10.0..200.0).contains(&avg),
        "implausible average game length: {avg}"
    );
    println!("self-play: {games} games, avg {avg:.1} turns, W:{whites} B:{blacks}, mars:{marses}");
}
