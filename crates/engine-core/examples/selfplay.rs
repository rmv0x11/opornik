//! Watch the baseline AI play a full game of длинные нарды against itself.
//!
//! Run with: `cargo run -p engine-core --example selfplay`

use engine_core::{best_turn, Board, GameState, Outcome, Player, Rules};

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

fn pip_line(board: &Board) -> String {
    format!(
        "White pip {:3} (off {:2})  |  Black pip {:3} (off {:2})",
        board.pip(Player::White),
        board.off[Player::White.index()],
        board.pip(Player::Black),
        board.off[Player::Black.index()],
    )
}

fn main() {
    // Seed from the process address of a local (good enough for a demo); falls
    // back to a fixed value for reproducibility if you prefer.
    let seed = 0xC0FFEE_u64 ^ (&main as *const _ as u64);
    let mut rng = Rng(seed | 1);
    let mut game = GameState::new(Rules::default());

    println!("Длинные нарды — self-play (baseline AI)\n{}", pip_line(&game.board));

    while !game.is_over() && game.turn_number < 2000 {
        let (d1, d2) = (rng.die(), rng.die());
        game.set_dice(d1, d2);
        let mover = game.turn;
        let turn = best_turn(&game).expect("a legal turn");

        let desc = if turn.is_pass() {
            "pass".to_string()
        } else {
            turn.moves
                .iter()
                .map(|m| {
                    if m.bear_off {
                        format!("{}/off", m.from)
                    } else {
                        format!("{}/{}", m.from, m.to)
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };

        let head_note = if !game.first_turn_done[mover.index()] { " (first)" } else { "" };
        println!(
            "{:>4}. {:?}{} {}-{}: {}",
            game.turn_number + 1,
            mover,
            head_note,
            d1,
            d2,
            desc
        );
        game.apply_turn(&turn);
    }

    println!("\n{}", pip_line(&game.board));
    match game.outcome() {
        Outcome::Win { winner, mars, points } => {
            println!(
                "Result: {winner:?} wins {} ({} point{}) after {} turns",
                if mars { "MARS (марс)" } else { "single (оин)" },
                points,
                if points == 1 { "" } else { "s" },
                game.turn_number
            );
        }
        Outcome::Draw => println!("Result: draw (ничья)"),
        Outcome::Ongoing => println!("Result: unfinished (turn cap reached)"),
    }
}
